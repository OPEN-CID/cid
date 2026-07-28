//! Network allow-list for Autonomous-mode commands (review_prompt.md /
//! Gemini-checklist follow-up: "sandboxed commands can still make
//! unconfined outgoing network calls" — true before this module).
//!
//! # What this is
//!
//! A local HTTP/HTTPS forward proxy that only permits connections to a
//! configured set of allowed hosts (`git`/`npm`/`cargo`/etc.'s usual
//! remotes by default). `SandboxManager::ensure_network_guard` starts it
//! lazily and returns its address; `execute_sandboxed` sets `HTTP_PROXY`/
//! `HTTPS_PROXY` (and lowercase variants, since tool support is
//! inconsistent) in the sandboxed command's environment.
//!
//! # What this is NOT — read before trusting it as a real security boundary
//!
//! This is **application-layer, not kernel-enforced**, unlike the
//! filesystem sandbox layers documented in `SECURITY.md`. It relies on the
//! spawned process actually honoring `HTTP_PROXY`/`HTTPS_PROXY` — which
//! `git`, `npm`, `pip`, `cargo`, and `curl` do by default, but:
//!
//! - A process using raw sockets, a hardcoded proxy bypass, or a language
//!   runtime that ignores these env vars is not confined by this at all.
//! - DNS-over-HTTPS or a tool that resolves and connects without going
//!   through the configured proxy bypasses it.
//! - This was deliberately chosen over `unshare -n` (full network
//!   denial) because full denial breaks the common case this project
//!   actually needs to support — `git push`, `npm install`, `cargo build`
//!   all need network access. An allow-list is the only version of
//!   "confined but still usable."
//!
//! Treat this the same way `SECURITY.md` treats the Windows filesystem
//! gap: a real, honest mitigation, not a guarantee.

use std::collections::HashSet;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Hosts Autonomous-mode commands can reach by default — the common,
/// legitimate remotes `git`/`npm`/`pip`/`cargo` actually need. Matched by
/// exact hostname or as a suffix of the requested host (so
/// `githubusercontent.com` also covers `raw.githubusercontent.com` and
/// `objects.githubusercontent.com`).
pub const DEFAULT_ALLOWED_HOSTS: &[&str] = &[
    "github.com",
    "api.github.com",
    "githubusercontent.com",
    "gitlab.com",
    "bitbucket.org",
    "registry.npmjs.org",
    "npmjs.org",
    "pypi.org",
    "files.pythonhosted.org",
    "crates.io",
    "static.crates.io",
    "index.crates.io",
];

#[derive(Clone)]
pub struct NetGuard {
    allowed_hosts: Arc<RwLock<HashSet<String>>>,
    addr: std::net::SocketAddr,
}

impl NetGuard {
    /// Binds a local proxy immediately (so `addr()` is always valid) and
    /// spawns the accept loop in the background. `allowed_hosts` should be
    /// lowercase; matching is case-insensitive on the request side regardless.
    pub async fn start(allowed_hosts: impl IntoIterator<Item = String>) -> anyhow::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let allowed_hosts = Arc::new(RwLock::new(
            allowed_hosts
                .into_iter()
                .map(|h| h.to_lowercase())
                .collect(),
        ));

