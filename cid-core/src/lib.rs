pub mod access;
pub mod acp;
pub mod analyzer;
pub mod api;
pub mod auth;
pub mod autonomy;
pub mod background_model;
pub mod confidence;
pub mod context;
pub mod context_engine;
pub mod decisions;
pub mod forges;
pub mod git;
pub mod github;
pub mod governance;
pub mod local_models;
pub mod mcp;
pub mod mcp_tasks;
pub mod model;
pub mod net_guard;
pub mod observability;
pub mod path_confine;
pub mod persistence;
pub mod pty;
pub mod redact;
pub mod repo_health;
pub mod role_profiles;
pub mod roles;
pub mod sandbox;
pub mod semantic_engine;
pub mod skills;
pub mod slack_bridge;
pub mod subagent;
pub mod teams_bridge;
pub mod trackers;

use crate::{
    access::AccessPolicy,
    acp::AcpHostManager,
    analyzer::CodeAnalyzer,
    api::router::{create_router, AppState},
    auth::AuthManager,
    autonomy::AutonomyManager,
    background_model::BackgroundModelRouter,
    confidence::ConfidenceEngine,
    context::ContextManager,
    context_engine::ContextEngineManager,
    decisions::DeploymentLog,
    forges::ForgeManager,
    git::GitManager,
    github::GitHubManager,
    governance::GovernanceManager,
    mcp::McpManager,
    mcp_tasks::McpTasksManager,
    model::ModelManager,
    observability::{CrashLog, Metrics},
    persistence::Persistence,
    pty::PtyManager,
    role_profiles::RoleProfileManager,
    roles::RoleRunner,
    sandbox::SandboxManager,
    semantic_engine::SemanticEngine,
    skills::SkillsManager,
    slack_bridge::SlackBridge,
    subagent::SubagentOrchestrator,
    teams_bridge::TeamsBridge,
    trackers::TrackerManager,
};
use anyhow::Result;
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::broadcast;

pub struct Core {
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
    pub role_runner: Arc<RoleRunner>,
    pub auth_manager: Arc<AuthManager>,
    pub governance_manager: Arc<GovernanceManager>,
    pub forge_manager: Arc<ForgeManager>,
    pub tracker_manager: Arc<TrackerManager>,
    pub confidence_engine: Arc<ConfidenceEngine>,
    pub role_profile_manager: Arc<RoleProfileManager>,
    pub deployment_log: Arc<DeploymentLog>,
    pub autonomy_manager: Arc<AutonomyManager>,
    pub background_model_router: Arc<BackgroundModelRouter>,
    pub subagent_orchestrator: Arc<SubagentOrchestrator>,
    pub slack_bridge: Arc<SlackBridge>,
    pub teams_bridge: Arc<TeamsBridge>,
    pub mcp_tasks_manager: Arc<McpTasksManager>,
    pub sandbox_manager: Arc<SandboxManager>,
    pub semantic_engine: Arc<SemanticEngine>,
    pub access_policy: Arc<AccessPolicy>,
    pub connected_clients: Arc<std::sync::atomic::AtomicUsize>,
    pub event_tx: broadcast::Sender<String>,
    pub metrics: Arc<Metrics>,
    pub crash_log: Arc<CrashLog>,
}

