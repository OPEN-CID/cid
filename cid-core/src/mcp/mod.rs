use crate::api::types::{McpServer, McpServerStatus, McpTool, McpTransportType};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::{mpsc, oneshot, Mutex, RwLock},
};

// MCP client targeting the 2026-07-28 spec shape (stateless core, Tasks
// extension, MCP Apps). Both transports the spec still supports — stdio and
// Streamable HTTP — speak real JSON-RPC 2.0 to the server; neither
// fabricates a response.

const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// A duplex JSON-RPC 2.0 connection to a stdio MCP server: a writer task
/// owns the child's stdin, a reader task owns its stdout and routes each
/// response to the pending request with a matching `id`. This replaces a
/// version that spawned the child and returned immediately without ever
/// reading from or writing to it — every "call" against it fabricated a
/// success response instead of talking to the process at all
/// (review_prompt.md §2.1).
struct StdioTransport {
    child: tokio::process::Child,
    write_tx: mpsc::UnboundedSender<String>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<serde_json::Value>>>>,
    next_id: AtomicU64,
}

impl StdioTransport {
    fn spawn(mut child: tokio::process::Child) -> Result<Self> {
        let stdin = child
            .stdin
            .take()
            .context("stdio MCP server has no stdin")?;
        let stdout = child
            .stdout
            .take()
            .context("stdio MCP server has no stdout")?;
        let stderr = child.stderr.take();

        let pending: Arc<Mutex<HashMap<String, oneshot::Sender<serde_json::Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let (write_tx, mut write_rx) = mpsc::unbounded_channel::<String>();
        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(line) = write_rx.recv().await {
                if stdin.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
                if stdin.write_all(b"\n").await.is_err() {
                    break;
                }
                if stdin.flush().await.is_err() {
                    break;
                }
            }
        });

        let pending_reader = pending.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                            tracing::debug!("MCP stdio server sent a non-JSON line: {line}");
                            continue;
                        };
                        // A notification (no `id`) has nowhere to be routed —
                        // MCP servers can send these unprompted; nothing in
                        // this client's request/response model consumes them
                        // yet, so they're logged and dropped rather than
                        // silently ignored without a trace.
                        let Some(id) = json_rpc_id_as_string(&value) else {
                            tracing::debug!("MCP stdio server notification: {value}");
                            continue;
                        };
                        if let Some(tx) = pending_reader.lock().await.remove(&id) {
                            let _ = tx.send(value);
                        }
                    }
                    Ok(None) => break, // stdout closed — server exited
                    Err(e) => {
                        tracing::warn!("MCP stdio server stdout read error: {e}");
                        break;
                    }
                }
            }
            // The read loop only exits when the server's stdout closed or
            // errored — nothing will ever answer a still-pending request at
            // that point. Fail them all immediately rather than leaving each
            // one to hang for the full REQUEST_TIMEOUT: a server that isn't
            // speaking MCP at all (e.g. a plain, non-JSON-RPC executable that
            // exits right away) would otherwise make every `add_server` call
            // against it take the full timeout to fail.
            let mut pending = pending_reader.lock().await;
            for (_, tx) in pending.drain() {
                let _ = tx.send(serde_json::json!({
                    "jsonrpc": "2.0",
                    "error": { "code": -32000, "message": "stdio MCP server exited before responding" }
                }));
            }
        });

        if let Some(stderr) = stderr {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!("MCP stdio server stderr: {line}");
                }
            });
        }

        Ok(Self {
            child,
            write_tx,
            pending,
            next_id: AtomicU64::new(1),
        })
    }

    /// Send a JSON-RPC request and await its correlated response, or time
    /// out. Returns the `result` field, or an error built from the
    /// response's `error` field.
    async fn request(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst).to_string();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id.clone(), tx);

        let envelope = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let line = serde_json::to_string(&envelope)?;
        if self.write_tx.send(line).is_err() {
            self.pending.lock().await.remove(&id);
            bail!("stdio MCP server's write task is no longer running");
        }

        let response = match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(v)) => v,
            Ok(Err(_)) => {
                self.pending.lock().await.remove(&id);
                bail!("stdio MCP server closed the connection before responding to {method}");
            }
            Err(_) => {
                self.pending.lock().await.remove(&id);
                bail!("stdio MCP server did not respond to {method} within {REQUEST_TIMEOUT:?}");
            }
        };

        if let Some(error) = response.get("error") {
            bail!("MCP server returned an error for {method}: {error}");
        }
        Ok(response
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null))
    }

    /// Send a JSON-RPC notification (no `id`, no response expected) — used
    /// for `notifications/initialized`, the handshake's final step.
    fn notify(&self, method: &str, params: serde_json::Value) -> Result<()> {
        let envelope = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        let line = serde_json::to_string(&envelope)?;
        self.write_tx
            .send(line)
            .map_err(|_| anyhow::anyhow!("stdio MCP server's write task is no longer running"))
    }
}

