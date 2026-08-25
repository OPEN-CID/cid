//! Fuzz tests for the JSON-RPC, MCP, and ACP protocol boundaries.
//!
//! Part 21's Phase 3+ bar calls for fuzzing the protocol boundaries specifically,
//! because they are the surfaces that accept untrusted input: an MCP server's
//! responses, an ACP editor's output, and whatever a client puts on the wire.
//!
//! The property under test throughout is the same: **Core answers, and does not
//! panic.** A malformed request may legitimately produce an error response; it
//! may never take the process down or hang.

use cid_core::Core;
use proptest::prelude::*;
use serde_json::{json, Value};
use std::net::SocketAddr;

async fn start_core() -> String {
    let core = Core::new_in_memory().expect("core");
    let app = cid_core::api::router::create_router(core.app_state());
    let listener = tokio::net::TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().unwrap())
        .await
        .expect("bind");
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _core = core;
        let _ = axum::serve(listener, app.into_make_service()).await;
    });
    let base = format!("http://{addr}");
    for _ in 0..50 {
        if reqwest::get(format!("{base}/health")).await.is_ok() {
            return base;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("core did not start");
}

/// POST a raw body and return the HTTP status plus parsed body, if any.
async fn post_raw(base: &str, body: String) -> (u16, Option<Value>) {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/api/rpc"))
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await
        .expect("core must answer, not drop the connection");
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    (status, serde_json::from_str(&text).ok())
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().unwrap()
}

// ---------------------------------------------------------------------------
// Malformed envelopes
// ---------------------------------------------------------------------------

#[test]
fn malformed_json_never_panics_the_server() {
    let rt = rt();
    rt.block_on(async {
        let base = start_core().await;
        let bodies = [
            "".to_string(),
            "{".to_string(),
            "null".to_string(),
            "[]".to_string(),
            "\"just a string\"".to_string(),
            "{\"jsonrpc\":}".to_string(),
            format!("{{\"a\":\"{}\"}}", "x".repeat(100_000)),
            "{\"jsonrpc\":\"2.0\",\"id\":1}".to_string(),
            "\u{0}\u{1}\u{2}".to_string(),
        ];
        for body in bodies {
            let (status, _) = post_raw(&base, body.clone()).await;
            assert!(
                status < 500,
                "body {:?} produced a server error ({status}) — the boundary must reject, not fail",
                &body[..body.len().min(40)]
            );
        }
        // Still alive and answering after all of that.
        let health: Value = reqwest::get(format!("{base}/health"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(health["status"], "ok");
    });
}

#[test]
fn deeply_nested_json_is_rejected_rather_than_exhausting_the_stack() {
    let rt = rt();
    rt.block_on(async {
        let base = start_core().await;
        let depth = 5_000;
        let nested = format!("{}{}{}", "[".repeat(depth), "1", "]".repeat(depth));
        let body = json!({
            "jsonrpc": "2.0", "id": "1", "method": "workspace.list", "params": {}
        })
        .to_string()
        .replace("{}", &nested);

        let (status, _) = post_raw(&base, body).await;
        assert!(status < 500, "deep nesting must not crash the parser");

        let health = reqwest::get(format!("{base}/health")).await;
        assert!(health.is_ok(), "core must survive a deeply nested payload");
    });
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(40))]

    /// Any method name at all — including control characters, path traversal
    /// shapes, and enormous strings — must produce an orderly error.
    #[test]
    fn arbitrary_method_names_yield_an_error_not_a_crash(method in ".{0,200}") {
        let rt = rt();
        rt.block_on(async {
            let base = start_core().await;
            let body = json!({
                "jsonrpc": "2.0", "id": "1", "method": method, "params": {}
            })
            .to_string();
            let (status, parsed) = post_raw(&base, body).await;
            prop_assert!(status < 500, "method {method:?} produced {status}");
            if let Some(v) = parsed {
                prop_assert!(
                    v.get("result").is_some() || v.get("error").is_some(),
                    "response must be a valid JSON-RPC envelope: {v}"
                );
            }
            Ok(())
        }).unwrap();
    }

    /// Params of arbitrary shape against a real method must not panic the
    /// handler — deserialization failures are errors, not crashes.
    #[test]
    fn arbitrary_params_against_real_methods_are_handled(
        method in prop::sample::select(vec![
            "repo.connect", "session.create", "session.get", "file.read",
            "git.status", "mcp.tool.call", "acp.handoff", "skills.resolve",
            "auth.login", "governance.policy.set", "forge.connect", "tracker.link",
        ]),
        s in ".{0,60}",
        n in any::<i64>(),
        b in any::<bool>(),
    ) {
        let rt = rt();
        rt.block_on(async {
            let base = start_core().await;
            let shapes = vec![
                json!(null),
                json!(s),
                json!(n),
                json!(b),
                json!([s, n, b]),
                json!({ "path": s, "id": n, "flag": b }),
                json!({ "repo_path": s, "session_id": s, "content": s }),
            ];
            for params in shapes {
                let body = json!({
                    "jsonrpc": "2.0", "id": "1", "method": method, "params": params
                })
                .to_string();
                let (status, _) = post_raw(&base, body).await;
                prop_assert!(status < 500, "{method} with {params} produced {status}");
            }
            Ok(())
        }).unwrap();
    }
}

// ---------------------------------------------------------------------------
// MCP boundary — a server's responses are untrusted input
// ---------------------------------------------------------------------------

#[test]
fn mcp_server_registration_rejects_hostile_input_without_panicking() {
    let rt = rt();
    rt.block_on(async {
        let base = start_core().await;
        let hostile = [
            json!({ "name": "", "transport_type": "stdio", "config": {} }),
            json!({ "name": "x", "transport_type": "../../etc/passwd", "config": {} }),
            json!({ "name": "x", "transport_type": "http", "config": { "url": "file:///etc/passwd" } }),
            json!({ "name": "x", "transport_type": "http", "config": { "url": "not a url" } }),
            json!({ "name": "\u{0}\u{1}", "transport_type": "stdio", "config": { "command": "rm -rf /" } }),
            json!({ "name": "x".repeat(10_000), "transport_type": "stdio", "config": {} }),
        ];
        for params in hostile {
            let body = json!({
                "jsonrpc": "2.0", "id": "1", "method": "mcp.server.add", "params": params
            })
            .to_string();
            let (status, _) = post_raw(&base, body).await;
            assert!(status < 500, "hostile MCP registration produced {status}: {params}");
        }
        assert!(reqwest::get(format!("{base}/health")).await.is_ok());
    });
}

#[test]
fn mcp_tool_calls_with_hostile_arguments_are_handled() {
    let rt = rt();
    rt.block_on(async {
        let base = start_core().await;
        let cases = [
            json!({ "server_id": "nope", "tool_name": "x", "arguments": {} }),
            json!({ "server_id": "", "tool_name": "", "arguments": null }),
            json!({ "server_id": "a", "tool_name": "../..", "arguments": [1, 2, 3] }),
            json!({ "server_id": "a", "tool_name": "x", "arguments": { "deep": { "deep": { "deep": "x" } } } }),
        ];
        for params in cases {
            let body = json!({
                "jsonrpc": "2.0", "id": "1", "method": "mcp.tool.call", "params": params
            })
            .to_string();
            let (status, _) = post_raw(&base, body).await;
            assert!(status < 500, "hostile tool call produced {status}");
        }
    });
}

// ---------------------------------------------------------------------------
// ACP boundary — an external editor's identifiers are untrusted
// ---------------------------------------------------------------------------

#[test]
fn acp_handoff_rejects_hostile_identifiers() {
    let rt = rt();
    rt.block_on(async {
        let base = start_core().await;
        let cases = [
            json!({ "session_id": "", "editor_id": "" }),
            json!({ "session_id": "../../etc", "editor_id": "zed" }),
            json!({ "session_id": "m", "editor_id": "; rm -rf /" }),
            json!({ "session_id": "m", "editor_id": "x".repeat(5_000) }),
            json!({ "session_id": null, "editor_id": 42 }),
        ];
        for params in cases {
            let body = json!({
                "jsonrpc": "2.0", "id": "1", "method": "acp.handoff", "params": params
            })
            .to_string();
            let (status, parsed) = post_raw(&base, body).await;
            assert!(status < 500, "hostile handoff produced {status}");
            if let Some(v) = parsed {
                assert!(
                    v.get("result").is_none(),
                    "a hostile handoff must not succeed: {v}"
                );
            }
        }
    });
}

#[test]
fn acp_take_back_with_arbitrary_tokens_is_safe() {
    let rt = rt();
    rt.block_on(async {
        let base = start_core().await;
        for token in ["", "../..", "\u{0}", &"x".repeat(10_000)] {
            let body = json!({
                "jsonrpc": "2.0", "id": "1", "method": "acp.take_back",
                "params": { "handoff_id": token }
            })
            .to_string();
            let (status, _) = post_raw(&base, body).await;
            assert!(status < 500);
        }
    });
}

// ---------------------------------------------------------------------------
// Path-shaped inputs — the highest-risk untrusted surface
// ---------------------------------------------------------------------------

#[test]
fn file_reads_outside_the_workspace_do_not_crash_or_leak_a_panic() {
    let rt = rt();
    rt.block_on(async {
        let base = start_core().await;
        let paths = [
            "/etc/passwd",
            "C:\\Windows\\System32\\config\\SAM",
            "../../../../../../etc/shadow",
            "\\\\?\\C:\\Windows\\win.ini",
            "",
            "\u{0}",
        ];
        for path in paths {
            let body = json!({
                "jsonrpc": "2.0", "id": "1", "method": "file.read", "params": { "path": path }
            })
            .to_string();
            let (status, _) = post_raw(&base, body).await;
            assert!(status < 500, "file.read({path:?}) produced {status}");
        }
        assert!(reqwest::get(format!("{base}/health")).await.is_ok());
    });
}
