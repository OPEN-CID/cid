use anyhow::Context as _;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tracing::{debug, error, info};

use crate::{
    access::{AccessDecision, AccessPolicy},
    acp::AcpHostManager,
    analyzer::CodeAnalyzer,
    api::types::{
        AutonomyAllowlistUpdateParams, AutonomyCheckParams, BackgroundModelConfig,
        BackgroundTaskSubmitParams, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse,
        LocalRuntimeDetectParams, McpTaskCreateParams, SandboxTestResult, SemanticDependencyParams,
        SemanticEnableParams, SemanticGitBlameParams, SemanticSearchParams, SlackConfig,
        SlackTriggerParams, SubagentCancelParams, SubagentListParams, SubagentSpawnParams,
        TeamsConfig, TeamsTriggerParams,
    },
    autonomy::AutonomyManager,
    background_model::BackgroundModelRouter,
    context::ContextManager,
    context_engine::ContextEngineManager,
    git::GitManager,
    github::GitHubManager,
    local_models::LocalRuntimeDetector,
    mcp::McpManager,
    mcp_tasks::McpTasksManager,
    model::ModelManager,
    persistence::Persistence,
    pty::PtyManager,
    sandbox::SandboxManager,
    semantic_engine::SemanticEngine,
    skills::SkillsManager,
    slack_bridge::SlackBridge,
    subagent::SubagentOrchestrator,
    teams_bridge::TeamsBridge,
};

#[derive(Clone)]
pub struct AppState {
    pub persistence: Arc<Persistence>,
    pub git_manager: Arc<GitManager>,
    pub pty_manager: Arc<PtyManager>,
    pub mcp_manager: Arc<McpManager>,
    pub model_manager: Arc<ModelManager>,
    pub context_manager: Arc<ContextManager>,
    pub context_engine_manager: Arc<ContextEngineManager>,
    pub acp_manager: Arc<AcpHostManager>,
    pub github_manager: Arc<GitHubManager>,
    pub analyzer: Arc<CodeAnalyzer>,
    pub skills_manager: Arc<SkillsManager>,
    pub role_runner: Arc<crate::roles::RoleRunner>,
    pub auth_manager: Arc<crate::auth::AuthManager>,
    pub governance_manager: Arc<crate::governance::GovernanceManager>,
    pub forge_manager: Arc<crate::forges::ForgeManager>,
    pub tracker_manager: Arc<crate::trackers::TrackerManager>,
    pub confidence_engine: Arc<crate::confidence::ConfidenceEngine>,
    pub role_profile_manager: Arc<crate::role_profiles::RoleProfileManager>,
    pub deployment_log: Arc<crate::decisions::DeploymentLog>,
    pub autonomy_manager: Arc<AutonomyManager>,
    pub background_model_router: Arc<BackgroundModelRouter>,
    pub subagent_orchestrator: Arc<SubagentOrchestrator>,
    pub slack_bridge: Arc<SlackBridge>,
    pub teams_bridge: Arc<TeamsBridge>,
    pub mcp_tasks_manager: Arc<McpTasksManager>,
    pub semantic_engine: Arc<SemanticEngine>,
    pub sandbox_manager: Arc<SandboxManager>,
    pub access_policy: Arc<AccessPolicy>,
    /// Live count of connected WebSocket clients, surfaced on /health.
    pub connected_clients: Arc<std::sync::atomic::AtomicUsize>,
    pub event_tx: broadcast::Sender<String>,
    pub metrics: Arc<crate::observability::Metrics>,
    pub crash_log: Arc<crate::observability::CrashLog>,
}

pub fn create_router(state: AppState) -> Router {
    use tower_http::cors::CorsLayer;

    // Origins are an explicit allow-list rather than `Any`: the RPC surface can
    // read files, run commands, and reach model credentials, so a page on an
    // arbitrary origin must not be able to drive it.
    let origins: Vec<axum::http::HeaderValue> = state
        .access_policy
        .allowed_origins
        .iter()
        .filter_map(|o| o.parse().ok())
        .collect();

    let cors = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
        ])
        .allow_credentials(false);

    Router::new()
        .route("/ws", get(ws_handler))
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        .route("/api/rpc", axum::routing::post(http_rpc_handler))
        .layer(cors)
        .with_state(state)
}

/// Prometheus text exposition format, unauthenticated like /health — it exposes
/// call counts and gauges, not RPC content. Consistent with running Core behind
/// a local scrape target rather than a public one.
async fn metrics_handler(State(state): State<AppState>) -> impl IntoResponse {
    state.metrics.set_gauge(
        "cid_ws_connections_current",
        state
            .connected_clients
            .load(std::sync::atomic::Ordering::Relaxed) as u64,
    );
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        state.metrics.render_prometheus(),
    )
}

/// Unauthenticated by design — it reports reachability and whether a token is
/// needed, and nothing that is not already implied by the port being open.
async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "status": "ok",
        "service": "cid-core",
        "version": env!("CARGO_PKG_VERSION"),
        "connected_clients": state.connected_clients.load(std::sync::atomic::Ordering::Relaxed),
        "auth_required": state.access_policy.requires_auth(),
        "loopback_only": state.access_policy.is_loopback_only(),
    }))
}

/// Extract the `Authorization` header and apply the access policy.
fn check_access(state: &AppState, headers: &axum::http::HeaderMap) -> Result<(), String> {
    let auth = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    match state.access_policy.authorize(auth) {
        AccessDecision::Allowed => Ok(()),
        AccessDecision::Denied(reason) => Err(reason.to_string()),
    }
}

async fn http_rpc_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    axum::Json(req): axum::Json<JsonRpcRequest>,
) -> impl IntoResponse {
    if let Err(reason) = check_access(&state, &headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            axum::Json(JsonRpcResponse::error(req.id, -32001, reason)),
        );
    }
    let resp = handle_rpc(req, &state).await;
    (axum::http::StatusCode::OK, axum::Json(resp))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // A WebSocket upgrade is authorized before the socket exists — once open it
    // carries the same authority as the HTTP surface.
    if let Err(reason) = check_access(&state, &headers) {
        return (axum::http::StatusCode::UNAUTHORIZED, reason).into_response();
    }
    ws.on_upgrade(move |socket| handle_ws(socket, state))
        .into_response()
}

/// Decrements the connected-client count however the socket task ends, so a
/// panicking or cancelled client cannot leave the count permanently inflated.
struct ClientGuard(Arc<std::sync::atomic::AtomicUsize>);

impl Drop for ClientGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Fixed WS handler with proper per-client response handling
/// - Per-client sink via Arc<Mutex<SplitSink>> for direct responses
/// - Broadcast channel only for notifications (pty.output, mission.message.delta, etc)
/// - No more broadcasting responses to all clients
async fn handle_ws(socket: WebSocket, state: AppState) {
    use std::sync::atomic::Ordering;
    let mut event_rx = state.event_tx.subscribe();
    state.connected_clients.fetch_add(1, Ordering::Relaxed);
    let _guard = ClientGuard(state.connected_clients.clone());
    info!("WebSocket client connected");

    let (sink, mut stream) = socket.split();
    let sink = Arc::new(Mutex::new(sink));

    // Task: forward broadcast notifications to this client
    let sink_clone = sink.clone();
    let forward_task = tokio::spawn(async move {
        while let Ok(event_json) = event_rx.recv().await {
            let mut guard = sink_clone.lock().await;
            if guard.send(Message::Text(event_json)).await.is_err() {
                break;
            }
        }
    });

    // Handle incoming messages
    while let Some(msg) = stream.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                match serde_json::from_str::<JsonRpcRequest>(&text) {
                    Ok(req) => {
                        let resp = handle_rpc(req, &state).await;
                        let resp_str =
                            serde_json::to_string(&resp).unwrap_or_else(|_| "{}".to_string());
                        // Send directly to this client only, not via broadcast
                        let mut guard = sink.lock().await;
                        if guard.send(Message::Text(resp_str)).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let err_resp =
                            JsonRpcResponse::error(None, -32700, format!("Parse error: {}", e));
                        let err_str = serde_json::to_string(&err_resp).unwrap();
                        let mut guard = sink.lock().await;
                        if guard.send(Message::Text(err_str)).await.is_err() {
                            break;
                        }
                    }
                }
            }
            Ok(Message::Close(_)) => break,
            Err(e) => {
                error!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }

    forward_task.abort();
    info!("WebSocket client disconnected");
}