fn json_rpc_id_as_string(value: &serde_json::Value) -> Option<String> {
    match value.get("id") {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(serde_json::Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

struct ServerConnection {
    server: McpServer,
    tools: Vec<McpTool>,
    stdio: Option<StdioTransport>,
}

pub struct McpManager {
    servers: Arc<RwLock<HashMap<String, ServerConnection>>>,
    http_client: reqwest::Client,
}

impl Default for McpManager {
    fn default() -> Self {
        Self::new()
    }
}

impl McpManager {
    pub fn new() -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
        }
    }

    pub async fn list_servers(&self) -> Vec<McpServer> {
        let guard = self.servers.read().await;
        guard.values().map(|c| c.server.clone()).collect()
    }

    pub async fn add_server(
        &self,
        name: &str,
        transport_type: McpTransportType,
        config: serde_json::Value,
    ) -> Result<McpServer> {
        let id = uuid::Uuid::new_v4().to_string();
        let server = McpServer {
            id: id.clone(),
            name: name.to_string(),
            transport_type: transport_type.clone(),
            transport_config: config.clone(),
            status: McpServerStatus::Disconnected,
            enabled_for_repos: vec![],
            created_at: Utc::now(),
        };

        let mut conn = ServerConnection {
            server: server.clone(),
            tools: vec![],
            stdio: None,
        };

        match transport_type {
            McpTransportType::Stdio => match self.connect_stdio(&config).await {
                Ok((transport, tools)) => {
                    conn.stdio = Some(transport);
                    conn.tools = tools;
                    conn.server.status = McpServerStatus::Connected;
                }
                Err(e) => {
                    tracing::warn!("Failed to connect stdio MCP server {}: {}", name, e);
                    conn.server.status = McpServerStatus::Error;
                }
            },
            McpTransportType::Http => match self.connect_http(&config).await {
                Ok(tools) => {
                    conn.tools = tools;
                    conn.server.status = McpServerStatus::Connected;
                }
                Err(e) => {
                    tracing::warn!("Failed to connect http MCP server {}: {}", name, e);
                    conn.server.status = McpServerStatus::Error;
                }
            },
        }

        let mut guard = self.servers.write().await;
        guard.insert(id.clone(), conn);
        let inserted = guard.get(&id).unwrap().server.clone();
        Ok(inserted)
    }

    /// Spawn the server, perform the real MCP handshake (`initialize` →
    /// `notifications/initialized` → `tools/list`), and return the live
    /// transport plus whatever tools it reported. Every step here is a real
    /// JSON-RPC round trip over the child's stdio, not a delay-then-guess.
    async fn connect_stdio(
        &self,
        config: &serde_json::Value,
    ) -> Result<(StdioTransport, Vec<McpTool>)> {
        let command = config
            .get("command")
            .and_then(|v| v.as_str())
            .context("Missing command in stdio config")?;
        let args: Vec<String> = config
            .get("args")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let env: HashMap<String, String> = config
            .get("env")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let child = tokio::process::Command::new(command)
            .args(&args)
            .envs(&env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to spawn stdio MCP server")?;

        let transport = StdioTransport::spawn(child)?;

        transport
            .request(
                "initialize",
                serde_json::json!({
                    "protocolVersion": "2026-07-28",
                    "capabilities": {},
                    "clientInfo": { "name": "cid-core", "version": env!("CARGO_PKG_VERSION") },
                }),
            )
            .await
            .context("MCP initialize handshake failed")?;

        // Notification, not a request — the spec doesn't define a response
        // to it, so this doesn't wait for one.
        transport.notify("notifications/initialized", serde_json::json!({}))?;

        let tools = match transport.request("tools/list", serde_json::json!({})).await {
            Ok(result) => result
                .get("tools")
                .cloned()
                .and_then(|t| serde_json::from_value::<Vec<McpTool>>(t).ok())
                .unwrap_or_default(),
            Err(e) => {
                tracing::warn!("MCP stdio server tools/list failed: {e}");
                vec![]
            }
        };

        Ok((transport, tools))
    }

    async fn connect_http(&self, config: &serde_json::Value) -> Result<Vec<McpTool>> {
        let url = config
            .get("url")
            .and_then(|v| v.as_str())
            .or_else(|| config.get("endpoint").and_then(|v| v.as_str()))
            .context("Missing url in http config")?;

        // Try to fetch tools via MCP HTTP endpoint: POST with JSON-RPC tools/list
        let req_body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
            "params": {}
        });

        match self.http_client.post(url).json(&req_body).send().await {
            Ok(resp) => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    if let Some(tools_val) = json.get("result").and_then(|r| r.get("tools")) {
                        if let Ok(tools) = serde_json::from_value::<Vec<McpTool>>(tools_val.clone())
                        {
                            return Ok(tools);
                        }
                    }
                }
                Ok(vec![])
            }
            Err(e) => {
                tracing::warn!("HTTP MCP connect failed, returning empty tools: {}", e);
                Ok(vec![])
            }
        }
    }

    pub async fn remove_server(&self, id: &str) -> Result<()> {
        let mut guard = self.servers.write().await;
        if let Some(mut conn) = guard.remove(id) {
            if let Some(mut transport) = conn.stdio.take() {
                let _ = transport.child.kill().await;
            }
        }
        Ok(())
    }

    pub async fn list_tools(&self, server_id: &str) -> Result<Vec<McpTool>> {
        let guard = self.servers.read().await;
        if let Some(conn) = guard.get(server_id) {
            // If empty and http transport, try to re-fetch
            if conn.tools.is_empty() && conn.server.transport_type == McpTransportType::Http {
                let config = conn.server.transport_config.clone();
                drop(guard);
                if let Ok(tools) = self.connect_http(&config).await {
                    let mut write_guard = self.servers.write().await;
                    if let Some(c) = write_guard.get_mut(server_id) {
                        c.tools = tools.clone();
                    }
                    return Ok(tools);
                }
                return Ok(vec![]);
            }
            Ok(conn.tools.clone())
        } else {
            anyhow::bail!("MCP server not found: {}", server_id)
        }
    }

    pub async fn call_tool(
        &self,
        server_id: &str,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let guard = self.servers.read().await;
        let conn = guard.get(server_id).context("Server not found")?;

        match conn.server.transport_type {
            McpTransportType::Http => {
                let url = conn
                    .server
                    .transport_config
                    .get("url")
                    .and_then(|v| v.as_str())
                    .context("Missing url")?;
                let req_body = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": uuid::Uuid::new_v4().to_string(),
                    "method": "tools/call",
                    "params": {
                        "name": tool_name,
                        "arguments": arguments
                    }
                });

                let resp = self.http_client.post(url).json(&req_body).send().await?;
                let json: serde_json::Value = resp.json().await?;
                Ok(json)
            }
            McpTransportType::Stdio => {
                let transport = conn
                    .stdio
                    .as_ref()
                    .context("stdio MCP server is not connected")?;
                transport
                    .request(
                        "tools/call",
                        serde_json::json!({ "name": tool_name, "arguments": arguments }),
                    )
                    .await
            }
        }
    }

    // Load persisted servers on startup
    pub async fn load_persisted(&self, servers: Vec<McpServer>) {
        for srv in servers {
            let id = srv.id.clone();
            let conn = ServerConnection {
                server: srv,
                tools: vec![],
                stdio: None,
            };
            self.servers.write().await.insert(id, conn);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn test_mcp_manager_new() {
        let mgr = McpManager::new();
        let servers = mgr.list_servers().await;
        assert_eq!(servers.len(), 0);
    }
}

/// Regression tests for review_prompt.md §2.1: stdio MCP tool calls were
/// fabricated — the transport spawned the child and returned a synthetic
/// `{"simulated": true}` success without ever reading from or writing to it.
/// These tests run a real Node.js script over real stdio pipes (Node is
/// already a hard dependency of this whole repo, unlike Python which isn't
/// guaranteed present — see the fixture below) and assert on its genuine
/// responses, not a canned string.
#[cfg(test)]
mod stdio_transport_tests {
    use super::*;
    use std::io::Write;

    /// Writes a tiny Node.js script that speaks real MCP-shaped JSON-RPC
    /// over stdin/stdout: `initialize`, `notifications/initialized` (no
    /// reply), `tools/list` (one `echo` tool), and `tools/call` (echoes its
    /// arguments back). Returns the script path; the caller's `TempDir`
    /// guard must outlive the test.
    fn write_mock_server_script(dir: &std::path::Path) -> std::path::PathBuf {
        let script = r#"
const readline = require('readline');
const rl = readline.createInterface({ input: process.stdin, terminal: false });
rl.on('line', (line) => {
  let req;
  try { req = JSON.parse(line); } catch (e) { return; }
  const reply = (result) => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: req.id, result }) + '\n');
  if (req.method === 'initialize') {
    reply({ protocolVersion: '2026-07-28', capabilities: {}, serverInfo: { name: 'mock', version: '1.0' } });
  } else if (req.method === 'notifications/initialized') {
    // notification: no response
  } else if (req.method === 'tools/list') {
    reply({ tools: [{ name: 'echo', description: 'Echoes its arguments', inputSchema: { type: 'object' } }] });
  } else if (req.method === 'tools/call') {
    const args = req.params && req.params.arguments;
    reply({ content: [{ type: 'text', text: 'echo: ' + JSON.stringify(args) }] });
  } else if (req.method === 'tools/error_probe') {
    process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: req.id, error: { code: -32000, message: 'probe error' } }) + '\n');
  }
});
"#;
        let path = dir.join("mock_mcp_server.js");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(script.as_bytes()).unwrap();
        path
    }

    fn node_available() -> bool {
        std::process::Command::new("node")
            .arg("--version")
            .output()
            .is_ok()
    }

    #[tokio::test]
    async fn stdio_server_connect_performs_a_real_handshake_and_lists_real_tools() {
        if !node_available() {
            eprintln!("skipping: node not available in this environment");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let script = write_mock_server_script(dir.path());

        let manager = McpManager::new();
        let server = manager
            .add_server(
                "mock-stdio",
                McpTransportType::Stdio,
                serde_json::json!({ "command": "node", "args": [script.to_string_lossy()] }),
            )
            .await
            .unwrap();

        assert_eq!(server.status, McpServerStatus::Connected);

        let tools = manager.list_tools(&server.id).await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "echo");

        manager.remove_server(&server.id).await.unwrap();
    }

    #[tokio::test]
    async fn stdio_tool_call_returns_the_real_servers_response_not_a_simulated_one() {
        if !node_available() {
            eprintln!("skipping: node not available in this environment");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let script = write_mock_server_script(dir.path());

        let manager = McpManager::new();
        let server = manager
            .add_server(
                "mock-stdio-2",
                McpTransportType::Stdio,
                serde_json::json!({ "command": "node", "args": [script.to_string_lossy()] }),
            )
            .await
            .unwrap();

        let result = manager
            .call_tool(
                &server.id,
                "echo",
                serde_json::json!({ "message": "hello" }),
            )
            .await
            .unwrap();

        assert!(
            result.get("simulated").is_none(),
            "the response must not carry the old fabricated-success marker: {result}"
        );
        let text = result["content"][0]["text"].as_str().unwrap_or_default();
        assert!(
            text.contains("hello"),
            "expected the mock server's real echoed argument in the response, got: {result}"
        );

        manager.remove_server(&server.id).await.unwrap();
    }

    #[tokio::test]
    async fn a_stdio_server_error_response_surfaces_as_a_real_error() {
        if !node_available() {
            eprintln!("skipping: node not available in this environment");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let script = write_mock_server_script(dir.path());

        let manager = McpManager::new();
        let server = manager
            .add_server(
                "mock-stdio-3",
                McpTransportType::Stdio,
                serde_json::json!({ "command": "node", "args": [script.to_string_lossy()] }),
            )
            .await
            .unwrap();

        let transport_conn = manager.servers.read().await;
        let conn = transport_conn.get(&server.id).unwrap();
        let transport = conn.stdio.as_ref().unwrap();
        let result = transport
            .request("tools/error_probe", serde_json::json!({}))
            .await;
        drop(transport_conn);

        assert!(
            result.is_err(),
            "a JSON-RPC error response must surface as an Err, not a fabricated Ok"
        );

        manager.remove_server(&server.id).await.unwrap();
    }

    #[tokio::test]
    async fn a_process_that_exits_without_speaking_mcp_fails_fast_not_after_the_full_timeout() {
        // A real regression this fix introduced and then had to close: the
        // first version left a still-pending request hanging for the entire
        // REQUEST_TIMEOUT (30s) when the child exited without ever
        // responding — e.g. a plain, non-MCP executable like `echo`, exactly
        // what one E2E fixture (`tests/e2e/health-check.spec.ts`) actually
        // configures. Found by checking that fixture still behaves
        // reasonably against the real implementation, not just the new
        // stdio_transport_tests written alongside it.
        let manager = McpManager::new();
        let started = std::time::Instant::now();
        let server = manager
            .add_server(
                "echo-is-not-mcp",
                McpTransportType::Stdio,
                serde_json::json!({ "command": "echo", "args": ["{}"] }),
            )
            .await
            .unwrap();
        let elapsed = started.elapsed();

        assert_eq!(server.status, McpServerStatus::Error);
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "a dead/non-MCP process must fail fast, not hang for the full request timeout: took {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn connecting_to_a_nonexistent_command_reports_error_status_not_connected() {
        let manager = McpManager::new();
        let server = manager
            .add_server(
                "bad-command",
                McpTransportType::Stdio,
                serde_json::json!({ "command": "this-binary-does-not-exist-anywhere-12345" }),
            )
            .await
            .unwrap();

        assert_eq!(server.status, McpServerStatus::Error);
    }
}
