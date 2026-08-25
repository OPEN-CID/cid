//! Phase 2 Subagent Orchestrator
//!
//! Session's Implementer can spawn ad hoc subagents for parallel/research subtasks.
//! Subagents SHARE the parent Session's worktree (do NOT create separate worktrees) —
//! and, as of the real-execution fix below, its tool-execution boundary too: a
//! subagent's tool calls go through the exact same `execute_tool_with_approval`
//! (autonomy gate, human-approval wait, per-path locking) as the main agent's own.
//!
//! Subagent roles: ResearchWorker, ParallelImpl, CodeExplorer, TestRunner.
//! Each subagent gets a prompt, tool permission set, and model config.
//! Max concurrent subagents configurable (default 5).
//! Subagent results reported back to parent Session thread via events.
//!
//! review_prompt.md / Gemini-checklist follow-up: `perform_subagent_work` used to
//! return a canned summary per role — no model call, no tool call, `files_changed`
//! always empty regardless of the prompt. It now runs a real, bounded tool-calling
//! turn (`ModelManager::run_subagent_turn`), so file-mutating subagents (`ParallelImpl`)
//! can genuinely touch the worktree — which is why `execute_tool_with_approval` gained
//! real per-path locking in the same pass: multiple subagents (or a subagent racing the
//! main Implementer) writing the same file concurrently is now a real scenario, not a
//! theoretical one.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{broadcast, RwLock, Semaphore};
use tracing::{info, warn};

use crate::api::router::AppState;
use crate::api::types::{
    new_id, now_utc, Subagent, SubagentCancelParams, SubagentListParams, SubagentResult,
    SubagentRole, SubagentSpawnParams, SubagentStatus,
};

const DEFAULT_MAX_CONCURRENT: usize = 5;

pub struct SubagentOrchestrator {
    subagents: Arc<RwLock<HashMap<String, Subagent>>>,
    session_subagent_map: RwLock<HashMap<String, Vec<String>>>,
    semaphore: Arc<Semaphore>,
    event_tx: broadcast::Sender<String>,
    max_concurrent: RwLock<usize>,
}

impl SubagentOrchestrator {
    pub fn new(event_tx: broadcast::Sender<String>) -> Self {
        Self {
            subagents: Arc::new(RwLock::new(HashMap::new())),
            session_subagent_map: RwLock::new(HashMap::new()),
            semaphore: Arc::new(Semaphore::new(DEFAULT_MAX_CONCURRENT)),
            event_tx,
            max_concurrent: RwLock::new(DEFAULT_MAX_CONCURRENT),
        }
    }

    pub async fn set_max_concurrent(&self, max: usize) {
        let mut guard = self.max_concurrent.write().await;
        *guard = max.max(1);
    }

    pub async fn get_max_concurrent(&self) -> usize {
        *self.max_concurrent.read().await
    }

    pub async fn spawn(
        &self,
        params: SubagentSpawnParams,
        app_state: AppState,
    ) -> anyhow::Result<Subagent> {
        let count = {
            let map = self.session_subagent_map.read().await;
            map.get(&params.session_id).map(|v| v.len()).unwrap_or(0)
        };

        let max = *self.max_concurrent.read().await;
        if count >= max {
            anyhow::bail!(
                "Max concurrent subagents ({}) reached for session {}",
                max,
                params.session_id
            );
        }

        let id = new_id();
        let role_permissions = default_tool_permissions(&params.role);
        let permissions = params.tool_permissions.unwrap_or(role_permissions);

        let subagent = Subagent {
            id: id.clone(),
            session_id: params.session_id.clone(),
            role: params.role,
            prompt: params.prompt.clone(),
            status: SubagentStatus::Pending,
            parent_agent_type: crate::api::types::AgentRole::Implementer,
            model_provider: params.model_provider,
            model_id: params.model_id,
            tool_permissions: permissions,
            result: None,
            created_at: now_utc(),
            completed_at: None,
        };

        {
            let mut guard = self.subagents.write().await;
            guard.insert(id.clone(), subagent.clone());
        }

        {
            let mut map = self.session_subagent_map.write().await;
            map.entry(params.session_id.clone())
                .or_default()
                .push(id.clone());
        }

        info!(
            "Spawning subagent {} ({:?}) for session {}",
            id, subagent.role, subagent.session_id
        );

        let subagent_clone = subagent.clone();
        let self_ref = SubagentOrchestratorRef {
            subagents: self.subagents.clone(),
            event_tx: self.event_tx.clone(),
            semaphore: self.semaphore.clone(),
            app_state,
        };
        tokio::spawn(async move {
            self_ref.execute_subagent(subagent_clone).await;
        });

        Ok(subagent)
    }