async fn handle_rpc(req: JsonRpcRequest, state: &AppState) -> JsonRpcResponse {
    let id = req.id.clone();
    let method = req.method.clone();
    let params = req.params.clone();

    // Tracing
    info!("RPC method: {}", method);
    state.metrics.inc_counter("cid_rpc_requests_total");
    // `method` is a fixed, small set of literal strings from this match arm list,
    // never user-supplied text — safe as an unbounded-cardinality Prometheus label.
    state
        .metrics
        .inc_labeled("cid_rpc_requests_by_method_total", &method);

    let result = match method.as_str() {
        // Workspace
        "workspace.list" => handle_workspace_list(state).await,
        "workspace.get" => handle_workspace_get(params, state).await,

        // Repo channel
        "repo.connect" => handle_repo_connect(params, state).await,
        "repo.list" => handle_repo_list(state).await,
        "repo.get" => handle_repo_get(params, state).await,
        "repo.disconnect" => handle_repo_disconnect(params, state).await,
        "repo.agents_md" => handle_repo_agents_md(params, state).await,
        "repo.agents_md.write" => handle_repo_agents_md_write(params, state).await,
        "repo.agents_md.approve" => handle_repo_agents_md_approve(params, state).await,

        // Mission
        "mission.create" => handle_mission_create(params, state).await,
        "mission.list" => handle_mission_list(params, state).await,
        "mission.get" => handle_mission_get(params, state).await,
        "mission.close" => handle_mission_close(params, state).await,
        "mission.send_message" => handle_mission_send_message(params, state).await,
        "mission.approve_tool" => handle_mission_approve_tool(params, state).await,

        // Messages
        "message.list" => handle_message_list(params, state).await,

        // Git
        "git.status" => handle_git_status(params, state).await,
        "git.diff" => handle_git_diff(params, state).await,
        "git.commit" => handle_git_commit(params, state).await,
        "git.log" => handle_git_log(params, state).await,
        "git.worktree.list" => handle_worktree_list(params, state).await,
        "git.worktree.create" => handle_worktree_create(params, state).await,
        "git.worktree.remove" => handle_worktree_remove(params, state).await,
        "git.hunk.apply" => handle_git_hunk_apply(params, state).await,

        // PTY
        "pty.create" => handle_pty_create(params, state).await,
        "pty.write" => handle_pty_write(params, state).await,
        "pty.resize" => handle_pty_resize(params, state).await,
        "pty.kill" => handle_pty_kill(params, state).await,
        "pty.list" => handle_pty_list(params, state).await,

        // MCP
        "mcp.server.list" => handle_mcp_list(state).await,
        "mcp.server.add" => handle_mcp_add(params, state).await,
        "mcp.server.remove" => handle_mcp_remove(params, state).await,
        "mcp.tools.list" => handle_mcp_tools_list(params, state).await,
        "mcp.tool.call" => handle_mcp_tool_call(params, state).await,

        // File
        "file.read" => handle_file_read(params, state).await,
        "file.write" => handle_file_write(params, state).await,
        "file.list" => handle_file_list(params, state).await,
        "fs.list_dirs" => handle_fs_list_dirs(params, state).await,

        // Skills
        "skills.list" => handle_skills_list(params, state).await,
        "skills.save" => handle_skills_save(params, state).await,

        // Settings
        "settings.get" => handle_settings_get(state).await,
        "settings.update" => handle_settings_update(params, state).await,

        // Model
        "model.list" => handle_model_list(state).await,
        "model.chat" => handle_model_chat(params, state).await,

        // Local runtimes (Phase 1)
        "local.runtime.list" => handle_local_runtime_list(state).await,
        "local.runtime.detect" => handle_local_runtime_detect(params, state).await,

        // GitHub bridge (Phase 1)
        "github.connect" => handle_github_connect(params, state).await,
        "github.config.get" => handle_github_config_get(params, state).await,
        "github.issues.list" => handle_github_issues_list(params, state).await,
        "github.issue.get" => handle_github_issue_get(params, state).await,
        "github.issue.to_mission" => handle_github_issue_to_mission(params, state).await,
        "github.pr.list" => handle_github_pr_list(params, state).await,
        "github.pr.create" => handle_github_pr_create(params, state).await,
        "github.pr.status" => handle_github_pr_status(params, state).await,

        // Autonomy (Phase 1) -- allow-lists, command checks
        "autonomy.allowlist.get" => handle_autonomy_allowlist_get(params, state).await,
        "autonomy.allowlist.set" => handle_autonomy_allowlist_set(params, state).await,
        "autonomy.allowlist.remove" => handle_autonomy_allowlist_remove(params, state).await,
        "autonomy.allowlist.list" => handle_autonomy_allowlist_list(state).await,
        "autonomy.allowlist.default" => handle_autonomy_allowlist_default(params, state).await,
        "autonomy.command.check" => handle_autonomy_command_check(params, state).await,
        "autonomy.budget.check" => handle_autonomy_budget_check(params, state).await,

        // Context Engine G�� Structural (Phase 1, off by default per Repo Channel)
        "context_engine.status" => handle_context_engine_status(params, state).await,
        "context_engine.enable" => handle_context_engine_enable(params, state).await,
        "context_engine.disable" => handle_context_engine_disable(params, state).await,
        "context_engine.search" => handle_context_engine_search(params, state).await,
        "context_engine.related" => handle_context_engine_related(params, state).await,
        "context_engine.file_index" => handle_context_engine_file_index(params, state).await,
        "context_engine.recent" => handle_context_engine_recent(params, state).await,

        // Code Analysis (Phase 2)
        "code.analyze_file" => handle_code_analyze_file(params, state).await,
        "code.analyze_directory" => handle_code_analyze_directory(params, state).await,
        "code.search_symbols" => handle_code_search_symbols(params, state).await,
        "code.get_imports" => handle_code_get_imports(params, state).await,

        // Background Model Router (Phase 2)
        "background_model.status" => handle_background_model_status(params, state).await,
        "background_model.configure" => handle_background_model_configure(params, state).await,
        "background_model.submit_task" => handle_background_model_submit_task(params, state).await,
        "background_model.list_tasks" => handle_background_model_list_tasks(params, state).await,

        // Subagent Orchestrator (Phase 2)
        "subagent.spawn" => handle_subagent_spawn(params, state).await,
        "subagent.list" => handle_subagent_list(params, state).await,
        "subagent.get" => handle_subagent_get(params, state).await,
        "subagent.cancel" => handle_subagent_cancel(params, state).await,

        // Slack Bridge (Phase 2)
        "slack.configure" => handle_slack_configure(params, state).await,
        "slack.config.get" => handle_slack_config_get(params, state).await,
        "slack.trigger_mission" => handle_slack_trigger_mission(params, state).await,

        // Teams Bridge (Phase 2)
        "teams.configure" => handle_teams_configure(params, state).await,
        "teams.config.get" => handle_teams_config_get(params, state).await,
        "teams.trigger_mission" => handle_teams_trigger_mission(params, state).await,

        // MCP Tasks (Phase 2)
        "mcp.task.create" => handle_mcp_task_create(params, state).await,
        "mcp.task.poll" => handle_mcp_task_poll(params, state).await,
        "mcp.task.subscribe" => handle_mcp_task_subscribe(params, state).await,
        "mcp.task.cancel" => handle_mcp_task_cancel(params, state).await,
        "mcp.task.list" => handle_mcp_task_list(state).await,

        // Semantic Engine (Phase 2)
        "semantic_engine.status" => handle_semantic_engine_status(params, state).await,
        "semantic_engine.enable" => handle_semantic_engine_enable(params, state).await,
        "semantic_engine.disable" => handle_semantic_engine_disable(params, state).await,
        "semantic_engine.search" => handle_semantic_engine_search(params, state).await,
        "semantic_engine.dependency_graph" => {
            handle_semantic_engine_dependency_graph(params, state).await
        }
        "semantic_engine.git_blame" => handle_semantic_engine_git_blame(params, state).await,
        "semantic_engine.index_file" => handle_semantic_engine_index_file(params, state).await,
        "semantic_engine.load_blame" => handle_semantic_engine_load_blame(params, state).await,
        "semantic_engine.test_impact.for_symbol" => {
            handle_test_impact_for_symbol(params, state).await
        }
        "semantic_engine.test_impact.for_symbols" => {
            handle_test_impact_for_symbols(params, state).await
        }
        "semantic_engine.test_impact.entries" => handle_test_impact_entries(params, state).await,
        "semantic_engine.docs.for_symbol" => handle_docs_for_symbol(params, state).await,
        "semantic_engine.docs.stale" => handle_stale_docs(params, state).await,

        // Sandbox (Phase 2)
        "sandbox.test" => handle_sandbox_test(params, state).await,
        "sandbox.status" => handle_sandbox_status(state).await,
        "sandbox.network_allowlist.get" => handle_sandbox_network_allowlist_get(state).await,
        "sandbox.network_allowlist.set" => {
            handle_sandbox_network_allowlist_set(params, state).await
        }

        // ACP host (Agent Client Protocol — Zed/JetBrains handoff)
        "acp.editors.list" => handle_acp_editors_list(state).await,
        "acp.handoff" => handle_acp_handoff(params, state).await,
        "acp.take_back" => handle_acp_take_back(params, state).await,
        "acp.handoffs.list" => handle_acp_handoffs_list(params, state).await,
        "acp.handoff.get" => handle_acp_handoff_get(params, state).await,
        "acp.handoff.remove" => handle_acp_handoff_remove(params, state).await,

        // Configurable role profiles (Phase 4, Part A)
        "role_profile.create" => handle_role_profile_create(params, state).await,
        "role_profile.update" => handle_role_profile_update(params, state).await,
        "role_profile.delete" => handle_role_profile_delete(params, state).await,
        "role_profile.get" => handle_role_profile_get(params, state).await,
        "role_profile.list" => handle_role_profile_list(params, state).await,
        "role_profile.check_permission" => {
            handle_role_profile_check_permission(params, state).await
        }

        // Decisions view + deployment record (Phase 4, Part A)
        "decisions.list" => handle_decisions_list(params, state).await,
        "decisions.for_mission" => handle_decisions_for_mission(params, state).await,
        "deployment.record" => handle_deployment_record(params, state).await,
        "deployment.webhook" => handle_deployment_webhook(params, state).await,
        "deployment.list" => handle_deployment_list(params, state).await,

        // Confidence Engine (Phase 4, Part A)
        "confidence.score" => handle_confidence_score(params, state).await,
        "confidence.history" => handle_confidence_history(params, state).await,

        // GitLab / Bitbucket bridges (Phase 3, Part 16)
        "forge.connect" => handle_forge_connect(params, state).await,
        "forge.config.get" => handle_forge_config_get(params, state).await,
        "forge.disconnect" => handle_forge_disconnect(params, state).await,
        "forge.issues.list" => handle_forge_issues_list(params, state).await,
        "forge.issue.get" => handle_forge_issue_get(params, state).await,
        "forge.issue.to_mission" => handle_forge_issue_to_mission(params, state).await,
        "forge.change_request.create" => handle_forge_cr_create(params, state).await,
        "forge.change_request.list" => handle_forge_cr_list(params, state).await,
        "forge.change_request.status" => handle_forge_cr_status(params, state).await,

        // Jira / Linear linkage (Phase 3, Part 16 — linkage only)
        "tracker.token.set" => handle_tracker_token_set(params, state).await,
        "tracker.status" => handle_tracker_status(state).await,
        "tracker.issue.get" => handle_tracker_issue_get(params, state).await,
        "tracker.link" => handle_tracker_link(params, state).await,
        "tracker.links.list" => handle_tracker_links_list(params, state).await,
        "tracker.unlink" => handle_tracker_unlink(params, state).await,
        "tracker.issue.to_mission" => handle_tracker_issue_to_mission(params, state).await,
        "tracker.comment" => handle_tracker_comment(params, state).await,

        // Accounts, sessions, roles (Phase 3, ADR 0013)
        "auth.status" => handle_auth_status(state).await,
        "auth.register" => handle_auth_register(params, state).await,
        "auth.login" => handle_auth_login(params, state).await,
        "auth.logout" => handle_auth_logout(params, state).await,
        "auth.session" => handle_auth_session(params, state).await,
        "auth.users.list" => handle_auth_users_list(params, state).await,
        "auth.user.set_role" => handle_auth_set_role(params, state).await,
        "auth.user.set_active" => handle_auth_set_active(params, state).await,
        "auth.user.change_password" => handle_auth_change_password(params, state).await,

        // Workspace governance and policy (Part 14)
        "governance.policy.get" => handle_governance_policy_get(params, state).await,
        "governance.policy.set" => handle_governance_policy_set(params, state).await,
        "governance.check.autonomous" => handle_governance_check_autonomous(params, state).await,
        "governance.check.plan_approval" => handle_governance_check_plan(params, state).await,
        "governance.check.merge" => handle_governance_check_merge(params, state).await,
        "governance.spend.check" => handle_governance_spend_check(params, state).await,
        "governance.spend.record" => handle_governance_spend_record(params, state).await,
        "governance.spend.summary" => handle_governance_spend_summary(params, state).await,

        // Planner / Reviewer (Part 5, Flow 1 steps 3 and 6)
        "mission.plan.generate" => handle_mission_plan_generate(params, state).await,
        "mission.plan.get" => handle_mission_plan_get(params, state).await,
        "mission.plan.update" => handle_mission_plan_update(params, state).await,
        "mission.plan.approve" => handle_mission_plan_approve(params, state).await,
        "mission.plan.reject" => handle_mission_plan_reject(params, state).await,
        "mission.review.run" => handle_mission_review_run(params, state).await,
        "mission.review.get" => handle_mission_review_get(params, state).await,
        "mission.review.list" => handle_mission_review_list(params, state).await,

        // Context compaction (Part 6/review_prompt.md §3.1): a usage
        // indicator plus the `/compact` composer command's backend.
        "mission.context.usage" => handle_mission_context_usage(params, state).await,
        "mission.context.compact" => handle_mission_context_compact(params, state).await,

        // Checkpoint/rewind (review_prompt.md §3.2), built on the git
        // worktree every Mission already has.
        "mission.checkpoint.list" => handle_mission_checkpoint_list(params, state).await,
        "mission.checkpoint.rewind" => handle_mission_checkpoint_rewind(params, state).await,

        // Skills — multi-file SKILL.md bundles and resolution stack
        "skills.bundles.list" => handle_skills_bundles_list(params, state).await,
        "skills.bundle.write" => handle_skills_bundle_write(params, state).await,
        "skills.resolve" => handle_skills_resolve(params, state).await,

        // Repository Health & observability (Phase 6)
        "repo_health.scan" => handle_repo_health_scan(params).await,
        "observability.crashes.list" => handle_observability_crashes_list(state).await,

        _ => Err(anyhow::anyhow!("Method not found: {}", method)),
    };

    if result.is_err() {
        state.metrics.inc_counter("cid_rpc_errors_total");
    }

    match result {
        Ok(val) => JsonRpcResponse::success(id, val),
        Err(e) => JsonRpcResponse::error(id, -32000, format!("{:?}", e)),
    }
}

async fn handle_repo_health_scan(params: serde_json::Value) -> anyhow::Result<serde_json::Value> {
    let path = required_str(&params, "path")?;
    let report = crate::repo_health::scan_repo_health(std::path::Path::new(&path));
    Ok(serde_json::to_value(report)?)
}

async fn handle_observability_crashes_list(state: &AppState) -> anyhow::Result<serde_json::Value> {
    Ok(serde_json::to_value(state.crash_log.list())?)
}

// ============ Handlers ============

/// Pull a required string param, failing with the param name rather than a serde error.
fn required_str(params: &serde_json::Value, key: &str) -> anyhow::Result<String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("{} is required", key))
}

/// Fan a JSON-RPC notification out to every connected client.
fn broadcast_notification(state: &AppState, method: &str, params: serde_json::Value) {
    let notif = JsonRpcNotification {
        jsonrpc: "2.0".to_string(),
        method: method.to_string(),
        params,
    };
    if let Ok(s) = serde_json::to_string(&notif) {
        let _ = state.event_tx.send(s);
    }
}

async fn handle_workspace_list(state: &AppState) -> anyhow::Result<serde_json::Value> {
    let ws = state.persistence.list_workspaces()?;
    Ok(serde_json::to_value(ws)?)
}

async fn handle_workspace_get(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let id: String = serde_json::from_value(params.get("id").cloned().unwrap_or_default())?;
    let ws = state.persistence.get_workspace(&id)?;
    Ok(serde_json::to_value(ws)?)
}

async fn handle_repo_connect(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: crate::api::types::ConnectRepoParams = serde_json::from_value(params)?;
    let repo = state
        .persistence
        .connect_repo(&p.path, p.workspace_id.as_deref())?;
    // Detect AGENTS.md
    let agents_md = state.context_manager.detect_agents_md(&p.path);
    let mut repo_with_ctx = repo.clone();
    repo_with_ctx.agents_md_content = agents_md;

    // Auto-create .cid/.gitignore entry for worktrees
    let cid_ignore_path = std::path::Path::new(&p.path).join(".cid");
    if !cid_ignore_path.exists() {
        let _ = std::fs::create_dir_all(&cid_ignore_path);
    }
    let gitignore_path = std::path::Path::new(&p.path).join(".gitignore");
    if gitignore_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&gitignore_path) {
            if !content.contains(".cid/") {
                let _ = std::fs::write(&gitignore_path, format!("{}\n.cid/\n", content));
            }
        }
    }

    // Start file watcher for this repo (polling every 5s for git status changes)
    let state_clone = state.clone();
    let repo_path = p.path.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        let mut last_status_hash = String::new();
        loop {
            interval.tick().await;
            if let Ok(status) = state_clone.git_manager.status(&repo_path) {
                let hash = format!("{:?}", status);
                if hash != last_status_hash {
                    last_status_hash = hash;
                    let notif = JsonRpcNotification {
                        jsonrpc: "2.0".to_string(),
                        method: "git.diff.update".to_string(),
                        params: serde_json::json!({ "repo_path": repo_path, "status": status }),
                    };
                    if let Ok(s) = serde_json::to_string(&notif) {
                        let _ = state_clone.event_tx.send(s);
                    }
                }
            }
        }
    });

    Ok(serde_json::to_value(repo_with_ctx)?)
}

async fn handle_repo_list(state: &AppState) -> anyhow::Result<serde_json::Value> {
    let repos = state.persistence.list_repo_channels()?;
    Ok(serde_json::to_value(repos)?)
}

async fn handle_repo_get(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let id: String = serde_json::from_value(
        params
            .get("id")
            .cloned()
            .unwrap_or(serde_json::Value::String("".to_string())),
    )?;
    if id.is_empty() {
        let repo_id: String = serde_json::from_value(params.clone())?;
        let repo = state.persistence.get_repo_channel(&repo_id)?;
        return Ok(serde_json::to_value(repo)?);
    }
    let repo = state.persistence.get_repo_channel(&id)?;
    Ok(serde_json::to_value(repo)?)
}

async fn handle_repo_disconnect(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let id: String = serde_json::from_value(params.get("id").cloned().unwrap_or_default())?;
    state.persistence.disconnect_repo(&id)?;
    Ok(serde_json::json!({ "ok": true }))
}

async fn handle_repo_agents_md(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let path: String = serde_json::from_value(params.get("path").cloned().unwrap_or_default())?;
    let content = state.context_manager.detect_agents_md(&path);
    Ok(serde_json::json!({ "path": path, "content": content }))
}

async fn handle_repo_agents_md_write(
    params: serde_json::Value,
    _state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let path: String = serde_json::from_value(params.get("path").cloned().unwrap_or_default())?;
    let content: String =
        serde_json::from_value(params.get("content").cloned().unwrap_or_default())?;
    let agents_path = if path.ends_with("AGENTS.md") {
        path.clone()
    } else {
        format!("{}/AGENTS.md", path.trim_end_matches('/'))
    };
    tokio::fs::write(&agents_path, &content).await?;
    Ok(serde_json::json!({ "ok": true, "path": agents_path }))
}

/// review_prompt.md §1.2 point 2: the human-review gate for a repo's
/// AGENTS.md. `handle_repo_connect` detects and surfaces AGENTS.md content
/// but never approves it — a Mission on this repo will not include it in the
/// system prompt (see `ModelManager::process_message_with_role`) until this
/// RPC is called, which happens when the user dismisses the frontend's
/// one-time review card.
async fn handle_repo_agents_md_approve(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let id: String = serde_json::from_value(params.get("id").cloned().unwrap_or_default())?;
    state.persistence.approve_agents_md(&id)?;
    let repo = state.persistence.get_repo_channel(&id)?;
    Ok(serde_json::to_value(repo)?)
}

