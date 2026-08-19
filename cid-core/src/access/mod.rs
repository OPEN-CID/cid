/*!
 * Access control for Core when it is reachable beyond localhost (Part 15, Phase 2).
 *
 * Phase 0 and 1 assumed Core was bound to loopback, where the OS is the access
 * boundary. The Web Shell makes remote binding a supported deployment, so the
 * rules become explicit:
 *
 *   - Bound to loopback  → open by default, same as before.
 *   - Bound to any other interface → a bearer token is REQUIRED. Core refuses to
 *     start otherwise rather than silently exposing an unauthenticated RPC
 *     surface that can read files, run commands, and reach model credentials.
 *
 * This is deliberately minimal — a shared token, not user accounts. Multi-user
 * membership is Phase 3 and layers on top of this rather than replacing it.
 */

use std::net::IpAddr;

use anyhow::{bail, Result};

/// How Core decides whether a request may proceed.
#[derive(Debug, Clone)]
pub struct AccessPolicy {
    /// Required bearer token, when one applies.
    token: Option<String>,
    /// Whether the listening address is loopback-only.
    loopback_only: bool,
    /// Origins permitted by CORS. Empty means the built-in local defaults.
    pub allowed_origins: Vec<String>,
}

pub const DEFAULT_ORIGINS: &[&str] = &[
    "http://localhost:1420",
    "http://127.0.0.1:1420",
    "tauri://localhost",
    "https://tauri.localhost",
];

impl AccessPolicy {
    /// Build the policy for a bind address, rejecting the unsafe combination of
    /// a non-loopback bind with no token.
    pub fn new(
        bind_ip: IpAddr,
        token: Option<String>,
        allowed_origins: Vec<String>,
    ) -> Result<Self> {
        let loopback_only = bind_ip.is_loopback();
        let token = token.filter(|t| !t.trim().is_empty());

        if !loopback_only && token.is_none() {
            bail!(
                "Core is bound to {bind_ip}, which is reachable from other machines, but no \
                 access token was provided. Pass --auth-token <token> (or set CID_AUTH_TOKEN), \
                 or bind to 127.0.0.1. Refusing to expose an unauthenticated RPC surface."
            );
        }

        Ok(Self {
            token,
            loopback_only,
            allowed_origins: if allowed_origins.is_empty() {
                DEFAULT_ORIGINS.iter().map(|s| s.to_string()).collect()
            } else {
                allowed_origins
            },
        })
    }

    /// A loopback policy with no token — the default local development shape.
    pub fn local_only() -> Self {
        Self {
            token: None,
            loopback_only: true,
            allowed_origins: DEFAULT_ORIGINS.iter().map(|s| s.to_string()).collect(),
        }
    }

    pub fn requires_auth(&self) -> bool {
        self.token.is_some()
    }

    pub fn is_loopback_only(&self) -> bool {
        self.loopback_only
    }

    /// Check a request's `Authorization` header.
    ///
    /// Comparison is constant-time so a wrong token cannot be recovered by
    /// timing the response.
    pub fn authorize(&self, auth_header: Option<&str>) -> AccessDecision {
        let expected = match &self.token {
            None => return AccessDecision::Allowed,
            Some(t) => t,
        };

        let presented = match auth_header {
            Some(h) => h
                .strip_prefix("Bearer ")
                .or_else(|| h.strip_prefix("bearer ")),
            None => None,
        };

        match presented {
            None => AccessDecision::Denied("Missing Authorization: Bearer <token> header"),
            Some(p) if constant_time_eq(p.trim().as_bytes(), expected.as_bytes()) => {
                AccessDecision::Allowed
            }
            Some(_) => AccessDecision::Denied("Invalid access token"),
        }
    }