    pub async fn list(&self, params: SubagentListParams) -> Vec<Subagent> {
        let guard = self.subagents.read().await;
        guard
            .values()
            .filter(|s| s.session_id == params.session_id)
            .cloned()
            .collect()
    }

    pub async fn get(&self, subagent_id: &str) -> Option<Subagent> {
        let guard = self.subagents.read().await;
        guard.get(subagent_id).cloned()
    }

    pub async fn cancel(&self, params: SubagentCancelParams) -> anyhow::Result<Option<Subagent>> {
        let mut guard = self.subagents.write().await;
        if let Some(subagent) = guard.get_mut(&params.subagent_id) {
            if matches!(
                subagent.status,
                SubagentStatus::Pending | SubagentStatus::Running
            ) {
                subagent.status = SubagentStatus::Cancelled;
                subagent.completed_at = Some(now_utc());
                if let Some(reason) = &params.reason {
                    subagent.result = Some(SubagentResult {
                        summary: format!("Cancelled: {}", reason),
                        findings: serde_json::json!({}),
                        files_changed: vec![],
                        success: false,
                    });
                }
                let clone = subagent.clone();
                drop(guard);
                self.emit_event("subagent.cancelled", &clone);
                Ok(Some(clone))
            } else {
                Ok(Some(subagent.clone()))
            }
        } else {
            Ok(None)
        }
    }

    fn emit_event(&self, method: &str, subagent: &Subagent) {
        let notif = crate::api::types::JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params: serde_json::to_value(subagent).unwrap_or_default(),
        };
        if let Ok(s) = serde_json::to_string(&notif) {
            let _ = self.event_tx.send(s);
        }
    }
}

struct SubagentOrchestratorRef {
    subagents: Arc<RwLock<HashMap<String, Subagent>>>,
    event_tx: broadcast::Sender<String>,
    semaphore: Arc<Semaphore>,
    app_state: AppState,
}

impl SubagentOrchestratorRef {
    async fn execute_subagent(&self, mut subagent: Subagent) {
        let _permit = self.semaphore.acquire().await;

        subagent.status = SubagentStatus::Running;
        self.update_and_notify(&mut subagent, "subagent.started")
            .await;

        let result = self.perform_subagent_work(&subagent).await;

        subagent.result = Some(result.clone());
        subagent.status = if result.success {
            SubagentStatus::Completed
        } else {
            SubagentStatus::Failed
        };
        subagent.completed_at = Some(now_utc());
        self.update_and_notify(&mut subagent, "subagent.completed")
            .await;
    }