async fn handle_mission_create(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: crate::api::types::CreateMissionParams = serde_json::from_value(params.clone())?;

    if p.title.trim().is_empty() {
        anyhow::bail!("Mission title must not be empty");
    }

    // Governance gate: Autonomous mode is a Workspace policy decision, not a
    // per-Mission preference. Enforced here so no shell can bypass it.
    if p.autonomy_level == Some(crate::api::types::AutonomyLevel::Autonomous) {
        let repo = state.persistence.get_repo_channel(&p.repo_channel_id)?;
        let actor = require_session(&params, state)?;
        let decision =
            state
                .governance_manager
                .can_enable_autonomous(&actor, &repo.workspace_id, &repo.path);
        if !decision.allowed() {
            anyhow::bail!("{}", decision.reason());
        }
    }

    // task 2: a Mission must never be created with an empty task, because
    // the Planner prompt (`ModelManager::process_message_with_role` ->
    // `SkillsManager::build_system_context`) depends on it — falling back to
    // the (already-validated non-empty) title is the intended behavior for
    // an omitted/blank task, not an error.
    let task = if p.task.trim().is_empty() {
        p.title.clone()
    } else {
        p.task.clone()
    };

    let mission = state.persistence.create_mission(
        &p.repo_channel_id,
        &p.title,
        &task,
        p.session_mode
            .unwrap_or(crate::api::types::SessionMode::Worktree),
        p.autonomy_level
            .unwrap_or(crate::api::types::AutonomyLevel::CoPilot),
    )?;

    let mission = if p.model_provider.is_some() || p.model_id.is_some() {
        state.persistence.update_mission_model(
            &mission.id,
            p.model_provider.clone(),
            p.model_id.clone(),
        )?
    } else {
        mission
    };

    // If worktree mode, create worktree via git manager
    let mission = if mission.session_mode == crate::api::types::SessionMode::Worktree {
        let repo = state.persistence.get_repo_channel(&p.repo_channel_id)?;
        let branch_name = format!("cid/{}", &mission.id[..8]);
        let worktree_base = state
            .persistence
            .get_settings()?
            .worktree_root
            .unwrap_or_else(|| format!("{}/.cid/worktrees", repo.path));
        let worktree_path = format!("{}/{}", worktree_base, mission.id);
        match state
            .git_manager
            .create_worktree(&repo.path, &branch_name, &worktree_path)
        {
            Ok(_) => state.persistence.update_mission_worktree(
                &mission.id,
                Some(worktree_path),
                Some(branch_name),
            )?,
            Err(e) => {
                tracing::warn!(
                    "Failed to create worktree: {}, falling back to shared mode",
                    e
                );
                mission
            }
        }
    } else {
        mission
    };

    // Vibe-coding preset (Phase 5): a minimal plan is generated and
    // auto-approved synchronously, so the gate is already open by the time
    // this response reaches the caller — no need to wait on a
    // mission.plan.changed notification for a quick, low-stakes Mission.
    if p.vibe {
        if let Err(e) = state.role_runner.generate_vibe_plan(&mission.id) {
            error!(
                "Vibe plan generation failed for mission {}: {:?}",
                mission.id, e
            );
        }
    } else if mission.autonomy_level != crate::api::types::AutonomyLevel::Manual {
        // Flow 1 step 3: the Planner proposes a plan in-thread as soon as the
        // Mission exists. Run it in the background so creation stays
        // responsive; the thread gets the plan via mission.plan.changed.
        let state_clone = state.clone();
        let mission_id = mission.id.clone();
        tokio::spawn(async move {
            match state_clone
                .role_runner
                .generate_plan(&mission_id, false)
                .await
            {
                Ok(plan) => {
                    let _ = state_clone.persistence.update_mission_status(
                        &mission_id,
                        crate::api::types::MissionStatus::BlockedOnApproval,
                    );
                    if let Ok(v) = serde_json::to_value(&plan) {
                        broadcast_notification(&state_clone, "mission.plan.changed", v);
                    }
                }
                Err(e) => error!("Planner failed for mission {}: {:?}", mission_id, e),
            }
        });
    }

    Ok(serde_json::to_value(mission)?)
}

async fn handle_mission_list(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_channel_id: Option<String> = params
        .get("repo_channel_id")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let missions = state
        .persistence
        .list_missions(repo_channel_id.as_deref())?;
    Ok(serde_json::to_value(missions)?)
}

async fn handle_mission_get(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let id: String = serde_json::from_value(params.get("id").cloned().unwrap_or_default())?;
    let mission = state.persistence.get_mission(&id)?;
    Ok(serde_json::to_value(mission)?)
}

async fn handle_mission_close(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let id: String = serde_json::from_value(params.get("id").cloned().unwrap_or_default())?;

    // Flow 1 step 6: a Mission marked done gets an automatic Reviewer pass before
    // it closes, unless the caller explicitly opts out.
    let run_review = params
        .get("skip_review")
        .and_then(|v| v.as_bool())
        .map(|skip| !skip)
        .unwrap_or(true);
    if run_review {
        let state_clone = state.clone();
        let mission_id = id.clone();
        tokio::spawn(async move {
            let diff = mission_diff(&state_clone, &mission_id).unwrap_or_default();
            match state_clone.role_runner.run_review(&mission_id, &diff).await {
                Ok(review) => {
                    if let Ok(v) = serde_json::to_value(&review) {
                        broadcast_notification(&state_clone, "mission.review.completed", v);
                    }
                }
                Err(e) => error!("Reviewer failed for mission {}: {:?}", mission_id, e),
            }
        });
    }

    let mission = state
        .persistence
        .update_mission_status(&id, crate::api::types::MissionStatus::Closed)?;
    Ok(serde_json::to_value(mission)?)
}

/// Read a Mission's diff from its own working directory — its worktree when it
/// has one, otherwise the repo channel's path for shared-clone Missions.
fn mission_diff(state: &AppState, mission_id: &str) -> anyhow::Result<String> {
    let mission = state.persistence.get_mission(mission_id)?;
    let path = match mission.worktree_path.clone() {
        Some(wt) => wt,
        None => {
            state
                .persistence
                .get_repo_channel(&mission.repo_channel_id)?
                .path
        }
    };
    let diff = state.git_manager.diff(&path, None)?;
    Ok(serde_json::to_string_pretty(&diff)?)
}

async fn handle_mission_send_message(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: crate::api::types::SendMessageParams = serde_json::from_value(params)?;
    // Save user message
    let user_msg = state.persistence.create_message(
        &p.mission_id,
        crate::api::types::MessageRole::User,
        &p.content,
        vec![],
    )?;

    // Plan-approval gate (Part 5, Flow 1 step 3): outside Manual autonomy the
    // Implementer does not run until a plan exists and a human has approved it.
    if let Some(reason) = state.role_runner.implementer_is_gated(&p.mission_id)? {
        let note = format!(
            "{reason}\n\nRun `mission.plan.generate`, edit the plan if needed, then approve it."
        );
        state.persistence.create_message(
            &p.mission_id,
            crate::api::types::MessageRole::System,
            &note,
            vec![],
        )?;
        let _ = state.persistence.update_mission_status(
            &p.mission_id,
            crate::api::types::MissionStatus::BlockedOnApproval,
        );
        broadcast_notification(
            state,
            "mission.blocked",
            serde_json::json!({ "mission_id": p.mission_id, "reason": reason }),
        );
        return Ok(serde_json::json!({ "message": user_msg, "blocked": true, "reason": reason }));
    }

    // Trigger agent loop in background
    let state_clone = state.clone();
    let mission_id = p.mission_id.clone();
    tokio::spawn(async move {
        if let Err(e) = state_clone
            .model_manager
            .process_message(&mission_id, &p.content, state_clone.clone())
            .await
        {
            error!("Model processing failed: {:?}", e);
            let _ = state_clone.persistence.create_message(
                &mission_id,
                crate::api::types::MessageRole::System,
                &format!("Error: {:?}", e),
                vec![],
            );
        }
    });

    Ok(serde_json::to_value(user_msg)?)
}

async fn handle_mission_approve_tool(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: crate::api::types::ToolApprovalParams = serde_json::from_value(params)?;
    state
        .model_manager
        .approve_tool_call(&p.mission_id, &p.tool_call_id, p.approved)
        .await?;
    Ok(serde_json::json!({ "ok": true }))
}

async fn handle_message_list(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let mission_id: String =
        serde_json::from_value(params.get("mission_id").cloned().unwrap_or_default())?;
    let messages = state.persistence.list_messages(&mission_id)?;
    Ok(serde_json::to_value(messages)?)
}

async fn handle_git_status(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: crate::api::types::GitStatusParams = serde_json::from_value(params)?;
    let status = state.git_manager.status(&p.repo_path)?;
    Ok(serde_json::to_value(status)?)
}

async fn handle_git_diff(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: crate::api::types::GitDiffParams = serde_json::from_value(params)?;
    let diff = state.git_manager.diff(&p.repo_path, p.base.as_deref())?;
    Ok(serde_json::to_value(diff)?)
}

async fn handle_git_commit(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: crate::api::types::GitCommitParams = serde_json::from_value(params)?;
    let oid = state.git_manager.commit(&p.repo_path, &p.message)?;
    Ok(serde_json::json!({ "oid": oid }))
}

async fn handle_git_log(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let path: String =
        serde_json::from_value(params.get("repo_path").cloned().unwrap_or_default())?;
    let logs = state.git_manager.log(&path, 50)?;
    Ok(serde_json::to_value(logs)?)
}

async fn handle_worktree_list(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let path: String =
        serde_json::from_value(params.get("repo_path").cloned().unwrap_or_default())?;
    let list = state.git_manager.list_worktrees(&path)?;
    Ok(serde_json::to_value(list)?)
}

async fn handle_worktree_create(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: crate::api::types::WorktreeCreateParams = serde_json::from_value(params)?;
    state
        .git_manager
        .create_worktree(&p.repo_path, &p.branch, &p.worktree_path)?;
    Ok(serde_json::json!({ "ok": true }))
}

async fn handle_worktree_remove(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path: String =
        serde_json::from_value(params.get("repo_path").cloned().unwrap_or_default())?;
    let worktree_path: String =
        serde_json::from_value(params.get("worktree_path").cloned().unwrap_or_default())?;
    state
        .git_manager
        .remove_worktree(&repo_path, &worktree_path)?;
    Ok(serde_json::json!({ "ok": true }))
}

async fn handle_git_hunk_apply(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path: String =
        serde_json::from_value(params.get("repo_path").cloned().unwrap_or_default())?;
    let file_path: String =
        serde_json::from_value(params.get("file_path").cloned().unwrap_or_default())?;
    let hunk_id: String =
        serde_json::from_value(params.get("hunk_id").cloned().unwrap_or_default())?;
    let action: String = serde_json::from_value(
        params
            .get("action")
            .cloned()
            .unwrap_or(serde_json::Value::String("accept".to_string())),
    )?;
    // The hunk's own diff text — its `@@ ... @@` header and +/-/space-prefixed
    // body — as the client already has it from its last `git.diff` call.
    // `hunk_id` alone cannot identify a hunk server-side: `GitManager::diff`
    // (`git/mod.rs`) mints a fresh UUID on every call, so an ID from one
    // `git.diff` response has no meaning to a later `git.hunk.apply` call.
    // review_prompt.md §6: this used to fall back to discarding the *entire*
    // file's changes on any reject, silently losing every other hunk in that
    // file — found via review, fixed here with a real reverse-patch.
    let header: Option<String> = params
        .get("header")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let content: Option<String> = params
        .get("content")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if action == "reject" {
        if hunk_id == "all" {
            // An explicit whole-file reject ("Reject file" in the UI, not a
            // per-hunk one) — discards every change in this file, by design.
            let full_path = std::path::Path::new(&repo_path).join(&file_path);
            if full_path.exists() {
                let _ = std::process::Command::new("git")
                    .args(["checkout", "HEAD", "--", &file_path])
                    .current_dir(&repo_path)
                    .output();
            }
        } else {
            match (header, content) {
                (Some(header), Some(content)) => {
                    reverse_apply_hunk(&repo_path, &file_path, &header, &content)?;
                }
                _ => {
                    anyhow::bail!(
                        "git.hunk.apply reject requires the hunk's `header` and `content` \
                         (as returned by git.diff) to reverse only that hunk — without them \
                         there is no safe way to know which change to discard."
                    );
                }
            }
        }
    }
    // For accept, do nothing - changes already in workdir, they will be committed later
    // Notify diff update
    let notif = JsonRpcNotification {
        jsonrpc: "2.0".to_string(),
        method: "git.diff.update".to_string(),
        params: serde_json::json!({ "repo_path": repo_path, "file_path": file_path, "hunk_id": hunk_id, "action": action }),
    };
    if let Ok(s) = serde_json::to_string(&notif) {
        let _ = state.event_tx.send(s);
    }

    Ok(serde_json::json!({ "ok": true, "action": action }))
}