impl Core {
    pub fn new(db_path: Option<std::path::PathBuf>) -> Result<Self> {
        let persistence = Arc::new(Persistence::new(db_path)?);
        let git_manager = Arc::new(GitManager::new());
        let pty_manager = Arc::new(PtyManager::new());
        let mcp_manager = Arc::new(McpManager::new());
        let model_manager = Arc::new(ModelManager::new(persistence.clone()));
        let context_manager = Arc::new(ContextManager::new());
        let context_engine_manager = Arc::new(ContextEngineManager::new());
        let acp_manager = Arc::new(AcpHostManager::new());
        let github_manager = Arc::new(GitHubManager::new(persistence.clone()));
        let analyzer = Arc::new(CodeAnalyzer::new());
        let skills_manager = Arc::new(SkillsManager::with_persistence(persistence.clone()));
        let autonomy_manager = Arc::new(AutonomyManager::new());
        let role_runner = Arc::new(RoleRunner::new(persistence.clone(), model_manager.clone()));
        let auth_manager = Arc::new(AuthManager::new(persistence.clone()));
        let governance_manager = Arc::new(GovernanceManager::new());
        let forge_manager = Arc::new(ForgeManager::new(persistence.clone()));
        let tracker_manager = Arc::new(TrackerManager::new(persistence.clone()));
        let role_profile_manager = Arc::new(RoleProfileManager::new(persistence.clone()));
        let deployment_log = Arc::new(DeploymentLog::new(persistence.clone()));
        let confidence_engine = Arc::new(ConfidenceEngine::new(
            analyzer.clone(),
            context_engine_manager.clone(),
        ));
        let (event_tx, _rx) = broadcast::channel(1000);

        let background_model_router = Arc::new(BackgroundModelRouter::new(event_tx.clone()));
        let subagent_orchestrator = Arc::new(SubagentOrchestrator::new(event_tx.clone()));
        let slack_bridge = Arc::new(SlackBridge::new(event_tx.clone()));
        let teams_bridge = Arc::new(TeamsBridge::new(event_tx.clone()));
        let mcp_tasks_manager =
            Arc::new(McpTasksManager::new(mcp_manager.clone(), event_tx.clone()));
        let semantic_engine = Arc::new(SemanticEngine::new(analyzer.clone()));
        let sandbox_manager = Arc::new(SandboxManager::new());
        let access_policy = Arc::new(AccessPolicy::local_only());
        let connected_clients = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let metrics = Arc::new(Metrics::new());
        let crash_log_path = dirs::data_dir().map(|mut p| {
            p.push("cid");
            std::fs::create_dir_all(&p).ok();
            p.push("crashes.jsonl");
            p
        });
        let crash_log = Arc::new(CrashLog::new(crash_log_path));

        // Load persisted MCP servers
        let persisted_servers = persistence.list_mcp_servers().unwrap_or_default();
        let mcp_clone = mcp_manager.clone();
        tokio::spawn(async move {
            mcp_clone.load_persisted(persisted_servers).await;
        });

        Ok(Self {
            persistence,
            git_manager,
            pty_manager,
            mcp_manager,
            model_manager,
            context_manager,
            context_engine_manager,
            acp_manager,
            github_manager,
            analyzer,
            skills_manager,
            role_runner,
            auth_manager,
            governance_manager,
            forge_manager,
            tracker_manager,
            role_profile_manager,
            deployment_log,
            confidence_engine,
            autonomy_manager,
            background_model_router,
            subagent_orchestrator,
            slack_bridge,
            teams_bridge,
            mcp_tasks_manager,
            semantic_engine,
            sandbox_manager,
            access_policy,
            connected_clients,
            event_tx,
            metrics,
            crash_log,
        })
    }