    /// Check a WebSocket handshake, which cannot rely on `Authorization`.
    ///
    /// A browser has no way to set request headers on `new WebSocket(...)` —
    /// the subprotocol list is the only part of the handshake it controls. So a
    /// browser client presents its token as `cid.bearer.<base64url>` and Core
    /// accepts it there in addition to (never instead of) the header, which
    /// native clients still use. The token is base64url-encoded because a
    /// subprotocol must be an HTTP token: no spaces, commas, or non-ASCII.
    ///
    /// This is the same secret over the same TLS-protected handshake as the
    /// header, and unlike a `?token=` query parameter it stays out of URLs,
    /// access logs, and `Referer`. See `SECURITY.md` §2.
    pub fn authorize_websocket(
        &self,
        auth_header: Option<&str>,
        protocol_header: Option<&str>,
    ) -> WsAuthorization {
        let offered = protocol_header.and_then(|h| {
            h.split(',')
                .map(str::trim)
                .find(|p| p.starts_with(WS_BEARER_PREFIX))
                .map(str::to_string)
        });

        // The header wins when present, so native clients keep their existing
        // path and a malformed subprotocol can never downgrade a valid header.
        if auth_header.is_some() || offered.is_none() {
            return WsAuthorization {
                decision: self.authorize(auth_header),
                selected_protocol: offered,
            };
        }

        let expected = match &self.token {
            None => {
                return WsAuthorization {
                    decision: AccessDecision::Allowed,
                    selected_protocol: offered,
                }
            }
            Some(t) => t,
        };

        let decision = match offered.as_deref().and_then(decode_ws_bearer) {
            None => AccessDecision::Denied("Malformed cid.bearer WebSocket subprotocol"),
            Some(p) if constant_time_eq(p.as_bytes(), expected.as_bytes()) => {
                AccessDecision::Allowed
            }
            Some(_) => AccessDecision::Denied("Invalid access token"),
        };

        WsAuthorization {
            decision,
            selected_protocol: offered,
        }
    }

    pub fn origin_allowed(&self, origin: &str) -> bool {
        self.allowed_origins.iter().any(|o| o == origin || o == "*")
    }
}

/// Subprotocol prefix carrying a bearer token through a WebSocket handshake.
pub const WS_BEARER_PREFIX: &str = "cid.bearer.";

/// Outcome of a WebSocket handshake check.
pub struct WsAuthorization {
    pub decision: AccessDecision,
    /// The subprotocol to echo in the response. RFC 6455 requires the server to
    /// select from what the client offered; omitting it leaves `ws.protocol`
    /// empty on the client, which some clients treat as a failed negotiation.
    pub selected_protocol: Option<String>,
}

/// Build the subprotocol a client offers for `token`. Shared with the tests and
/// any non-browser client that prefers the subprotocol path.
pub fn ws_bearer_protocol(token: &str) -> String {
    use base64::Engine;
    format!(
        "{WS_BEARER_PREFIX}{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(token)
    )
}

fn decode_ws_bearer(protocol: &str) -> Option<String> {
    use base64::Engine;
    let encoded = protocol.strip_prefix(WS_BEARER_PREFIX)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .ok()?;
    String::from_utf8(bytes).ok()
}