/// Reverse-apply exactly one hunk (its unified-diff `header` + `content`,
/// e.g. `@@ -10,5 +10,6 @@` plus the following +/-/space-prefixed lines) to
/// `file_path` in `repo_path`, leaving every other hunk in that file — and
/// every other file — untouched. Shells out to `git apply -R`, the same tool
/// `git add -p`'s own reject path uses, rather than hand-rolling a patch
/// applier.
fn reverse_apply_hunk(
    repo_path: &str,
    file_path: &str,
    header: &str,
    content: &str,
) -> anyhow::Result<()> {
    // A minimal but complete unified diff: `git apply` needs the file headers
    // even for a single hunk, and needs both `a/` and `b/` paths to identify
    // which file the hunk belongs to.
    let normalized_path = file_path.replace('\\', "/");
    let mut patch = String::new();
    patch.push_str(&format!("--- a/{normalized_path}\n"));
    patch.push_str(&format!("+++ b/{normalized_path}\n"));
    patch.push_str(header.trim_end());
    patch.push('\n');
    patch.push_str(content);
    if !patch.ends_with('\n') {
        patch.push('\n');
    }

    let mut child = std::process::Command::new("git")
        .args(["apply", "-R", "--whitespace=nowarn", "-"])
        .current_dir(repo_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("failed to start `git apply`")?;

    {
        use std::io::Write;
        let stdin = child
            .stdin
            .as_mut()
            .context("git apply stdin unavailable")?;
        stdin
            .write_all(patch.as_bytes())
            .context("failed to write patch to git apply")?;
    }

    let output = child
        .wait_with_output()
        .context("failed to wait for git apply")?;
    if !output.status.success() {
        anyhow::bail!(
            "git apply -R failed for {}: {}",
            file_path,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

// PTY handlers - fixed to avoid thread-per-subscriber leak, use broadcast receiver directly

async fn handle_pty_create(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: crate::api::types::PtyCreateParams = serde_json::from_value(params)?;
    let mission = state.persistence.get_mission(&p.mission_id)?;
    let repo = state
        .persistence
        .get_repo_channel(&mission.repo_channel_id)?;
    let workdir = mission.worktree_path.unwrap_or(repo.path);
    let pty = state.pty_manager.create_pty(
        &p.mission_id,
        &workdir,
        p.cols.unwrap_or(120),
        p.rows.unwrap_or(30),
    )?;

    // Subscribe to output and broadcast via event_tx - single task per PTY, not per subscriber leak
    let event_tx = state.event_tx.clone();
    if let Ok(mut rx) = state.pty_manager.get_receiver(&pty.id) {
        let pty_id = pty.id.clone();
        tokio::spawn(async move {
            while let Ok(data) = rx.recv().await {
                // Secret redaction - Phase 0.1: basic pattern matching for common secrets
                let redacted = redact_secrets(&data);
                let notif = JsonRpcNotification {
                    jsonrpc: "2.0".to_string(),
                    method: "pty.output".to_string(),
                    params: serde_json::json!({ "pty_id": pty_id, "data": redacted }),
                };
                if let Ok(s) = serde_json::to_string(&notif) {
                    let _ = event_tx.send(s);
                }
            }
        });
    }

    Ok(serde_json::to_value(pty)?)
}

use crate::redact::redact_secrets;

async fn handle_pty_write(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: crate::api::types::PtyWriteParams = serde_json::from_value(params)?;
    state.pty_manager.write(&p.pty_id, &p.data)?;
    Ok(serde_json::json!({ "ok": true }))
}

async fn handle_pty_resize(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: crate::api::types::PtyResizeParams = serde_json::from_value(params)?;
    state.pty_manager.resize(&p.pty_id, p.cols, p.rows)?;
    Ok(serde_json::json!({ "ok": true }))
}

async fn handle_pty_kill(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let pty_id: String = serde_json::from_value(params.get("pty_id").cloned().unwrap_or_default())?;
    state.pty_manager.kill(&pty_id)?;
    Ok(serde_json::json!({ "ok": true }))
}

async fn handle_pty_list(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let mission_id: String =
        serde_json::from_value(params.get("mission_id").cloned().unwrap_or_default())?;
    let list = state.pty_manager.list(&mission_id);
    Ok(serde_json::to_value(list)?)
}

// MCP

async fn handle_mcp_list(state: &AppState) -> anyhow::Result<serde_json::Value> {
    let servers = state.mcp_manager.list_servers().await;
    Ok(serde_json::to_value(servers)?)
}

async fn handle_mcp_add(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: crate::api::types::McpAddServerParams = serde_json::from_value(params)?;
    let server = state
        .mcp_manager
        .add_server(&p.name, p.transport_type, p.config)
        .await?;
    state.persistence.save_mcp_server(&server)?;
    Ok(serde_json::to_value(server)?)
}

async fn handle_mcp_remove(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let id: String = serde_json::from_value(params.get("id").cloned().unwrap_or_default())?;
    state.mcp_manager.remove_server(&id).await?;
    state.persistence.delete_mcp_server(&id)?;
    Ok(serde_json::json!({ "ok": true }))
}

async fn handle_mcp_tools_list(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let server_id: String =
        serde_json::from_value(params.get("server_id").cloned().unwrap_or_default())?;
    let tools = state.mcp_manager.list_tools(&server_id).await?;
    Ok(serde_json::to_value(tools)?)
}

async fn handle_mcp_tool_call(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: crate::api::types::McpCallToolParams = serde_json::from_value(params)?;
    let result = state
        .mcp_manager
        .call_tool(&p.server_id, &p.tool_name, p.arguments)
        .await?;
    Ok(serde_json::to_value(result)?)
}

// File
//
// review_prompt.md §1.1 confined the model's own file tools to a Mission's
// worktree; it did not touch these RPCs, which the Editor pane calls directly
// over the network socket (050-Gold-Standard-Review.md F1). A caller here has
// no Mission root to confine to, so the boundary is "inside some repo the
// user has actually connected" — resolved against every connected repo
// channel via `path_confine::resolve_confined_path_in_any`, which is the same
// primitive (not a second implementation of it) the model tools use.

/// Every connected repo channel's filesystem path, as confinement roots.
fn connected_repo_roots(state: &AppState) -> anyhow::Result<Vec<std::path::PathBuf>> {
    Ok(state
        .persistence
        .list_repo_channels()?
        .into_iter()
        .map(|r| std::path::PathBuf::from(r.path))
        .collect())
}

async fn handle_file_read(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: crate::api::types::FileReadParams = serde_json::from_value(params)?;
    let roots = connected_repo_roots(state)?;
    let resolved = crate::path_confine::resolve_confined_path_in_any(&roots, &p.path)?;
    let content = tokio::fs::read_to_string(&resolved).await?;
    Ok(serde_json::json!({ "path": p.path, "content": content }))
}

async fn handle_file_write(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: crate::api::types::FileWriteParams = serde_json::from_value(params)?;
    let roots = connected_repo_roots(state)?;
    let resolved = crate::path_confine::resolve_confined_path_in_any(&roots, &p.path)?;
    if let Some(parent) = resolved.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&resolved, &p.content).await?;
    Ok(serde_json::json!({ "ok": true }))
}

async fn handle_file_list(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let path: String = serde_json::from_value(
        params
            .get("path")
            .cloned()
            .unwrap_or(serde_json::Value::String(".".to_string())),
    )?;
    let roots = connected_repo_roots(state)?;
    let resolved = crate::path_confine::resolve_confined_path_in_any(&roots, &path)?;
    let mut entries = Vec::new();
    let mut read_dir = tokio::fs::read_dir(&resolved).await?;
    while let Some(entry) = read_dir.next_entry().await? {
        let meta = entry.metadata().await?;
        entries.push(serde_json::json!({
            "name": entry.file_name().to_string_lossy(),
            "path": entry.path().to_string_lossy(),
            "is_dir": meta.is_dir(),
            "is_file": meta.is_file(),
            "size": meta.len(),
        }));
    }
    Ok(serde_json::Value::Array(entries))
}

/// `fs.list_dirs` — a directory-only browser for the repo-connect picker,
/// deliberately *not* confined to a connected repo the way `file.*` above is
/// (its whole job is finding the repo to connect in the first place). This
/// is the security boundary the tests in `api_integration.rs`'s
/// `fs_list_dirs` module exist to pin down: directory names only, anywhere
/// the Core process can read — never file names, never file contents. No
/// `require_session` here, matching `repo.list`/`file.*`'s existing pattern
/// of leaving this RPC surface gated by the loopback bind rather than a
/// per-call session check — see SECURITY.md.
async fn handle_fs_list_dirs(
    params: serde_json::Value,
    _state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let requested_path: Option<String> = params
        .get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let Some(raw_path) = requested_path else {
        return Ok(serde_json::json!({
            "path": "",
            "parent": serde_json::Value::Null,
            "entries": fs_list_roots().await,
        }));
    };

    // `canonicalize` hands back Windows' extended-length form
    // (`\\?\C:\Projects\cid`). `simplified` is the string-only counterpart to
    // `path_confine::normalize_stored_path` (no second filesystem round-trip
    // needed — we just canonicalized), so the paths this RPC feeds the folder
    // picker are the same spelling `repo.connect` will store. Every child
    // below inherits it, since `fs_list_subdirs` joins onto this value.
    let canonical = tokio::fs::canonicalize(&raw_path)
        .await
        .with_context(|| format!("cannot access '{raw_path}'"))?;
    let canonical = dunce::simplified(&canonical).to_path_buf();
    let meta = tokio::fs::metadata(&canonical)
        .await
        .with_context(|| format!("cannot stat '{raw_path}'"))?;
    if !meta.is_dir() {
        anyhow::bail!("'{raw_path}' is not a directory");
    }

    let entries = fs_list_subdirs(&canonical).await;
    let parent = canonical.parent().map(|p| p.to_string_lossy().to_string());
    Ok(serde_json::json!({
        "path": canonical.to_string_lossy(),
        "parent": parent,
        "entries": entries,
    }))
}

/// True when `dir` contains a `.git` entry (directory for a normal repo, or
/// file for a worktree/submodule) — the only signal `fs.list_dirs` gives the
/// picker about repo-ness, no file contents read.
async fn is_git_repo_dir(dir: &std::path::Path) -> bool {
    tokio::fs::metadata(dir.join(".git")).await.is_ok()
}

/// Filesystem roots for a null/absent `path` — logical drives on Windows,
/// a single "/" entry elsewhere.
#[cfg(windows)]
async fn fs_list_roots() -> Vec<serde_json::Value> {
    let mut entries = Vec::new();
    for letter in b'A'..=b'Z' {
        let drive = format!("{}:\\", letter as char);
        let drive_path = std::path::PathBuf::from(&drive);
        if tokio::fs::metadata(&drive_path).await.is_ok() {
            let is_git_repo = is_git_repo_dir(&drive_path).await;
            entries.push(serde_json::json!({
                "name": drive,
                "path": drive_path.to_string_lossy(),
                "is_git_repo": is_git_repo,
            }));
        }
    }
    entries
}

#[cfg(not(windows))]
async fn fs_list_roots() -> Vec<serde_json::Value> {
    let root = std::path::PathBuf::from("/");
    let is_git_repo = is_git_repo_dir(&root).await;
    vec![serde_json::json!({
        "name": "/",
        "path": "/",
        "is_git_repo": is_git_repo,
    })]
}

/// Lists the directory-only, non-hidden children of `dir`, sorted
/// case-insensitively by name. An entry that errors when stat'd (permission
/// denied) is skipped rather than failing the whole listing; a broken
/// directory stream stops the listing with whatever was gathered so far
/// instead of retrying indefinitely against the same error.
async fn fs_list_subdirs(dir: &std::path::Path) -> Vec<serde_json::Value> {
    let mut entries = Vec::new();
    let mut read_dir = match tokio::fs::read_dir(dir).await {
        Ok(rd) => rd,
        Err(_) => return entries,
    };
    loop {
        let entry = match read_dir.next_entry().await {
            Ok(Some(e)) => e,
            Ok(None) => break,
            Err(_) => break,
        };
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let meta = match entry.metadata().await {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !meta.is_dir() {
            continue;
        }
        let full_path = entry.path();
        let is_git_repo = is_git_repo_dir(&full_path).await;
        entries.push(serde_json::json!({
            "name": name,
            "path": full_path.to_string_lossy(),
            "is_git_repo": is_git_repo,
        }));
    }
    entries.sort_by(|a, b| {
        let an = a["name"].as_str().unwrap_or("").to_lowercase();
        let bn = b["name"].as_str().unwrap_or("").to_lowercase();
        an.cmp(&bn)
    });
    entries
}

// Skills

async fn handle_skills_list(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let scope: Option<String> = params
        .get("scope")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let skills = state.persistence.list_skills(scope.as_deref())?;
    Ok(serde_json::to_value(skills)?)
}

async fn handle_skills_save(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let skill: crate::api::types::Skill = serde_json::from_value(params)?;
    let saved = state.persistence.save_skill(&skill)?;
    Ok(serde_json::to_value(saved)?)
}

// Settings with keyring integration for secrets - Phase 1 multi-provider

fn try_get_keyring(key_name: &str) -> Option<String> {
    if let Ok(entry) = keyring::Entry::new("com.cid.dev", key_name) {
        if let Ok(pw) = entry.get_password() {
            if !pw.trim().is_empty() {
                return Some(pw);
            }
        }
    }
    None
}

fn redact_key(key: &str) -> String {
    if key.len() > 8 {
        format!("{}...{}", &key[..4], &key[key.len() - 4..])
    } else {
        "***".to_string()
    }
}

async fn handle_settings_get(state: &AppState) -> anyhow::Result<serde_json::Value> {
    let mut settings = state.persistence.get_settings()?;

    // Migration path: try keyring if DB field is None for each provider
    if settings.anthropic_api_key.is_none() {
        if let Some(k) = try_get_keyring("anthropic_api_key") {
            settings.anthropic_api_key = Some(k);
        }
    }
    if settings.openai_api_key.is_none() {
        if let Some(k) = try_get_keyring("openai_api_key").or_else(|| try_get_keyring("openai")) {
            settings.openai_api_key = Some(k);
        }
    }
    if settings.google_api_key.is_none() {
        if let Some(k) =
            try_get_keyring("google_api_key").or_else(|| try_get_keyring("gemini_api_key"))
        {
            settings.google_api_key = Some(k);
        }
    }
    if settings.openai_compatible_api_key.is_none() {
        if let Some(k) = try_get_keyring("openai_compatible_api_key")
            .or_else(|| try_get_keyring("openrouter_api_key"))
        {
            settings.openai_compatible_api_key = Some(k);
        }
    }

    // Build safe version with redacted keys
    let mut safe_settings = settings.clone();
    if let Some(k) = &settings.anthropic_api_key {
        safe_settings.anthropic_api_key = Some(redact_key(k));
    }
    if let Some(k) = &settings.openai_api_key {
        safe_settings.openai_api_key = Some(redact_key(k));
    }
    if let Some(k) = &settings.google_api_key {
        safe_settings.google_api_key = Some(redact_key(k));
    }
    if let Some(k) = &settings.openai_compatible_api_key {
        safe_settings.openai_compatible_api_key = Some(redact_key(k));
    }
    if let Some(k) = &settings.github_token {
        safe_settings.github_token = Some(redact_key(k));
    }

    // Flattened so the client can read/round-trip fields directly (settings.foo, not
    // settings.settings.foo) — the `has_*_key` flags ride alongside as extra fields,
    // which settings.update's deserialize into `Settings` silently ignores. Secrets
    // are redacted above; nothing plaintext is ever included in this response.
    let mut result = serde_json::to_value(&safe_settings)?;
    if let serde_json::Value::Object(ref mut map) = result {
        map.insert(
            "has_anthropic_key".into(),
            serde_json::json!(settings.anthropic_api_key.is_some()),
        );
        map.insert(
            "has_openai_key".into(),
            serde_json::json!(settings.openai_api_key.is_some()),
        );
        map.insert(
            "has_google_key".into(),
            serde_json::json!(settings.google_api_key.is_some()),
        );
        map.insert(
            "has_openai_compatible_key".into(),
            serde_json::json!(
                settings.openai_compatible_api_key.is_some()
                    || settings.openai_compatible_endpoint.is_some()
            ),
        );
        map.insert(
            "has_github_token".into(),
            serde_json::json!(
                settings.github_token.is_some() || try_get_keyring("github_token").is_some()
            ),
        );
    }
    Ok(result)
}

async fn handle_settings_update(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let existing = state.persistence.get_settings()?;

    // Merge onto the persisted settings before deserializing, so a caller can send
    // just the fields it's changing (e.g. `{"theme": "dark"}`) rather than being
    // forced to round-trip every required field on every update.
    let mut merged = serde_json::to_value(&existing)?;
    if let (serde_json::Value::Object(target), serde_json::Value::Object(incoming)) =
        (&mut merged, &params)
    {
        for (k, v) in incoming {
            target.insert(k.clone(), v.clone());
        }
    }
    let mut settings: crate::api::types::Settings = serde_json::from_value(merged)?;

    // Helper: if incoming key is redacted (contains ...), keep existing; else if real, store in keyring and keep
    let handle_key = |incoming: &Option<String>,
                      existing_val: &Option<String>,
                      keyring_name: &str|
     -> Option<String> {
        match incoming {
            Some(k) if k.contains("...") || k == "***" => existing_val.clone(),
            Some(k) if !k.trim().is_empty() => {
                // Real key - store in keyring
                if let Ok(entry) = keyring::Entry::new("com.cid.dev", keyring_name) {
                    let _ = entry.set_password(k);
                }
                Some(k.clone())
            }
            _ => existing_val.clone(), // Keep existing if incoming is None/empty
        }
    };

    // Anthropic
    settings.anthropic_api_key = handle_key(
        &settings.anthropic_api_key,
        &existing.anthropic_api_key,
        "anthropic_api_key",
    );
    // OpenAI
    settings.openai_api_key = handle_key(
        &settings.openai_api_key,
        &existing.openai_api_key,
        "openai_api_key",
    );
    // Google
    settings.google_api_key = handle_key(
        &settings.google_api_key,
        &existing.google_api_key,
        "google_api_key",
    );
    // OpenAI Compatible
    settings.openai_compatible_api_key = handle_key(
        &settings.openai_compatible_api_key,
        &existing.openai_compatible_api_key,
        "openai_compatible_api_key",
    );
    // GitHub token (stored separately but also in settings fallback)
    settings.github_token = handle_key(
        &settings.github_token,
        &existing.github_token,
        "github_token",
    );

    // If endpoint is redacted? Endpoint not secret, keep as provided unless empty
    // For per-role fields, preserve existing if incoming is empty and we want to keep? But frontend should send full.

    let updated = state.persistence.update_settings(&settings)?;
    Ok(serde_json::to_value(updated)?)
}

async fn handle_model_list(state: &AppState) -> anyhow::Result<serde_json::Value> {
    let models = state.model_manager.list_models();
    Ok(serde_json::to_value(models)?)
}

async fn handle_model_chat(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let mission_id: String =
        serde_json::from_value(params.get("mission_id").cloned().unwrap_or_default())?;
    let content: String =
        serde_json::from_value(params.get("content").cloned().unwrap_or_default())?;
    state
        .model_manager
        .process_message(&mission_id, &content, state.clone())
        .await?;
    Ok(serde_json::json!({ "ok": true }))
}

// ---------------------------------------------------------------------------
// Local Runtime Detection (Phase 1)
// ---------------------------------------------------------------------------

/// List all local runtimes (Ollama, LM Studio, llama.cpp) with their models.
/// Stateless detection, always probes live endpoints with 2s timeout.
async fn handle_local_runtime_list(_state: &AppState) -> anyhow::Result<serde_json::Value> {
    let detector = LocalRuntimeDetector::new();
    let runtimes = detector.detect_all().await;
    Ok(serde_json::to_value(runtimes)?)
}

/// Detect local runtimes with optional force_refresh param.
/// For Phase 1, detection is always fresh (no caching) since it's stateless.
/// The force_refresh param is accepted for API compatibility and future caching.
async fn handle_local_runtime_detect(
    params: serde_json::Value,
    _state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    // Parse optional force_refresh, ignore for now but log
    if let Ok(p) = serde_json::from_value::<LocalRuntimeDetectParams>(params.clone()) {
        if p.force_refresh.unwrap_or(false) {
            info!("local.runtime.detect called with force_refresh=true");
        } else {
            debug!("local.runtime.detect called");
        }
    } else {
        debug!("local.runtime.detect called with no/invalid params, proceeding anyway");
    }

    let detector = LocalRuntimeDetector::new();
    let runtimes = detector.detect_all().await;
    Ok(serde_json::to_value(runtimes)?)
}

// ---------------------------------------------------------------------------
// GitHub Bridge (Phase 1)
// ---------------------------------------------------------------------------

async fn handle_github_connect(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    // Use typed params if possible, fallback to manual
    let p: crate::api::types::GitHubConnectParams = match serde_json::from_value(params.clone()) {
        Ok(v) => v,
        Err(_) => {
            let repo_path = params
                .get("repo_path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let owner = params
                .get("owner")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let repo = params
                .get("repo")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let token = params
                .get("token")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            crate::api::types::GitHubConnectParams {
                repo_path,
                owner,
                repo,
                token,
            }
        }
    };

    let cfg = state
        .github_manager
        .connect(&p.repo_path, &p.owner, &p.repo, p.token)
        .await?;
    Ok(serde_json::to_value(cfg)?)
}

async fn handle_github_config_get(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = params
        .get("repo_path")
        .or_else(|| params.get("path"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("repo_path is required"))?
        .to_string();

    match state.github_manager.get_config(&repo_path)? {
        Some(cfg) => Ok(serde_json::to_value(cfg)?),
        None => Ok(
            serde_json::json!({ "connected": false, "repo_path": repo_path, "has_token": false }),
        ),
    }
}

async fn handle_github_issues_list(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = params
        .get("repo_path")
        .or_else(|| params.get("path"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("repo_path required"))?
        .to_string();
    let st = params
        .get("state")
        .and_then(|v| v.as_str())
        .unwrap_or("open")
        .to_string();

    let issues = state.github_manager.list_issues(&repo_path, &st).await?;
    Ok(serde_json::to_value(issues)?)
}

async fn handle_github_issue_get(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = params
        .get("repo_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("repo_path required"))?
        .to_string();
    let number = params
        .get("issue_number")
        .or_else(|| params.get("number"))
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow::anyhow!("issue_number required"))?;

    let issue = state.github_manager.get_issue(&repo_path, number).await?;
    Ok(serde_json::to_value(issue)?)
}

async fn handle_github_issue_to_mission(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    // Try typed params first
    if let Ok(p) =
        serde_json::from_value::<crate::api::types::GitHubIssueToMissionParams>(params.clone())
    {
        let mission = state
            .github_manager
            .issue_to_mission(&p.repo_path, p.issue_number, p.session_mode)
            .await?;
        return Ok(serde_json::to_value(mission)?);
    }

    // Manual fallback
    let repo_path = params
        .get("repo_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("repo_path required"))?
        .to_string();
    let issue_number = params
        .get("issue_number")
        .or_else(|| params.get("number"))
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow::anyhow!("issue_number required"))?;
    let session_mode: Option<crate::api::types::SessionMode> = params
        .get("session_mode")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    let mission = state
        .github_manager
        .issue_to_mission(&repo_path, issue_number, session_mode)
        .await?;
    Ok(serde_json::to_value(mission)?)
}

async fn handle_github_pr_list(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = params
        .get("repo_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("repo_path required"))?
        .to_string();
    let prs = state.github_manager.list_prs(&repo_path).await?;
    Ok(serde_json::to_value(prs)?)
}

async fn handle_github_pr_create(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: crate::api::types::GitHubPrCreateParams = match serde_json::from_value(params.clone()) {
        Ok(v) => v,
        Err(_) => {
            let repo_path = params
                .get("repo_path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let title = params
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let body = params
                .get("body")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let head_branch = params
                .get("head_branch")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let base_branch = params
                .get("base_branch")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            crate::api::types::GitHubPrCreateParams {
                repo_path,
                title,
                body,
                head_branch,
                base_branch,
            }
        }
    };

    // Governance gate (Part 14): opening a PR is a merge decision, gated by
    // `can_merge` the same way `mission.create`'s Autonomous-mode branch
    // already gates that decision. Only enforced once a Workspace has
    // actually bootstrapped auth (an Owner account exists) — matching
    // `handle_auth_register`'s own conditional pattern — so the default
    // single-user, no-auth-configured golden path (Flow 1 step 7: "Merge or
    // open PR") is unaffected; a team that opts into multi-user governance
    // gets it enforced here without a caller being able to bypass it by
    // going through this RPC instead of some other merge path.
    // review_prompt.md §1.3: this RPC existed and was tested, but nothing
    // called it — found and fixed by wiring it here.
    if state.auth_manager.is_bootstrapped()? {
        if let Ok(repo) = state.persistence.get_repo_channel_by_path(&p.repo_path) {
            let actor = require_session(&params, state)?;
            let decision = state
                .governance_manager
                .can_merge(&actor, &repo.workspace_id);
            if !decision.allowed() {
                anyhow::bail!("{}", decision.reason());
            }
        }
    }

    let pr = state
        .github_manager
        .create_pr(
            &p.repo_path,
            &p.title,
            p.body.as_deref(),
            &p.head_branch,
            p.base_branch.as_deref(),
        )
        .await?;
    // Emit notification for PR created (sync back into Mission thread if needed)
    let notif = crate::api::types::JsonRpcNotification {
        jsonrpc: "2.0".to_string(),
        method: "github.pr.created".to_string(),
        params: serde_json::json!({ "repo_path": p.repo_path, "pr": pr }),
    };
    if let Ok(s) = serde_json::to_string(&notif) {
        let _ = state.event_tx.send(s);
    }

    Ok(serde_json::to_value(pr)?)
}

async fn handle_github_pr_status(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = params
        .get("repo_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("repo_path required"))?
        .to_string();
    let pr_number = params
        .get("pr_number")
        .or_else(|| params.get("number"))
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow::anyhow!("pr_number required"))?;

    let pr = state
        .github_manager
        .get_pr_status(&repo_path, pr_number)
        .await?;
    Ok(serde_json::to_value(pr)?)
}

// ============ Context Engine Handlers (Phase 1) ============

async fn handle_context_engine_status(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = params
        .get("repo_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("repo_path required"))?
        .to_string();
    let status = state.context_engine_manager.status(&repo_path);
    Ok(serde_json::to_value(status)?)
}

async fn handle_context_engine_enable(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = params
        .get("repo_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("repo_path required"))?
        .to_string();
    let status = state.context_engine_manager.enable_for_repo(&repo_path)?;
    Ok(serde_json::to_value(status)?)
}

async fn handle_context_engine_disable(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = params
        .get("repo_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("repo_path required"))?
        .to_string();
    state.context_engine_manager.disable_for_repo(&repo_path)?;
    Ok(serde_json::json!({ "status": "disabled", "repo_path": repo_path }))
}

async fn handle_context_engine_search(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = params
        .get("repo_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("repo_path required"))?
        .to_string();
    let query = params
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("query required"))?
        .to_string();
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
    let results = state
        .context_engine_manager
        .search(&query, &repo_path, limit);
    Ok(serde_json::json!({
        "query": query,
        "results": results,
        "total": results.len(),
    }))
}

async fn handle_context_engine_related(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let file_path = params
        .get("file_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("file_path required"))?
        .to_string();
    let related = state.context_engine_manager.get_related_files(&file_path);
    Ok(serde_json::json!({
        "file_path": file_path,
        "related_files": related,
        "total": related.len(),
    }))
}

async fn handle_context_engine_file_index(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let file_path = params
        .get("file_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("file_path required"))?
        .to_string();
    match state.context_engine_manager.get_file_index(&file_path) {
        Some(idx) => Ok(serde_json::to_value(idx)?),
        None => Ok(serde_json::json!({ "error": "File not indexed", "file_path": file_path })),
    }
}

async fn handle_context_engine_recent(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = params
        .get("repo_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("repo_path required"))?
        .to_string();
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
    let recent = state
        .context_engine_manager
        .get_recently_touched(&repo_path, limit);
    Ok(serde_json::json!({
        "repo_path": repo_path,
        "recent_files": recent,
        "total": recent.len(),
    }))
}

// ============ Code Analysis Handlers (Phase 2) ============

async fn handle_code_analyze_file(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let file_path = params
        .get("file_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("file_path required"))?
        .to_string();

    let content = std::fs::read_to_string(&file_path)
        .map_err(|e| anyhow::anyhow!("Failed to read file: {}", e))?;

    let result = state.analyzer.analyze_file(&file_path, &content)?;
    Ok(serde_json::to_value(result)?)
}

async fn handle_code_analyze_directory(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let dir_path = params
        .get("dir_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("dir_path required"))?
        .to_string();

    let result = state.analyzer.analyze_directory(&dir_path)?;
    Ok(serde_json::to_value(result)?)
}

async fn handle_code_search_symbols(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let dir_path = params
        .get("dir_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("dir_path required"))?
        .to_string();
    let query = params
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("query required"))?
        .to_string();

    let files = state.analyzer.analyze_directory(&dir_path)?;
    let mut matching: Vec<serde_json::Value> = Vec::new();

    for file in &files {
        for symbol in &file.symbols {
            if symbol.name.to_lowercase().contains(&query.to_lowercase()) {
                matching.push(serde_json::json!({
                    "symbol": symbol,
                    "file_path": file.path,
                    "language": file.language,
                }));
            }
        }
    }

    Ok(serde_json::json!({
        "query": query,
        "results": matching,
        "total": matching.len(),
    }))
}

async fn handle_code_get_imports(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let file_path = params
        .get("file_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("file_path required"))?
        .to_string();

    let content = std::fs::read_to_string(&file_path)?;
    let result = state.analyzer.analyze_file(&file_path, &content)?;

    Ok(serde_json::json!({
        "file_path": file_path,
        "imports": result.imports,
        "symbols": result.symbols,
    }))
}

// ============ Autonomy Handlers (Phase 1) ============

async fn handle_autonomy_allowlist_get(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let scope_id = params
        .get("scope_id")
        .or_else(|| params.get("repo_path"))
        .or_else(|| params.get("repo_channel_id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("scope_id or repo_path required"))?
        .to_string();

    match state.autonomy_manager.get_allowlist(&scope_id) {
        Some(al) => Ok(serde_json::to_value(al)?),
        None => Ok(serde_json::json!({ "exists": false, "scope_id": scope_id })),
    }
}

async fn handle_autonomy_allowlist_set(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: AutonomyAllowlistUpdateParams = serde_json::from_value(params)?;

    let al = state.autonomy_manager.set_allowlist(
        &p.scope_id,
        p.allowed_commands,
        p.allowed_paths,
        p.denied_paths,
        None,
    );
    Ok(serde_json::to_value(al)?)
}

async fn handle_autonomy_allowlist_remove(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let scope_id = params
        .get("scope_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("scope_id required"))?
        .to_string();

    let removed = state.autonomy_manager.remove_allowlist(&scope_id);
    Ok(serde_json::json!({ "removed": removed, "scope_id": scope_id }))
}

async fn handle_autonomy_allowlist_list(state: &AppState) -> anyhow::Result<serde_json::Value> {
    let allowlists = state.autonomy_manager.list_allowlists();
    Ok(serde_json::to_value(allowlists)?)
}

async fn handle_autonomy_allowlist_default(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let scope_id = params
        .get("scope_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("scope_id required"))?
        .to_string();

    let al = state.autonomy_manager.create_default_allowlist(&scope_id);
    Ok(serde_json::to_value(al)?)
}

async fn handle_autonomy_command_check(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: AutonomyCheckParams = serde_json::from_value(params)?;
    let result = state
        .autonomy_manager
        .check_command(&p.repo_path, &p.command, None);
    Ok(serde_json::to_value(result)?)
}

async fn handle_autonomy_budget_check(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let scope_id = params
        .get("scope_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("scope_id required"))?
        .to_string();
    let current = params
        .get("current_tool_calls")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;

    let result = state.autonomy_manager.check_budget(&scope_id, current);
    Ok(serde_json::to_value(result)?)
}

// ============ Phase 2: Background Model Router Handlers ============

async fn handle_background_model_status(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_channel_id = params
        .get("repo_channel_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("repo_channel_id required"))?
        .to_string();
    let status = state
        .background_model_router
        .get_status(&repo_channel_id)
        .await;
    Ok(serde_json::to_value(status)?)
}

async fn handle_background_model_configure(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let config: BackgroundModelConfig = serde_json::from_value(params.clone())?;
    let repo_id = config.repo_channel_id.clone();
    let updated = state
        .background_model_router
        .configure(&repo_id, config)
        .await;
    Ok(serde_json::to_value(updated)?)
}

async fn handle_background_model_submit_task(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: BackgroundTaskSubmitParams = serde_json::from_value(params)?;
    let task = state.background_model_router.submit_task(p).await?;
    Ok(serde_json::to_value(task)?)
}

async fn handle_background_model_list_tasks(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_channel_id = params.get("repo_channel_id").and_then(|v| v.as_str());
    let tasks = state
        .background_model_router
        .list_tasks(repo_channel_id)
        .await;
    Ok(serde_json::to_value(tasks)?)
}

// ============ Phase 2: Subagent Orchestrator Handlers ============

async fn handle_subagent_spawn(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: SubagentSpawnParams = serde_json::from_value(params)?;
    let subagent = state.subagent_orchestrator.spawn(p, state.clone()).await?;
    Ok(serde_json::to_value(subagent)?)
}

async fn handle_subagent_list(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: SubagentListParams = serde_json::from_value(params)?;
    let list = state.subagent_orchestrator.list(p).await;
    Ok(serde_json::to_value(list)?)
}

async fn handle_subagent_get(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let id: String =
        serde_json::from_value(params.get("subagent_id").cloned().unwrap_or_default())?;
    match state.subagent_orchestrator.get(&id).await {
        Some(sa) => Ok(serde_json::to_value(sa)?),
        None => Err(anyhow::anyhow!("Subagent not found: {}", id)),
    }
}

async fn handle_subagent_cancel(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: SubagentCancelParams = serde_json::from_value(params)?;
    let result = state.subagent_orchestrator.cancel(p).await?;
    Ok(serde_json::to_value(result)?)
}

// ============ Phase 2: Slack Bridge Handlers ============

async fn handle_slack_configure(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let workspace_id = params
        .get("workspace_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("workspace_id required"))?
        .to_string();
    let config: SlackConfig = serde_json::from_value(params)?;
    let saved = state.slack_bridge.configure(&workspace_id, config).await?;
    Ok(serde_json::to_value(saved)?)
}

async fn handle_slack_config_get(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let workspace_id = params
        .get("workspace_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("workspace_id required"))?
        .to_string();
    match state.slack_bridge.get_config(&workspace_id).await {
        Some(cfg) => Ok(serde_json::to_value(cfg)?),
        None => Ok(serde_json::json!({ "configured": false, "workspace_id": workspace_id })),
    }
}

async fn handle_slack_trigger_mission(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: SlackTriggerParams = serde_json::from_value(params)?;
    let trigger = state.slack_bridge.trigger_mission(p).await?;
    Ok(serde_json::to_value(trigger)?)
}

// ============ Phase 2: Teams Bridge Handlers ============

async fn handle_teams_configure(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let workspace_id = params
        .get("workspace_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("workspace_id required"))?
        .to_string();
    let config: TeamsConfig = serde_json::from_value(params)?;
    let saved = state.teams_bridge.configure(&workspace_id, config).await?;
    Ok(serde_json::to_value(saved)?)
}

async fn handle_teams_config_get(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let workspace_id = params
        .get("workspace_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("workspace_id required"))?
        .to_string();
    match state.teams_bridge.get_config(&workspace_id).await {
        Some(cfg) => Ok(serde_json::to_value(cfg)?),
        None => Ok(serde_json::json!({ "configured": false, "workspace_id": workspace_id })),
    }
}

async fn handle_teams_trigger_mission(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: TeamsTriggerParams = serde_json::from_value(params)?;
    let result = state.teams_bridge.trigger_mission(p).await?;
    Ok(result)
}

// ============ Phase 2: MCP Tasks Handlers ============

async fn handle_mcp_task_create(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: McpTaskCreateParams = serde_json::from_value(params)?;
    let handle = state.mcp_tasks_manager.create_task(p).await?;
    Ok(serde_json::to_value(handle)?)
}

async fn handle_mcp_task_poll(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let task_id = params
        .get("task_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("task_id required"))?
        .to_string();
    match state.mcp_tasks_manager.poll(&task_id).await {
        Some(handle) => Ok(serde_json::to_value(handle)?),
        None => Err(anyhow::anyhow!("Task not found: {}", task_id)),
    }
}

async fn handle_mcp_task_subscribe(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let task_id = params
        .get("task_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("task_id required"))?
        .to_string();
    match state.mcp_tasks_manager.subscribe(&task_id).await {
        Some(handle) => Ok(serde_json::to_value(handle)?),
        None => Err(anyhow::anyhow!("Task not found: {}", task_id)),
    }
}

async fn handle_mcp_task_cancel(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let task_id = params
        .get("task_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("task_id required"))?
        .to_string();
    let result = state.mcp_tasks_manager.cancel_task(&task_id).await?;
    Ok(serde_json::to_value(result)?)
}

async fn handle_mcp_task_list(state: &AppState) -> anyhow::Result<serde_json::Value> {
    let tasks = state.mcp_tasks_manager.list_tasks().await;
    Ok(serde_json::to_value(tasks)?)
}

// ============ Phase 2: Semantic Engine Handlers ============

async fn handle_semantic_engine_status(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = params
        .get("repo_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("repo_path required"))?
        .to_string();
    let status = state.semantic_engine.status(&repo_path).await;
    Ok(serde_json::to_value(status)?)
}

async fn handle_semantic_engine_enable(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: SemanticEnableParams = serde_json::from_value(params)?;
    let status = state.semantic_engine.enable(&p.repo_path).await?;
    Ok(serde_json::to_value(status)?)
}

async fn handle_semantic_engine_disable(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = params
        .get("repo_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("repo_path required"))?
        .to_string();
    state.semantic_engine.disable(&repo_path).await?;
    Ok(serde_json::json!({ "status": "disabled", "repo_path": repo_path }))
}

async fn handle_semantic_engine_search(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: SemanticSearchParams = serde_json::from_value(params)?;
    let results = state
        .semantic_engine
        .search(
            &p.query,
            &p.repo_path,
            p.limit.unwrap_or(20),
            p.include_dependencies.unwrap_or(false),
            p.include_blame.unwrap_or(false),
        )
        .await;
    Ok(serde_json::json!({
        "repo_path": p.repo_path,
        "query": p.query,
        "results": results,
        "total": results.len(),
    }))
}

async fn handle_semantic_engine_dependency_graph(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: SemanticDependencyParams = serde_json::from_value(params)?;
    let (nodes, edges) = state
        .semantic_engine
        .dependency_graph(
            &p.repo_path,
            p.file_path.as_deref(),
            p.symbol_name.as_deref(),
            p.depth,
        )
        .await?;
    Ok(serde_json::json!({
        "repo_path": p.repo_path,
        "nodes": nodes,
        "edges": edges,
    }))
}

async fn handle_semantic_engine_git_blame(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: SemanticGitBlameParams = serde_json::from_value(params)?;
    let blames = state
        .semantic_engine
        .git_blame(&p.repo_path, &p.file_path, p.line)
        .await;
    Ok(serde_json::to_value(blames)?)
}

async fn handle_semantic_engine_index_file(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = params
        .get("repo_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("repo_path required"))?
        .to_string();
    let file_path = params
        .get("file_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("file_path required"))?
        .to_string();
    let content = params
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("content required"))?
        .to_string();
    state
        .semantic_engine
        .index_file(&repo_path, &file_path, &content)
        .await?;
    Ok(serde_json::json!({ "indexed": true, "file_path": file_path }))
}

async fn handle_semantic_engine_load_blame(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = params
        .get("repo_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("repo_path required"))?
        .to_string();
    let file_path = params
        .get("file_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("file_path required"))?
        .to_string();
    let blames: Vec<crate::api::types::GitBlameInfo> =
        serde_json::from_value(params.get("blames").cloned().unwrap_or_default())?;
    state
        .semantic_engine
        .load_git_blame(&repo_path, &file_path, blames)
        .await;
    Ok(serde_json::json!({ "loaded": true, "file_path": file_path }))
}

// ============ Phase 4: Test-impact and documentation graphs ============

async fn handle_test_impact_for_symbol(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = required_str(&params, "repo_path")?;
    let symbol = required_str(&params, "symbol")?;
    let tests = state
        .semantic_engine
        .tests_for_symbol(&repo_path, &symbol)
        .await;
    Ok(serde_json::json!({ "symbol": symbol, "covering_tests": tests }))
}

async fn handle_test_impact_for_symbols(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = required_str(&params, "repo_path")?;
    let symbols: Vec<String> =
        serde_json::from_value(params.get("symbols").cloned().unwrap_or_default())
            .unwrap_or_default();
    let tests = state
        .semantic_engine
        .tests_for_symbols(&repo_path, &symbols)
        .await;
    Ok(serde_json::json!({ "symbols": symbols, "covering_tests": tests }))
}

async fn handle_test_impact_entries(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = required_str(&params, "repo_path")?;
    Ok(serde_json::to_value(
        state.semantic_engine.test_impact_entries(&repo_path).await,
    )?)
}

async fn handle_docs_for_symbol(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = required_str(&params, "repo_path")?;
    let symbol = required_str(&params, "symbol")?;
    let docs = state
        .semantic_engine
        .docs_for_symbol(&repo_path, &symbol)
        .await;
    Ok(serde_json::json!({ "symbol": symbol, "docs": docs }))
}

async fn handle_stale_docs(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = required_str(&params, "repo_path")?;
    Ok(serde_json::to_value(
        state.semantic_engine.stale_docs(&repo_path).await,
    )?)
}

// ============ Phase 2: Sandbox Handlers ============

async fn handle_sandbox_test(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let worktree_path: String = params
        .get("worktree_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("worktree_path required"))?
        .to_string();

    match state.sandbox_manager.boundary_report(&worktree_path) {
        Ok((passed, reason)) => {
            let test_result = SandboxTestResult {
                passed,
                reason,
                attempted_path: std::env::temp_dir().to_string_lossy().to_string(),
                blocked: passed,
            };
            Ok(serde_json::to_value(test_result)?)
        }
        Err(e) => {
            let test_result = SandboxTestResult {
                passed: false,
                reason: format!("Sandbox boundary probe could not run: {}", e),
                attempted_path: std::env::temp_dir().to_string_lossy().to_string(),
                blocked: false,
            };
            Ok(serde_json::to_value(test_result)?)
        }
    }
}

async fn handle_sandbox_status(state: &AppState) -> anyhow::Result<serde_json::Value> {
    let status = state.sandbox_manager.status();
    Ok(serde_json::to_value(status)?)
}

/// review_prompt.md / Gemini-checklist follow-up: the network allow-list
/// guard's live host list, so a Settings-style panel can show and edit it —
/// mirrors `autonomy.allowlist.get`'s shape for the command allow-list.
async fn handle_sandbox_network_allowlist_get(
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let hosts = state.sandbox_manager.network_allow_list().await;
    Ok(serde_json::json!({ "allowed_hosts": hosts }))
}

async fn handle_sandbox_network_allowlist_set(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let hosts: Vec<String> = serde_json::from_value(
        params
            .get("allowed_hosts")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("allowed_hosts is required"))?,
    )?;
    state.sandbox_manager.set_network_allow_list(hosts).await?;
    let hosts = state.sandbox_manager.network_allow_list().await;
    Ok(serde_json::json!({ "allowed_hosts": hosts }))
}

// ============ Configurable role profiles ============

async fn handle_role_profile_create(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let input: crate::role_profiles::RoleProfileInput = serde_json::from_value(params)?;
    Ok(serde_json::to_value(
        state.role_profile_manager.create(input)?,
    )?)
}

async fn handle_role_profile_update(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let id = required_str(&params, "id")?;
    let input: crate::role_profiles::RoleProfileInput =
        serde_json::from_value(params.get("profile").cloned().unwrap_or_default())?;
    Ok(serde_json::to_value(
        state.role_profile_manager.update(&id, input)?,
    )?)
}

async fn handle_role_profile_delete(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let id = required_str(&params, "id")?;
    state.role_profile_manager.delete(&id)?;
    Ok(serde_json::json!({ "ok": true }))
}

async fn handle_role_profile_get(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let id = required_str(&params, "id")?;
    Ok(serde_json::to_value(state.role_profile_manager.get(&id)?)?)
}

async fn handle_role_profile_list(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let workspace_id = required_str(&params, "workspace_id")?;
    let repo_channel_id = required_str(&params, "repo_channel_id")?;
    Ok(serde_json::to_value(
        state
            .role_profile_manager
            .list_for_repo(&workspace_id, &repo_channel_id)?,
    )?)
}

async fn handle_role_profile_check_permission(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let profile_id = required_str(&params, "profile_id")?;
    let tool_name = required_str(&params, "tool_name")?;
    let profile = state.role_profile_manager.get(&profile_id)?;
    let check = state
        .role_profile_manager
        .check_permission(&profile, &tool_name);
    Ok(match check {
        crate::role_profiles::PermissionCheck::Allowed => serde_json::json!({ "allowed": true }),
        crate::role_profiles::PermissionCheck::Denied { reason } => {
            serde_json::json!({ "allowed": false, "reason": reason })
        }
    })
}

// ============ Decisions view + deployment record ============

async fn handle_decisions_list(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = required_str(&params, "repo_path")?;
    let _ = state; // repo_path is filesystem-derived; no Core state needed
    Ok(serde_json::to_value(crate::decisions::list_adrs(
        &repo_path,
    ))?)
}

/// ADRs relevant to a specific Mission — matched against its task
/// description and plan content, per the module's "explicit reference wins"
/// rule (see `decisions::adrs_relevant_to_mission`).
async fn handle_decisions_for_mission(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let mission_id = required_str(&params, "mission_id")?;
    let mission = state.persistence.get_mission(&mission_id)?;
    let repo = state
        .persistence
        .get_repo_channel(&mission.repo_channel_id)?;

    let plan_text = state
        .role_runner
        .get_plan(&mission_id)?
        .map(|p| p.content)
        .unwrap_or_default();
    let texts = [mission.task_description.as_str(), plan_text.as_str()];

    Ok(serde_json::to_value(
        crate::decisions::adrs_relevant_to_mission(&repo.path, &texts),
    )?)
}

/// Manual deployment entry — the path the UI uses.
async fn handle_deployment_record(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let input: crate::decisions::DeploymentRecordInput = serde_json::from_value(params)?;
    let record = state
        .deployment_log
        .record(input, crate::decisions::DeploymentSource::Manual)?;
    broadcast_notification(state, "deployment.recorded", serde_json::to_value(&record)?);
    Ok(serde_json::to_value(record)?)
}

/// CI-triggered deployment entry — same shape, tagged with its real source so
/// the Mission thread can distinguish "someone typed this in" from "a real CI
/// run reported it."
async fn handle_deployment_webhook(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let input: crate::decisions::DeploymentRecordInput = serde_json::from_value(params)?;
    let record = state
        .deployment_log
        .record(input, crate::decisions::DeploymentSource::CiWebhook)?;
    broadcast_notification(state, "deployment.recorded", serde_json::to_value(&record)?);
    Ok(serde_json::to_value(record)?)
}

async fn handle_deployment_list(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let mission_id = required_str(&params, "mission_id")?;
    Ok(serde_json::to_value(
        state.deployment_log.for_mission(&mission_id)?,
    )?)
}

// ============ Confidence Engine ============

/// Score a patch and log the result against a Mission.
///
/// Callers can supply `new_content` directly (a patch not yet on disk) or
/// omit it and pass `target_file` alone, in which case the file's current
/// content on disk — as the Implementer already wrote it — is scored. The
/// latter is the common case: by the time a diff is ready for review, the
/// file already exists in the Mission's worktree.
async fn handle_confidence_score(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let mission_id = required_str(&params, "mission_id")?;
    let target_file = required_str(&params, "target_file")?;
    let diff = params
        .get("diff")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mission = state.persistence.get_mission(&mission_id)?;
    let repo = state
        .persistence
        .get_repo_channel(&mission.repo_channel_id)?;
    let root = mission
        .worktree_path
        .clone()
        .unwrap_or_else(|| repo.path.clone());

    let full_path = std::path::Path::new(&root).join(&target_file);
    let new_content = match params.get("new_content").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => std::fs::read_to_string(&full_path).map_err(|e| {
            anyhow::anyhow!(
                "No new_content supplied and could not read {} from the worktree: {}",
                full_path.display(),
                e
            )
        })?,
    };

    let patch = crate::confidence::Patch::from_content(
        uuid::Uuid::new_v4().to_string(),
        target_file.clone(),
        root.clone(),
        new_content,
        diff,
    );

    let card = state.confidence_engine.score_patch(&patch, &root)?;
    state
        .persistence
        .save_confidence_score(&mission_id, &target_file, &card)?;

    let summary = format!(
        "**Confidence**: {:.0}/100 for `{}` — {}",
        card.overall * 100.0,
        target_file,
        card.verdict()
    );
    state.persistence.create_message(
        &mission_id,
        crate::api::types::MessageRole::System,
        &summary,
        vec![],
    )?;

    broadcast_notification(state, "confidence.scored", serde_json::to_value(&card)?);
    Ok(serde_json::to_value(card)?)
}

async fn handle_confidence_history(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let mission_id = required_str(&params, "mission_id")?;
    Ok(serde_json::to_value(
        state.persistence.list_confidence_scores(&mission_id)?,
    )?)
}

// ============ GitLab / Bitbucket ============

fn forge_kind_of(params: &serde_json::Value) -> anyhow::Result<crate::forges::ForgeKind> {
    let raw = required_str(params, "kind")?;
    crate::forges::ForgeKind::parse(&raw)
        .ok_or_else(|| anyhow::anyhow!("Unknown forge '{raw}' — expected 'gitlab' or 'bitbucket'"))
}

async fn handle_forge_connect(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = required_str(&params, "repo_path")?;
    let kind = forge_kind_of(&params)?;
    let project = required_str(&params, "project")?;
    let base_url = params
        .get("base_url")
        .and_then(|v| v.as_str())
        .map(String::from);
    let token = params
        .get("token")
        .and_then(|v| v.as_str())
        .map(String::from);

    let config = state
        .forge_manager
        .connect(&repo_path, kind, &project, base_url, token)
        .await?;
    Ok(serde_json::to_value(config)?)
}

async fn handle_forge_config_get(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = required_str(&params, "repo_path")?;
    Ok(serde_json::to_value(
        state.forge_manager.get_config(&repo_path)?,
    )?)
}

async fn handle_forge_disconnect(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = required_str(&params, "repo_path")?;
    state.forge_manager.disconnect(&repo_path)?;
    Ok(serde_json::json!({ "ok": true }))
}

async fn handle_forge_issues_list(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = required_str(&params, "repo_path")?;
    let issue_state = params
        .get("state")
        .and_then(|v| v.as_str())
        .unwrap_or("opened");
    Ok(serde_json::to_value(
        state
            .forge_manager
            .list_issues(&repo_path, issue_state)
            .await?,
    )?)
}

async fn handle_forge_issue_get(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = required_str(&params, "repo_path")?;
    let number = params
        .get("number")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow::anyhow!("number is required"))?;
    Ok(serde_json::to_value(
        state.forge_manager.get_issue(&repo_path, number).await?,
    )?)
}

async fn handle_forge_issue_to_mission(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = required_str(&params, "repo_path")?;
    let number = params
        .get("issue_number")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow::anyhow!("issue_number is required"))?;
    let session_mode = params
        .get("session_mode")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let mission = state
        .forge_manager
        .issue_to_mission(&repo_path, number, session_mode)
        .await?;
    broadcast_notification(state, "mission.created", serde_json::to_value(&mission)?);
    Ok(serde_json::to_value(mission)?)
}

async fn handle_forge_cr_create(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = required_str(&params, "repo_path")?;
    let title = required_str(&params, "title")?;
    let source_branch = required_str(&params, "source_branch")?;
    let body = params.get("body").and_then(|v| v.as_str());
    let target = params.get("target_branch").and_then(|v| v.as_str());
    let cr = state
        .forge_manager
        .create_change_request(&repo_path, &title, body, &source_branch, target)
        .await?;
    broadcast_notification(
        state,
        "forge.change_request.created",
        serde_json::to_value(&cr)?,
    );
    Ok(serde_json::to_value(cr)?)
}

async fn handle_forge_cr_list(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = required_str(&params, "repo_path")?;
    Ok(serde_json::to_value(
        state.forge_manager.list_change_requests(&repo_path).await?,
    )?)
}

async fn handle_forge_cr_status(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = required_str(&params, "repo_path")?;
    let number = params
        .get("number")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow::anyhow!("number is required"))?;
    Ok(serde_json::to_value(
        state
            .forge_manager
            .change_request_status(&repo_path, number)
            .await?,
    )?)
}

// ============ Jira / Linear ============

fn tracker_of(params: &serde_json::Value) -> anyhow::Result<crate::trackers::Tracker> {
    let raw = required_str(params, "tracker")?;
    crate::trackers::Tracker::parse(&raw)
        .ok_or_else(|| anyhow::anyhow!("Unknown tracker '{raw}' — expected 'jira' or 'linear'"))
}

fn tracker_config_of(params: &serde_json::Value) -> anyhow::Result<crate::trackers::TrackerConfig> {
    Ok(crate::trackers::TrackerConfig {
        tracker: tracker_of(params)?,
        site_url: params
            .get("site_url")
            .and_then(|v| v.as_str())
            .map(String::from),
        email: params
            .get("email")
            .and_then(|v| v.as_str())
            .map(String::from),
    })
}

async fn handle_tracker_token_set(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let tracker = tracker_of(&params)?;
    let token = required_str(&params, "token")?;
    // Live check before persisting anything — matches the forge pattern
    // (GitHubManager::connect's GET /user). Jira/Linear used to store
    // whatever was typed with no verification at all.
    let config = tracker_config_of(&params)?;
    state
        .tracker_manager
        .verify_credentials(&config, &token)
        .await?;
    state.tracker_manager.store_token(tracker, &token)?;
    Ok(serde_json::json!({ "tracker": tracker.as_str(), "stored": true, "verified": true }))
}

async fn handle_tracker_status(state: &AppState) -> anyhow::Result<serde_json::Value> {
    Ok(serde_json::json!({
        "jira": { "has_token": state.tracker_manager.has_token(crate::trackers::Tracker::Jira) },
        "linear": { "has_token": state.tracker_manager.has_token(crate::trackers::Tracker::Linear) },
    }))
}

async fn handle_tracker_issue_get(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let config = tracker_config_of(&params)?;
    let key = required_str(&params, "issue_key")?;
    Ok(serde_json::to_value(
        state.tracker_manager.fetch_issue(&config, &key).await?,
    )?)
}

async fn handle_tracker_link(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let mission_id = required_str(&params, "mission_id")?;
    let tracker = tracker_of(&params)?;
    let key = required_str(&params, "issue_key")?;
    // The config is optional: without it the link is still recorded, just
    // without a fetched title.
    let config = tracker_config_of(&params).ok();
    let link = state
        .tracker_manager
        .link(&mission_id, tracker, &key, config.as_ref())
        .await?;
    broadcast_notification(state, "tracker.link.changed", serde_json::to_value(&link)?);
    Ok(serde_json::to_value(link)?)
}

async fn handle_tracker_links_list(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let mission_id = required_str(&params, "mission_id")?;
    Ok(serde_json::to_value(
        state.tracker_manager.links_for_mission(&mission_id)?,
    )?)
}

async fn handle_tracker_unlink(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let link_id = required_str(&params, "link_id")?;
    state.tracker_manager.unlink(&link_id)?;
    Ok(serde_json::json!({ "ok": true }))
}

async fn handle_tracker_issue_to_mission(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = required_str(&params, "repo_path")?;
    let config = tracker_config_of(&params)?;
    let key = required_str(&params, "issue_key")?;
    let session_mode = params
        .get("session_mode")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let mission = state
        .tracker_manager
        .issue_to_mission(&repo_path, &config, &key, session_mode)
        .await?;
    broadcast_notification(state, "mission.created", serde_json::to_value(&mission)?);
    Ok(serde_json::to_value(mission)?)
}

async fn handle_tracker_comment(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let config = tracker_config_of(&params)?;
    let key = required_str(&params, "issue_key")?;
    let body = required_str(&params, "body")?;
    state.tracker_manager.comment(&config, &key, &body).await?;
    Ok(serde_json::json!({ "ok": true }))
}

// ============ Accounts, sessions, roles ============

/// Resolve the caller's session from a `session_token` param.
///
/// Every governance and user-administration call goes through this, so a
/// missing or expired token is refused in one place rather than per handler.
fn require_session(
    params: &serde_json::Value,
    state: &AppState,
) -> anyhow::Result<crate::auth::Session> {
    let token = required_str(params, "session_token")?;
    state
        .auth_manager
        .resolve_session(&token)?
        .ok_or_else(|| anyhow::anyhow!("Session is invalid or has expired; sign in again"))
}

async fn handle_auth_status(state: &AppState) -> anyhow::Result<serde_json::Value> {
    Ok(serde_json::json!({
        "bootstrapped": state.auth_manager.is_bootstrapped()?,
        "session_ttl_hours": crate::auth::SESSION_TTL_HOURS,
    }))
}

async fn handle_auth_register(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let username = required_str(&params, "username")?;
    let password = required_str(&params, "password")?;
    let requested_role = params
        .get("role")
        .and_then(|v| v.as_str())
        .and_then(crate::auth::Role::parse);

    // Once an Owner exists, creating accounts is an administrative act. The
    // very first registration is open, because there is nobody to authorize it.
    let role = if state.auth_manager.is_bootstrapped()? {
        let actor = require_session(&params, state)?;
        crate::auth::require(&actor, crate::auth::Role::Admin, "create accounts")?;
        requested_role
    } else {
        None
    };

    let user = state.auth_manager.register(&username, &password, role)?;
    Ok(serde_json::to_value(user)?)
}

async fn handle_auth_login(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let username = required_str(&params, "username")?;
    let password = required_str(&params, "password")?;
    let session = state.auth_manager.login(&username, &password)?;
    Ok(serde_json::to_value(session)?)
}

async fn handle_auth_logout(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let token = required_str(&params, "session_token")?;
    state.auth_manager.logout(&token)?;
    Ok(serde_json::json!({ "ok": true }))
}

async fn handle_auth_session(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let token = required_str(&params, "session_token")?;
    Ok(serde_json::to_value(
        state.auth_manager.resolve_session(&token)?,
    )?)
}

async fn handle_auth_users_list(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let actor = require_session(&params, state)?;
    crate::auth::require(&actor, crate::auth::Role::Admin, "list accounts")?;
    Ok(serde_json::to_value(state.auth_manager.list_users()?)?)
}

async fn handle_auth_set_role(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let actor = require_session(&params, state)?;
    let user_id = required_str(&params, "user_id")?;
    let role = crate::auth::Role::parse(&required_str(&params, "role")?)
        .ok_or_else(|| anyhow::anyhow!("Unknown role"))?;
    Ok(serde_json::to_value(
        state.auth_manager.set_role(&actor, &user_id, role)?,
    )?)
}

async fn handle_auth_set_active(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let actor = require_session(&params, state)?;
    let user_id = required_str(&params, "user_id")?;
    let active = params
        .get("active")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| anyhow::anyhow!("active is required"))?;
    Ok(serde_json::to_value(
        state.auth_manager.set_active(&actor, &user_id, active)?,
    )?)
}

async fn handle_auth_change_password(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let actor = require_session(&params, state)?;
    let user_id = params
        .get("user_id")
        .and_then(|v| v.as_str())
        .unwrap_or(&actor.user_id)
        .to_string();
    let new_password = required_str(&params, "new_password")?;
    state
        .auth_manager
        .change_password(&actor, &user_id, &new_password)?;
    Ok(serde_json::json!({ "ok": true, "sessions_revoked": true }))
}

// ============ Workspace governance ============

fn workspace_id_of(params: &serde_json::Value) -> String {
    params
        .get("workspace_id")
        .and_then(|v| v.as_str())
        .unwrap_or("default")
        .to_string()
}

async fn handle_governance_policy_get(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let policy = state
        .governance_manager
        .get_policy(&workspace_id_of(&params));
    Ok(serde_json::to_value(policy)?)
}

async fn handle_governance_policy_set(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let actor = require_session(&params, state)?;
    let policy: crate::governance::WorkspacePolicy =
        serde_json::from_value(params.get("policy").cloned().unwrap_or_default())?;
    let saved = state.governance_manager.set_policy(&actor, policy)?;
    broadcast_notification(
        state,
        "governance.policy.changed",
        serde_json::to_value(&saved)?,
    );
    Ok(serde_json::to_value(saved)?)
}

async fn handle_governance_check_autonomous(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let actor = require_session(&params, state)?;
    let repo_path = required_str(&params, "repo_path")?;
    let decision = state.governance_manager.can_enable_autonomous(
        &actor,
        &workspace_id_of(&params),
        &repo_path,
    );
    Ok(serde_json::to_value(decision)?)
}

async fn handle_governance_check_plan(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let actor = require_session(&params, state)?;
    let decision = state
        .governance_manager
        .can_approve_plan(&actor, &workspace_id_of(&params));
    Ok(serde_json::to_value(decision)?)
}

async fn handle_governance_check_merge(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let actor = require_session(&params, state)?;
    let decision = state
        .governance_manager
        .can_merge(&actor, &workspace_id_of(&params));
    Ok(serde_json::to_value(decision)?)
}

async fn handle_governance_spend_check(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let mission_id = required_str(&params, "mission_id")?;
    let usd = params.get("usd").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let decision =
        state
            .governance_manager
            .check_spend(&workspace_id_of(&params), &mission_id, usd);
    Ok(serde_json::to_value(decision)?)
}

async fn handle_governance_spend_record(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let mission_id = required_str(&params, "mission_id")?;
    let usd = params
        .get("usd")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| anyhow::anyhow!("usd is required"))?;
    let note = params
        .get("note")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let record =
        state
            .governance_manager
            .record_spend(&workspace_id_of(&params), &mission_id, usd, note);
    Ok(serde_json::to_value(record)?)
}

async fn handle_governance_spend_summary(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let workspace_id = workspace_id_of(&params);
    let mission_id = params.get("mission_id").and_then(|v| v.as_str());
    Ok(serde_json::json!({
        "workspace_id": workspace_id,
        "workspace_spend_24h_usd": state.governance_manager.workspace_spend_24h(&workspace_id),
        "mission_spend_usd": mission_id.map(|m| state.governance_manager.mission_spend(m)),
        "records": state.governance_manager.spend_records(mission_id),
    }))
}

// ============ Planner / Reviewer ============

async fn handle_mission_plan_generate(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let mission_id = required_str(&params, "mission_id")?;
    let force = params
        .get("force")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let _ = state
        .persistence
        .update_mission_status(&mission_id, crate::api::types::MissionStatus::Planning);
    let plan = state.role_runner.generate_plan(&mission_id, force).await?;
    let _ = state.persistence.update_mission_status(
        &mission_id,
        crate::api::types::MissionStatus::BlockedOnApproval,
    );

    broadcast_notification(state, "mission.plan.changed", serde_json::to_value(&plan)?);
    Ok(serde_json::to_value(plan)?)
}

async fn handle_mission_plan_get(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let mission_id = required_str(&params, "mission_id")?;
    let plan = state.role_runner.get_plan(&mission_id)?;
    let gate = state.role_runner.implementer_is_gated(&mission_id)?;
    Ok(serde_json::json!({ "plan": plan, "implementer_blocked_reason": gate }))
}

async fn handle_mission_plan_update(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let mission_id = required_str(&params, "mission_id")?;
    let content = required_str(&params, "content")?;
    let plan = state.role_runner.update_plan(&mission_id, &content)?;
    broadcast_notification(state, "mission.plan.changed", serde_json::to_value(&plan)?);
    Ok(serde_json::to_value(plan)?)
}

async fn handle_mission_plan_approve(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let mission_id = required_str(&params, "mission_id")?;

    // When a session is supplied, the approver's role is checked against
    // Workspace policy and their username is recorded as the approver — an
    // audit trail needs a person, not a free-text string.
    let approved_by = if params.get("session_token").is_some() {
        let actor = require_session(&params, state)?;
        let mission = state.persistence.get_mission(&mission_id)?;
        let repo = state
            .persistence
            .get_repo_channel(&mission.repo_channel_id)?;
        let decision = state
            .governance_manager
            .can_approve_plan(&actor, &repo.workspace_id);
        if !decision.allowed() {
            anyhow::bail!("{}", decision.reason());
        }
        Some(actor.username)
    } else {
        params
            .get("approved_by")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    };

    let plan = state
        .role_runner
        .approve_plan(&mission_id, approved_by.as_deref())?;
    broadcast_notification(state, "mission.plan.changed", serde_json::to_value(&plan)?);
    Ok(serde_json::to_value(plan)?)
}

async fn handle_mission_plan_reject(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let mission_id = required_str(&params, "mission_id")?;
    let reason = params.get("reason").and_then(|v| v.as_str());
    let plan = state.role_runner.reject_plan(&mission_id, reason)?;
    broadcast_notification(state, "mission.plan.changed", serde_json::to_value(&plan)?);
    Ok(serde_json::to_value(plan)?)
}

/// Run the Reviewer over the Mission's current diff. The diff is read from the
/// Mission's own worktree so a review can't accidentally cover another Mission.
async fn handle_mission_review_run(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let mission_id = required_str(&params, "mission_id")?;
    let mission = state.persistence.get_mission(&mission_id)?;
    let path = match mission.worktree_path.clone() {
        Some(wt) => wt,
        None => {
            state
                .persistence
                .get_repo_channel(&mission.repo_channel_id)?
                .path
        }
    };

    let diff = match params.get("diff").and_then(|v| v.as_str()) {
        Some(d) => d.to_string(),
        None => state
            .git_manager
            .diff(&path, None)
            .map(|d| serde_json::to_string_pretty(&d).unwrap_or_default())
            .unwrap_or_default(),
    };

    let review = state.role_runner.run_review(&mission_id, &diff).await?;
    let _ = state
        .persistence
        .update_mission_status(&mission_id, crate::api::types::MissionStatus::Review);
    broadcast_notification(
        state,
        "mission.review.completed",
        serde_json::to_value(&review)?,
    );
    Ok(serde_json::to_value(review)?)
}

async fn handle_mission_review_get(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let mission_id = required_str(&params, "mission_id")?;
    let review = state.role_runner.latest_review(&mission_id)?;
    Ok(serde_json::to_value(review)?)
}

async fn handle_mission_review_list(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let mission_id = required_str(&params, "mission_id")?;
    let reviews = state.persistence.list_mission_reviews(&mission_id)?;
    Ok(serde_json::to_value(reviews)?)
}

async fn handle_mission_context_usage(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let mission_id = required_str(&params, "mission_id")?;
    let usage = state.model_manager.context_usage(&mission_id)?;
    Ok(serde_json::to_value(usage)?)
}

async fn handle_mission_context_compact(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let mission_id = required_str(&params, "mission_id")?;
    let digest = state.model_manager.compact_context_now(&mission_id)?;
    Ok(serde_json::json!({ "digest": digest }))
}

async fn handle_mission_checkpoint_list(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let mission_id = required_str(&params, "mission_id")?;
    let checkpoints = state.persistence.list_checkpoints(&mission_id)?;
    Ok(serde_json::to_value(checkpoints)?)
}

/// Rewinds a Mission's worktree to an earlier checkpoint — `git reset
/// --hard` to that checkpoint's commit, discarding everything after it in
/// this worktree. Requires an explicit `confirm: true`, not just a
/// checkpoint id, since this is exactly the class of hard-to-reverse action
/// that should never happen from a single accidental click.
async fn handle_mission_checkpoint_rewind(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let mission_id = required_str(&params, "mission_id")?;
    let checkpoint_id = required_str(&params, "checkpoint_id")?;
    let confirmed = params
        .get("confirm")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !confirmed {
        anyhow::bail!(
            "Rewinding discards every change made after this checkpoint in the Mission's \
             worktree. Pass confirm: true to proceed."
        );
    }

    let checkpoint = state.persistence.get_checkpoint(&checkpoint_id)?;
    if checkpoint.mission_id != mission_id {
        anyhow::bail!("Checkpoint {checkpoint_id} does not belong to Mission {mission_id}");
    }
    let mission = state.persistence.get_mission(&mission_id)?;
    let worktree = mission
        .worktree_path
        .ok_or_else(|| anyhow::anyhow!("Mission {mission_id} has no worktree to rewind"))?;

    state.git_manager.reset_hard(&worktree, &checkpoint.sha)?;

    broadcast_notification(
        state,
        "mission.checkpoint.rewound",
        serde_json::json!({ "mission_id": mission_id, "checkpoint": checkpoint }),
    );

    Ok(serde_json::to_value(checkpoint)?)
}

// ============ ACP host (Agent Client Protocol) ============

async fn handle_acp_editors_list(state: &AppState) -> anyhow::Result<serde_json::Value> {
    let editors = state.acp_manager.list_editors_async().await;
    Ok(serde_json::to_value(editors)?)
}

/// Hand a Mission's working directory off to an external ACP-capable editor.
/// The path comes from the Mission's worktree, falling back to its repo channel
/// for shared-clone Missions.
async fn handle_acp_handoff(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: crate::api::types::AcpHandoffParams = serde_json::from_value(params)?;
    let mission = state.persistence.get_mission(&p.mission_id)?;
    let path = match mission.worktree_path.clone() {
        Some(wt) => wt,
        None => {
            state
                .persistence
                .get_repo_channel(&mission.repo_channel_id)?
                .path
        }
    };
    let handoff = state
        .acp_manager
        .handoff(&p.mission_id, &p.editor_id, &path)
        .await?;
    broadcast_notification(
        state,
        "acp.handoff.changed",
        serde_json::to_value(&handoff)?,
    );
    Ok(serde_json::to_value(handoff)?)
}

async fn handle_acp_take_back(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let handoff_id = required_str(&params, "handoff_id")?;
    let handoff = state.acp_manager.take_back_async(&handoff_id).await?;
    broadcast_notification(
        state,
        "acp.handoff.changed",
        serde_json::to_value(&handoff)?,
    );
    Ok(serde_json::to_value(handoff)?)
}

async fn handle_acp_handoffs_list(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let handoffs = match params.get("mission_id").and_then(|v| v.as_str()) {
        Some(mission_id) => state.acp_manager.list_handoffs_for_mission(mission_id),
        None => state.acp_manager.list_handoffs(),
    };
    Ok(serde_json::to_value(handoffs)?)
}

async fn handle_acp_handoff_get(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let handoff_id = required_str(&params, "handoff_id")?;
    let handoff = state
        .acp_manager
        .get_handoff(&handoff_id)
        .ok_or_else(|| anyhow::anyhow!("Handoff not found: {}", handoff_id))?;
    Ok(serde_json::to_value(handoff)?)
}

async fn handle_acp_handoff_remove(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let handoff_id = required_str(&params, "handoff_id")?;
    state.acp_manager.remove_handoff(&handoff_id)?;
    Ok(serde_json::json!({ "removed": handoff_id }))
}

// ============ Skills: multi-file SKILL.md bundles and resolution ============

async fn handle_skills_bundles_list(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let p: crate::api::types::SkillBundleListParams = serde_json::from_value(params)?;
    let scope = match p.scope.as_deref() {
        Some("workspace") => crate::api::types::SkillScope::Workspace,
        _ => crate::api::types::SkillScope::Repo,
    };
    let dir = p
        .repo_path
        .ok_or_else(|| anyhow::anyhow!("repo_path is required"))?;
    let bundles = state.skills_manager.list_file_skills(&dir, scope);
    Ok(serde_json::to_value(bundles)?)
}

async fn handle_skills_bundle_write(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let path = required_str(&params, "path")?;
    let content = required_str(&params, "content")?;
    state.skills_manager.write_skill_md(&path, &content)?;
    Ok(serde_json::json!({ "path": path, "written": true }))
}

/// Return the resolved context stack for a repo — the Workspace → Repo → Mission
/// layering from Part 12, with each layer visible separately so the UI can show
/// which layer a given instruction came from.
async fn handle_skills_resolve(
    params: serde_json::Value,
    state: &AppState,
) -> anyhow::Result<serde_json::Value> {
    let repo_path = required_str(&params, "repo_path")?;
    let mission_context = params
        .get("mission_context")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let workspace_skills = state.persistence.list_skills(Some("workspace"))?;
    let repo_skills = state.persistence.list_skills(Some("repo"))?;
    let agents_md = state.skills_manager.detect_agents_md(&repo_path);
    let repo_bundles = state
        .skills_manager
        .list_file_skills(&repo_path, crate::api::types::SkillScope::Repo);

    let resolved = state.skills_manager.resolve_context(
        &workspace_skills,
        &repo_skills,
        agents_md.as_deref(),
        mission_context.as_deref(),
    );

    Ok(serde_json::json!({
        "resolved": resolved,
        "layers": {
            "workspace_skills": workspace_skills,
            "repo_skills": repo_skills,
            "repo_skill_bundles": repo_bundles,
            "agents_md": agents_md,
            "mission_context": mission_context,
        }
    }))
}
