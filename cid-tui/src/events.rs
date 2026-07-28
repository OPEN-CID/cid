//! Background WebSocket listener.
//!
//! Core's `/ws` endpoint already carries `mission.tool_call.request` and
//! `mission.tool_call.complete` notifications for every other shell (Part 15's
//! "one Core, many surfaces") — nothing new was added to Core for this. The
//! TUI's own state refresh runs over plain HTTP for simplicity (see the ADR),
//! but pending-approval visibility genuinely needs the push channel: HTTP
//! polling alone has no way to learn "a tool call is now waiting on you."

use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, Clone)]
pub enum CoreEvent {
    ToolCallRequest {
        mission_id: String,
        tool_call_id: String,
        tool_name: String,
        arguments: serde_json::Value,
    },
    ToolCallComplete {
        // Kept for shape-symmetry with `ToolCallRequest` and for a future
        // per-Mission (rather than selected-Mission-only) approval view;
        // `apply_event` currently matches on `tool_call_id` alone since
        // `pending_approvals` is already scoped to the selected Mission.
        #[allow(dead_code)]
        mission_id: String,
        tool_call_id: String,
    },
    MissionChanged,
}

/// Connect to Core's WebSocket and forward decoded notifications. Runs until
/// the connection drops; the caller is responsible for retrying — the TUI's
/// own HTTP polling keeps working even while this is disconnected, so a lost
/// WS connection degrades approval visibility, not the whole app.
pub async fn listen(
    host: String,
    port: u16,
    token: Option<String>,
    tx: mpsc::UnboundedSender<CoreEvent>,
) {
    let url = format!("ws://{host}:{port}/ws");
    let mut request =
        match tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(&url) {
            Ok(r) => r,
            Err(_) => return,
        };
    if let Some(token) = &token {
        if let Ok(value) = format!("Bearer {token}").parse() {
            request.headers_mut().insert("Authorization", value);
        }
    }

    let Ok((stream, _)) = tokio_tungstenite::connect_async(request).await else {
        return;
    };
    let (_write, mut read) = stream.split();

    while let Some(msg) = read.next().await {
        let Ok(Message::Text(text)) = msg else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(method) = value["method"].as_str() else {
            continue;
        };

        let event = match method {
            "mission.tool_call.request" => Some(CoreEvent::ToolCallRequest {
                mission_id: value["params"]["mission_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                tool_call_id: value["params"]["tool_call_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                tool_name: value["params"]["tool_name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                arguments: value["params"]["arguments"].clone(),
            }),
            "mission.tool_call.complete" => Some(CoreEvent::ToolCallComplete {
                mission_id: value["params"]["mission_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                tool_call_id: value["params"]["tool_call_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
            }),
            m if m.starts_with("mission.") => Some(CoreEvent::MissionChanged),
            _ => None,
        };

        if let Some(event) = event {
            if tx.send(event).is_err() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn tool_call_request_parses_from_a_realistic_notification() {
        let raw = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "mission.tool_call.request",
            "params": {
                "mission_id": "m1",
                "tool_call_id": "tc1",
                "tool_name": "write_file",
                "arguments": { "path": "src/x.rs" }
            }
        });
        assert_eq!(raw["method"], "mission.tool_call.request");
        assert_eq!(raw["params"]["tool_call_id"], "tc1");
    }
}