    /// review_prompt.md / Gemini-checklist follow-up: this used to return a
    /// canned summary per role with `files_changed: vec![]` always,
    /// regardless of what the prompt asked for — no model call, no tool
    /// call, nothing real happened. Now runs a real, bounded tool-calling
    /// turn (`ModelManager::run_subagent_turn`) scoped to the parent
    /// Session's worktree, autonomy level, and approval flow — a subagent
    /// has no separate identity from the Session's tool-execution boundary,
    /// it shares it, per this module's own doc comment above.
    async fn perform_subagent_work(&self, subagent: &Subagent) -> SubagentResult {
        let system_prompt = subagent_system_prompt(&subagent.role);
        let outcome = self
            .app_state
            .model_manager
            .run_subagent_turn(
                &subagent.session_id,
                &system_prompt,
                &subagent.prompt,
                self.app_state.clone(),
            )
            .await;

        match outcome {
            Ok(outcome) => SubagentResult {
                summary: if outcome.summary.is_empty() {
                    "Completed with no final text response".to_string()
                } else {
                    outcome.summary
                },
                findings: serde_json::json!({
                    "role": subagent.role,
                    "prompt": subagent.prompt,
                    "input_tokens": outcome.usage.input_tokens,
                    "output_tokens": outcome.usage.output_tokens,
                }),
                files_changed: outcome.files_changed,
                success: true,
            },
            Err(e) => {
                warn!(
                    "Subagent {} ({:?}) turn failed for session {}: {:?}",
                    subagent.id, subagent.role, subagent.session_id, e
                );
                SubagentResult {
                    summary: format!("Failed: {e}"),
                    findings: serde_json::json!({"error": e.to_string()}),
                    files_changed: vec![],
                    success: false,
                }
            }
        }
    }

    async fn update_and_notify(&self, subagent: &mut Subagent, method: &str) {
        let clone = subagent.clone();
        let mut guard = self.subagents.write().await;
        guard.insert(subagent.id.clone(), clone.clone());
        drop(guard);

        let event_method = if method == "subagent.started" {
            "subagent.status".to_string()
        } else {
            method.to_string()
        };

        let notif = crate::api::types::JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: event_method,
            params: serde_json::to_value(clone).unwrap_or_default(),
        };
        if let Ok(s) = serde_json::to_string(&notif) {
            let _ = self.event_tx.send(s);
        }
    }
}

/// Role-specific instructions for a subagent's one-shot turn
/// (`run_subagent_turn`). Deliberately short — a subagent's prompt already
/// carries the specific task; this just sets the frame and the tool-call
/// ceiling matches `run_subagent_turn`'s own `MAX_TOOL_ROUNDS` (25, inherited
/// from the same per-provider loops the main agent uses).
fn subagent_system_prompt(role: &SubagentRole) -> String {
    let role_line = match role {
        SubagentRole::ResearchWorker => {
            "You are a research subagent. Investigate the codebase to answer the question \
             below — read files, search, check git history. Do not write or edit files."
        }
        SubagentRole::ParallelImpl => {
            "You are an implementation subagent working on one scoped piece of a larger \
             change, in parallel with other subagents on the same codebase. Make only the \
             specific change described below — do not touch files outside its scope."
        }
        SubagentRole::CodeExplorer => {
            "You are a code-exploration subagent. Map out the structure, dependencies, or \
             usages described below and report what you find. Do not write or edit files."
        }
        SubagentRole::TestRunner => {
            "You are a test-running subagent. Run the tests or commands described below and \
             report the real result — pass/fail counts and failure output, not a guess."
        }
    };
    format!(
        "{role_line}\n\nYou share this Session's worktree with the main agent and any other \
         subagents running in parallel — another subagent may be editing a different file at \
         the same time. Stay within the scope of your own task. When you're done, summarize \
         what you actually did in your final response; that summary is reported back to the \
         Session thread."
    )
}