        let guard = Self {
            allowed_hosts,
            addr,
        };
        let accept_guard = guard.clone();
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let g = accept_guard.clone();
                        tokio::spawn(async move {
                            if let Err(e) = g.handle_connection(stream).await {
                                debug!("net_guard connection ended: {e}");
                            }
                        });
                    }
                    Err(e) => {
                        warn!("net_guard accept loop error: {e}");
                    }
                }
            }
        });

        Ok(guard)
    }

    /// `http://127.0.0.1:PORT` — set as `HTTP_PROXY`/`HTTPS_PROXY` for a
    /// sandboxed command's environment.
    pub fn proxy_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub async fn set_allowed_hosts(&self, hosts: impl IntoIterator<Item = String>) {
        let mut guard = self.allowed_hosts.write().await;
        *guard = hosts.into_iter().map(|h| h.to_lowercase()).collect();
    }

    pub async fn allowed_hosts(&self) -> Vec<String> {
        let guard = self.allowed_hosts.read().await;
        let mut v: Vec<String> = guard.iter().cloned().collect();
        v.sort();
        v
    }

    async fn is_allowed(&self, host: &str) -> bool {
        let host = host.to_lowercase();
        let guard = self.allowed_hosts.read().await;
        guard
            .iter()
            .any(|allowed| host == *allowed || host.ends_with(&format!(".{allowed}")))
    }

    async fn handle_connection(&self, mut client: TcpStream) -> anyhow::Result<()> {
        // Read just the request line (and enough headers to find it, for
        // plain-HTTP forwarding) — proxy requests are small, a modest cap
        // avoids reading an unbounded amount from an unauthenticated local
        // socket before we've even decided whether to allow it.
        let mut buf = Vec::with_capacity(4096);
        let mut chunk = [0u8; 1024];
        let head_end = loop {
            let n = client.read(&mut chunk).await?;
            if n == 0 {
                return Ok(());
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
                break pos;
            }
            if buf.len() > 64 * 1024 {
                anyhow::bail!("request header too large");
            }
        };

        let head = String::from_utf8_lossy(&buf[..head_end]);
        let request_line = head.lines().next().unwrap_or_default();
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or_default();
        let target = parts.next().unwrap_or_default();

        if method.eq_ignore_ascii_case("CONNECT") {
            self.handle_connect(client, target).await
        } else {
            self.handle_plain_http(client, target, &buf).await
        }
    }

    async fn handle_connect(&self, mut client: TcpStream, target: &str) -> anyhow::Result<()> {
        let (host, port) = split_host_port(target, 443);
        if !self.is_allowed(&host).await {
            warn!("net_guard: denied CONNECT to {host}:{port} (not on the allow-list)");
            client
                .write_all(b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n")
                .await?;
            return Ok(());
        }

        let mut upstream = match TcpStream::connect((host.as_str(), port)).await {
            Ok(s) => s,
            Err(e) => {
                client
                    .write_all(b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\n\r\n")
                    .await?;
                return Err(e.into());
            }
        };
        client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;
        tokio::io::copy_bidirectional(&mut client, &mut upstream).await?;
        Ok(())
    }

    async fn handle_plain_http(
        &self,
        mut client: TcpStream,
        target: &str,
        already_read: &[u8],
    ) -> anyhow::Result<()> {
        // Absolute-form request target, e.g. `http://host/path` — proxies
        // receive this form for plain (non-CONNECT) HTTP requests.
        let host_part = target
            .split("://")
            .nth(1)
            .and_then(|rest| rest.split('/').next())
            .unwrap_or(target);
        let (host, port) = split_host_port(host_part, 80);

        if !self.is_allowed(&host).await {
            warn!("net_guard: denied HTTP request to {host} (not on the allow-list)");
            client
                .write_all(b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n")
                .await?;
            return Ok(());
        }

        let mut upstream = match TcpStream::connect((host.as_str(), port)).await {
            Ok(s) => s,
            Err(e) => {
                client
                    .write_all(b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\n\r\n")
                    .await?;
                return Err(e.into());
            }
        };
        upstream.write_all(already_read).await?;
        tokio::io::copy_bidirectional(&mut client, &mut upstream).await?;
        Ok(())
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn split_host_port(input: &str, default_port: u16) -> (String, u16) {
    match input.rsplit_once(':') {
        Some((h, p)) => match p.parse::<u16>() {
            Ok(port) => (h.to_string(), port),
            Err(_) => (input.to_string(), default_port),
        },
        None => (input.to_string(), default_port),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_allowed_host_is_reachable_through_the_proxy() {
        // A tiny local "upstream" standing in for an allow-listed host —
        // is_allowed matches by hostname, so we allow-list 127.0.0.1
        // itself and point at it directly.
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut sock, _) = upstream.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf).await;
            let _ = sock
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .await;
        });

        let guard = NetGuard::start(vec!["127.0.0.1".to_string()])
            .await
            .unwrap();
        let client = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all(guard.proxy_url()).unwrap())
            .build()
            .unwrap();

        let resp = client
            .get(format!("http://127.0.0.1:{}/", upstream_addr.port()))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        assert_eq!(resp.text().await.unwrap(), "ok");
    }

    #[tokio::test]
    async fn a_host_not_on_the_allow_list_is_denied() {
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream.local_addr().unwrap();
        // No allow-list entries at all — everything must be denied.
        let guard = NetGuard::start(Vec::<String>::new()).await.unwrap();
        let client = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all(guard.proxy_url()).unwrap())
            .build()
            .unwrap();

        let resp = client
            .get(format!("http://127.0.0.1:{}/", upstream_addr.port()))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 403);
    }

    #[tokio::test]
    async fn allow_list_can_be_updated_at_runtime() {
        let guard = NetGuard::start(Vec::<String>::new()).await.unwrap();
        assert!(!guard.is_allowed("example.com").await);

        guard
            .set_allowed_hosts(vec!["example.com".to_string()])
            .await;
        assert!(guard.is_allowed("example.com").await);
        assert!(
            guard.is_allowed("sub.example.com").await,
            "a subdomain of an allow-listed host must also be allowed"
        );
        assert!(!guard.is_allowed("evil-example.com").await);
    }

    #[test]
    fn split_host_port_handles_missing_and_present_ports() {
        assert_eq!(
            split_host_port("github.com", 443),
            ("github.com".to_string(), 443)
        );
        assert_eq!(
            split_host_port("github.com:8443", 443),
            ("github.com".to_string(), 8443)
        );
    }
}