#[derive(Debug, PartialEq, Eq)]
pub enum AccessDecision {
    Allowed,
    Denied(&'static str),
}

/// Compare without an early return, so the time taken does not depend on how
/// many leading bytes matched.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Generate a token suitable for `--auth-token`.
pub fn generate_token() -> String {
    use rand::Rng;
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    (0..40)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn loopback_without_a_token_is_allowed() {
        let policy = AccessPolicy::new(IpAddr::V4(Ipv4Addr::LOCALHOST), None, vec![]).unwrap();
        assert!(!policy.requires_auth());
        assert_eq!(policy.authorize(None), AccessDecision::Allowed);
    }

    #[test]
    fn remote_bind_without_a_token_is_refused_at_startup() {
        let err = AccessPolicy::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), None, vec![]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("--auth-token"), "{msg}");
        assert!(msg.contains("Refusing"), "{msg}");
    }

    #[test]
    fn remote_bind_with_a_token_requires_that_token() {
        let policy = AccessPolicy::new(
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            Some("s3cret-token".into()),
            vec![],
        )
        .unwrap();
        assert!(policy.requires_auth());
        assert_eq!(
            policy.authorize(None),
            AccessDecision::Denied("Missing Authorization: Bearer <token> header")
        );
        assert_eq!(
            policy.authorize(Some("Bearer wrong")),
            AccessDecision::Denied("Invalid access token")
        );
        assert_eq!(
            policy.authorize(Some("Bearer s3cret-token")),
            AccessDecision::Allowed
        );
    }

    fn token_policy(token: &str) -> AccessPolicy {
        AccessPolicy::new(
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            Some(token.into()),
            vec![],
        )
        .unwrap()
    }

    #[test]
    fn websocket_accepts_the_token_as_a_subprotocol() {
        let policy = token_policy("s3cret-token");
        let offered = ws_bearer_protocol("s3cret-token");

        let authz = policy.authorize_websocket(None, Some(&offered));
        assert_eq!(authz.decision, AccessDecision::Allowed);
        // Must be echoed verbatim or the client fails the negotiation.
        assert_eq!(authz.selected_protocol.as_deref(), Some(offered.as_str()));
    }

    #[test]
    fn websocket_rejects_a_wrong_or_malformed_subprotocol() {
        let policy = token_policy("s3cret-token");

        assert_eq!(
            policy
                .authorize_websocket(None, Some(&ws_bearer_protocol("wrong")))
                .decision,
            AccessDecision::Denied("Invalid access token")
        );
        assert_eq!(
            policy
                .authorize_websocket(None, Some("cid.bearer.!!!not-base64!!!"))
                .decision,
            AccessDecision::Denied("Malformed cid.bearer WebSocket subprotocol")
        );
        // No header and no subprotocol is the browser's pre-token state.
        assert_eq!(
            policy.authorize_websocket(None, None).decision,
            AccessDecision::Denied("Missing Authorization: Bearer <token> header")
        );
    }

    #[test]
    fn websocket_picks_the_bearer_entry_out_of_a_multi_protocol_offer() {
        let policy = token_policy("s3cret-token");
        let offered = format!("chat, {}, superchat", ws_bearer_protocol("s3cret-token"));

        let authz = policy.authorize_websocket(None, Some(&offered));
        assert_eq!(authz.decision, AccessDecision::Allowed);
        assert_eq!(
            authz.selected_protocol.as_deref(),
            Some(ws_bearer_protocol("s3cret-token").as_str())
        );
    }

    #[test]
    fn websocket_header_still_wins_over_a_bad_subprotocol() {
        let policy = token_policy("s3cret-token");
        let authz =
            policy.authorize_websocket(Some("Bearer s3cret-token"), Some("cid.bearer.garbage"));
        assert_eq!(authz.decision, AccessDecision::Allowed);
    }

    #[test]
    fn websocket_without_a_configured_token_stays_open() {
        let policy = AccessPolicy::new(IpAddr::V4(Ipv4Addr::LOCALHOST), None, vec![]).unwrap();
        assert_eq!(
            policy.authorize_websocket(None, None).decision,
            AccessDecision::Allowed
        );
        // Still echoed, so a client that always offers one negotiates cleanly.
        let authz = policy.authorize_websocket(None, Some(&ws_bearer_protocol("anything")));
        assert_eq!(authz.decision, AccessDecision::Allowed);
        assert!(authz.selected_protocol.is_some());
    }

    #[test]
    fn ws_bearer_round_trips_tokens_a_subprotocol_cannot_carry_literally() {
        // A token with a space or comma would break the subprotocol grammar if
        // it were not encoded — this is why the value is base64url.
        let raw = "tok en,with/odd+chars";
        let policy = token_policy(raw);
        assert_eq!(
            policy
                .authorize_websocket(None, Some(&ws_bearer_protocol(raw)))
                .decision,
            AccessDecision::Allowed
        );
    }

    #[test]
    fn bearer_prefix_is_case_tolerant() {
        let policy = AccessPolicy::new(
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            Some("tok".into()),
            vec![],
        )
        .unwrap();
        assert_eq!(
            policy.authorize(Some("bearer tok")),
            AccessDecision::Allowed
        );
    }

    #[test]
    fn a_header_without_the_bearer_scheme_is_denied() {
        let policy = AccessPolicy::new(
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            Some("tok".into()),
            vec![],
        )
        .unwrap();
        assert!(matches!(
            policy.authorize(Some("tok")),
            AccessDecision::Denied(_)
        ));
    }

    #[test]
    fn a_blank_token_counts_as_no_token() {
        let err = AccessPolicy::new(
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            Some("   ".into()),
            vec![],
        )
        .unwrap_err();
        assert!(err.to_string().contains("--auth-token"));
    }

    #[test]
    fn default_origins_cover_the_local_shells() {
        let policy = AccessPolicy::local_only();
        assert!(policy.origin_allowed("http://localhost:1420"));
        assert!(policy.origin_allowed("tauri://localhost"));
        assert!(!policy.origin_allowed("https://evil.example.com"));
    }

    #[test]
    fn explicit_origins_replace_the_defaults() {
        let policy = AccessPolicy::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            None,
            vec!["https://cid.internal".into()],
        )
        .unwrap();
        assert!(policy.origin_allowed("https://cid.internal"));
        assert!(!policy.origin_allowed("http://localhost:1420"));
    }

    #[test]
    fn constant_time_eq_matches_ordinary_equality() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn generated_tokens_are_long_and_distinct() {
        let a = generate_token();
        let b = generate_token();
        assert_eq!(a.len(), 40);
        assert_ne!(a, b);
    }
}
