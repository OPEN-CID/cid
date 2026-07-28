//! Thin JSON-RPC 2.0 client over Core's HTTP surface.
//!
//! Mirrors `src/lib/api.ts`'s HTTP fallback path — the TUI is a client of the
//! same wire protocol every other shell uses, per Part 15's "one Core, many
//! surfaces." Polling over HTTP rather than a WebSocket, documented as an ADR
//! (`docs/adr/0014-cli-tui-shell.md`): simpler to reason about in a terminal
//! event loop, and the TUI's own refresh cadence already bounds staleness.

use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::Value;

#[derive(Clone)]
pub struct CoreClient {
    http: reqwest::Client,
    base_url: String,
    auth_token: Option<String>,
    request_id: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl CoreClient {
    pub fn new(host: &str, port: u16, auth_token: Option<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            base_url: format!("http://{host}:{port}"),
            auth_token,
            request_id: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }

    pub async fn health(&self) -> Result<Value> {
        let resp = self
            .http
            .get(format!("{}/health", self.base_url))
            .send()
            .await?;
        Ok(resp.json().await?)
    }

    pub async fn call<P: Serialize>(&self, method: &str, params: P) -> Result<Value> {
        let id = self
            .request_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id.to_string(),
            "method": method,
            "params": params,
        });

        let mut req = self
            .http
            .post(format!("{}/api/rpc", self.base_url))
            .json(&body);
        if let Some(token) = &self.auth_token {
            req = req.bearer_auth(token);
        }

        let resp = req.send().await?;
        let status = resp.status();
        let json: Value = resp.json().await?;

        if !status.is_success() {
            let msg = json["error"]["message"]
                .as_str()
                .unwrap_or("request failed");
            bail!("{method}: {msg} (HTTP {status})");
        }
        if let Some(err) = json.get("error") {
            let msg = err["message"].as_str().unwrap_or("unknown error");
            bail!("{method}: {msg}");
        }
        Ok(json.get("result").cloned().unwrap_or(Value::Null))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_is_built_from_host_and_port() {
        let client = CoreClient::new("127.0.0.1", 5919, None);
        assert_eq!(client.base_url, "http://127.0.0.1:5919");
    }

    #[test]
    fn request_ids_increase_monotonically() {
        let client = CoreClient::new("127.0.0.1", 5919, None);
        let a = client
            .request_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let b = client
            .request_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        assert!(b > a);
    }
}