    pub fn new_in_memory() -> Result<Self> {
        let persistence = Arc::new(Persistence::new_in_memory()?);
        let git_manager = Arc::new(GitManager::new());
        let pty_manager = Arc::new(PtyManager::new());
        let mcp_manager = Arc::new(McpManager::new());
        let model_manager = Arc::new(ModelManager::new(persistence.clone()));
        let context_manager = Arc::new(ContextManager::new());
        let context_engine_manager = Arc::new(ContextEngineManager::new());
        let acp_manager = Arc::new(AcpHostManager::new());
        let github_manager = Arc::new(GitHubManager::new(persistence.clone()));
        let analyzer = Arc::new(CodeAnalyzer::new());
        let skills_manager = Arc::new(SkillsManager::with_persistence(persistence.clone()));
        let autonomy_manager = Arc::new(AutonomyManager::new());
        let role_runner = Arc::new(RoleRunner::new(persistence.clone(), model_manager.clone()));
        let auth_manager = Arc::new(AuthManager::new(persistence.clone()));
        let governance_manager = Arc::new(GovernanceManager::new());
        let forge_manager = Arc::new(ForgeManager::new(persistence.clone()));
        let tracker_manager = Arc::new(TrackerManager::new(persistence.clone()));
        let role_profile_manager = Arc::new(RoleProfileManager::new(persistence.clone()));
        let deployment_log = Arc::new(DeploymentLog::new(persistence.clone()));
        let confidence_engine = Arc::new(ConfidenceEngine::new(
            analyzer.clone(),
            context_engine_manager.clone(),
        ));
        let (event_tx, _rx) = broadcast::channel(1000);

        let background_model_router = Arc::new(BackgroundModelRouter::new(event_tx.clone()));
        let subagent_orchestrator = Arc::new(SubagentOrchestrator::new(event_tx.clone()));
        let slack_bridge = Arc::new(SlackBridge::new(event_tx.clone()));
        let teams_bridge = Arc::new(TeamsBridge::new(event_tx.clone()));
        let mcp_tasks_manager =
            Arc::new(McpTasksManager::new(mcp_manager.clone(), event_tx.clone()));
        let semantic_engine = Arc::new(SemanticEngine::new(analyzer.clone()));
        let sandbox_manager = Arc::new(SandboxManager::new());
        let access_policy = Arc::new(AccessPolicy::local_only());
        let connected_clients = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let metrics = Arc::new(Metrics::new());
        let crash_log = Arc::new(CrashLog::new(None));

        Ok(Self {
            persistence,
            git_manager,
            pty_manager,
            mcp_manager,
            model_manager,
            context_manager,
            context_engine_manager,
            acp_manager,
            github_manager,
            analyzer,
            skills_manager,
            role_runner,
            auth_manager,
            governance_manager,
            forge_manager,
            tracker_manager,
            role_profile_manager,
            deployment_log,
            confidence_engine,
            autonomy_manager,
            background_model_router,
            subagent_orchestrator,
            slack_bridge,
            teams_bridge,
            mcp_tasks_manager,
            semantic_engine,
            sandbox_manager,
            access_policy,
            connected_clients,
            event_tx,
            metrics,
            crash_log,
        })
    }

    pub fn app_state(&self) -> AppState {
        AppState {
            persistence: self.persistence.clone(),
            git_manager: self.git_manager.clone(),
            pty_manager: self.pty_manager.clone(),
            mcp_manager: self.mcp_manager.clone(),
            model_manager: self.model_manager.clone(),
            context_manager: self.context_manager.clone(),
            context_engine_manager: self.context_engine_manager.clone(),
            acp_manager: self.acp_manager.clone(),
            github_manager: self.github_manager.clone(),
            analyzer: self.analyzer.clone(),
            skills_manager: self.skills_manager.clone(),
            role_runner: self.role_runner.clone(),
            auth_manager: self.auth_manager.clone(),
            governance_manager: self.governance_manager.clone(),
            forge_manager: self.forge_manager.clone(),
            tracker_manager: self.tracker_manager.clone(),
            role_profile_manager: self.role_profile_manager.clone(),
            deployment_log: self.deployment_log.clone(),
            confidence_engine: self.confidence_engine.clone(),
            autonomy_manager: self.autonomy_manager.clone(),
            background_model_router: self.background_model_router.clone(),
            subagent_orchestrator: self.subagent_orchestrator.clone(),
            slack_bridge: self.slack_bridge.clone(),
            teams_bridge: self.teams_bridge.clone(),
            mcp_tasks_manager: self.mcp_tasks_manager.clone(),
            semantic_engine: self.semantic_engine.clone(),
            sandbox_manager: self.sandbox_manager.clone(),
            access_policy: self.access_policy.clone(),
            connected_clients: self.connected_clients.clone(),
            event_tx: self.event_tx.clone(),
            metrics: self.metrics.clone(),
            crash_log: self.crash_log.clone(),
        }
    }

    /// Replace the access policy. Called before `serve` when Core is bound to a
    /// non-loopback address, where a token is mandatory.
    pub fn set_access_policy(&mut self, policy: AccessPolicy) {
        self.access_policy = Arc::new(policy);
    }

    pub async fn serve(&self, addr: SocketAddr) -> Result<()> {
        let app = create_router(self.app_state());
        tracing::info!(
            "CID Core listening on {} (auth_required={}, loopback_only={})",
            addr,
            self.access_policy.requires_auth(),
            self.access_policy.is_loopback_only()
        );
        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app.into_make_service()).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn test_core_creation() {
        let core = Core::new_in_memory().unwrap();
        assert!(!core.persistence.list_workspaces().unwrap().is_empty());
    }
}