/// Advisory metadata surfaced to the UI (which tools this subagent role
/// *typically* needs) — not yet an enforced restriction. A subagent's actual
/// tool access is whatever its parent Session's own role profile (if any)
/// allows; `run_subagent_turn` does not currently narrow the tool set
/// further per subagent. Documented here rather than left implicit, since
/// this exact gap (metadata that looks like enforcement but isn't) is the
/// kind of thing this project's own audits keep finding.
fn default_tool_permissions(role: &SubagentRole) -> Vec<String> {
    match role {
        SubagentRole::ResearchWorker => vec![
            "file.read".to_string(),
            "file.list".to_string(),
            "git.log".to_string(),
            "git.status".to_string(),
        ],
        SubagentRole::ParallelImpl => vec![
            "file.read".to_string(),
            "file.write".to_string(),
            "file.list".to_string(),
            "git.status".to_string(),
            "pty.create".to_string(),
            "pty.write".to_string(),
        ],
        SubagentRole::CodeExplorer => vec![
            "file.read".to_string(),
            "file.list".to_string(),
            "context_engine.search".to_string(),
            "context_engine.related".to_string(),
        ],
        SubagentRole::TestRunner => vec![
            "file.read".to_string(),
            "file.list".to_string(),
            "pty.create".to_string(),
            "pty.write".to_string(),
            "git.status".to_string(),
        ],
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// No provider is configured in a fresh in-memory Core, so a real
    /// subagent turn fails fast (no network call) with a clear error rather
    /// than hanging — see `test_subagent_lifecycle` below, which relies on
    /// exactly that to stay fast and deterministic.
    fn test_app_state() -> AppState {
        crate::Core::new_in_memory().unwrap().app_state()
    }

    #[test]
    fn test_subagent_role_serde() {
        assert_eq!(
            serde_json::to_string(&SubagentRole::ResearchWorker).unwrap(),
            "\"research_worker\""
        );
        assert_eq!(
            serde_json::to_string(&SubagentRole::TestRunner).unwrap(),
            "\"test_runner\""
        );
    }

    #[test]
    fn test_default_permissions() {
        let perms = default_tool_permissions(&SubagentRole::ResearchWorker);
        assert!(perms.contains(&"file.read".to_string()));
        assert!(!perms.contains(&"file.write".to_string()));

        let perms = default_tool_permissions(&SubagentRole::ParallelImpl);
        assert!(perms.contains(&"file.write".to_string()));
    }

    #[tokio::test]
    async fn test_orchestrator_creation() {
        let (tx, _rx) = broadcast::channel(10);
        let orch = SubagentOrchestrator::new(tx);
        assert_eq!(orch.get_max_concurrent().await, DEFAULT_MAX_CONCURRENT);
    }

    #[tokio::test]
    async fn test_set_max_concurrent() {
        let (tx, _rx) = broadcast::channel(10);
        let orch = SubagentOrchestrator::new(tx);
        orch.set_max_concurrent(10).await;
        assert_eq!(orch.get_max_concurrent().await, 10);
    }

    #[tokio::test]
    async fn test_spawn_and_list() {
        let (tx, _rx) = broadcast::channel(10);
        let orch = SubagentOrchestrator::new(tx);

        let params = SubagentSpawnParams {
            session_id: "session-1".to_string(),
            role: SubagentRole::ResearchWorker,
            prompt: "Research Rust async patterns".to_string(),
            tool_permissions: None,
            model_provider: None,
            model_id: None,
        };

        let sa = orch.spawn(params, test_app_state()).await.unwrap();
        assert_eq!(sa.session_id, "session-1");
        assert_eq!(sa.role, SubagentRole::ResearchWorker);
        assert_eq!(sa.status, SubagentStatus::Pending);

        let list = orch
            .list(SubagentListParams {
                session_id: "session-1".to_string(),
            })
            .await;
        assert_eq!(list.len(), 1);

        let fetched = orch.get(&sa.id).await;
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().id, sa.id);
    }

    #[tokio::test]
    async fn test_cancel_subagent() {
        let (tx, _rx) = broadcast::channel(10);
        let orch = SubagentOrchestrator::new(tx);

        let params = SubagentSpawnParams {
            session_id: "session-2".to_string(),
            role: SubagentRole::CodeExplorer,
            prompt: "Explore the codebase".to_string(),
            tool_permissions: None,
            model_provider: None,
            model_id: None,
        };

        let sa = orch.spawn(params, test_app_state()).await.unwrap();

        let result = orch
            .cancel(SubagentCancelParams {
                subagent_id: sa.id.clone(),
                reason: Some("No longer needed".to_string()),
            })
            .await
            .unwrap();

        assert!(result.is_some());
        let cancelled = result.unwrap();
        assert_eq!(cancelled.status, SubagentStatus::Cancelled);
        assert!(cancelled.result.is_some());
    }

    #[tokio::test]
    async fn test_spawn_exceeding_limit() {
        let (tx, _rx) = broadcast::channel(10);
        let orch = SubagentOrchestrator::new(tx);
        orch.set_max_concurrent(1).await;

        let params = SubagentSpawnParams {
            session_id: "session-3".to_string(),
            role: SubagentRole::ResearchWorker,
            prompt: "Task".to_string(),
            tool_permissions: None,
            model_provider: None,
            model_id: None,
        };

        let _ = orch.spawn(params.clone(), test_app_state()).await.unwrap();
        let result = orch.spawn(params, test_app_state()).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Max concurrent subagents"));
    }

    #[tokio::test]
    async fn test_subagent_lifecycle() {
        let (tx, _rx) = broadcast::channel(10);
        let orch = SubagentOrchestrator::new(tx);

        let params = SubagentSpawnParams {
            session_id: "session-lifecycle".to_string(),
            role: SubagentRole::TestRunner,
            prompt: "Run integration tests".to_string(),
            tool_permissions: None,
            model_provider: None,
            model_id: None,
        };

        let sa = orch.spawn(params, test_app_state()).await.unwrap();
        assert_eq!(sa.status, SubagentStatus::Pending);

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // No provider is configured in this in-memory Core, so
        // `run_subagent_turn` fails fast — Failed is the real, expected
        // terminal state here, not a fallback to tolerate.
        let updated = orch.get(&sa.id).await.unwrap();
        assert!(matches!(
            updated.status,
            SubagentStatus::Running | SubagentStatus::Failed
        ));
    }

    /// A real Session a subagent turn can legitimately run against — plain
    /// `resolve_active_config` in this dev environment can pick up
    /// `OPENROUTER_API_KEY` from the shell (set for this repo's own
    /// OpenCode delegation workflow, see CLAUDE.md), so tests can't assume
    /// "no provider configured." Explicit per-role settings pointing at an
    /// address nothing listens on make the outcome deterministic instead —
    /// same principle as `a_session_already_over_its_spend_cap_is_blocked...`
    /// in `model::spend_tracking_tests`.
    fn session_with_unreachable_provider(app_state: &AppState) -> String {
        let repo = app_state
            .persistence
            .connect_repo("/tmp/subagent-test-repo", None)
            .unwrap();
        let session = app_state
            .persistence
            .create_session(
                &repo.id,
                "Subagent test",
                "test",
                crate::api::types::IsolationMode::Shared,
                crate::api::types::AutonomyLevel::CoPilot,
            )
            .unwrap();
        let mut settings = app_state.persistence.get_settings().unwrap();
        settings.implementer_provider = Some("openai_compatible".to_string());
        settings.openai_compatible_endpoint = Some("http://127.0.0.1:1".to_string());
        settings.openai_compatible_api_key = Some("unused".to_string());
        app_state.persistence.update_settings(&settings).unwrap();
        session.id
    }

    #[tokio::test]
    async fn perform_subagent_work_reports_a_clear_error_when_the_provider_is_unreachable() {
        let (tx, _rx) = broadcast::channel(10);
        let orch = SubagentOrchestrator::new(tx);
        let app_state = test_app_state();
        let session_id = session_with_unreachable_provider(&app_state);

        let params = SubagentSpawnParams {
            session_id,
            role: SubagentRole::ResearchWorker,
            prompt: "Find all usages of foo()".to_string(),
            tool_permissions: None,
            model_provider: None,
            model_id: None,
        };

        let sa = orch.spawn(params, app_state).await.unwrap();

        let mut updated = orch.get(&sa.id).await.unwrap();
        for _ in 0..40 {
            if updated.status == SubagentStatus::Failed {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            updated = orch.get(&sa.id).await.unwrap();
        }

        assert_eq!(updated.status, SubagentStatus::Failed);
        let result = updated
            .result
            .expect("a failed subagent must still carry a result");
        assert!(!result.success);
        assert!(result.files_changed.is_empty());
    }
}
