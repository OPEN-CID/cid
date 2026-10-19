//! Workflows — Jenkins-style pipelines over Sessions (v1: linear chains).
//!
//! A Workflow is an ordered, repo-scoped list of steps executed against one
//! Session, in order, with per-step status persisted after every transition
//! so a crash between steps resumes rather than re-runs. Three step kinds,
//! each reusing an already-wired execution path rather than inventing a
//! parallel one:
//!
//! - `Agent`: a real Implementer turn (`ModelManager::process_message`),
//!   with an optional per-step model override applied through the Session's
//!   own `model_provider`/`model_id` columns (the same override Sessions
//!   carry since task 3) and restored afterwards. A "local model step" is
//!   not a separate kind — it is an Agent step whose override points at the
//!   Ollama/LM Studio/llama.cpp provider.
//! - `Command`: a shell command through `execute_tool_with_approval` on
//!   `run_terminal`, so the repo allow-list, sandbox, and any human-approval
//!   wait apply unchanged. A denied/blocked result fails the step; only a
//!   transport error fails the runner itself.
//! - `ManualGate`: the pipeline pauses (`WaitingApproval`, persisted) until a
//!   human continues it via `workflow.run.approve_step` — the Jenkins `input`
//!   step. Crash-safe by construction: there is no in-memory wait to lose.
//!
//! Deliberately out of v1 (recorded here so they are decisions, not gaps):
//! fan-out/fan-in (needs artifact passing between branches — variable
//! interpolation across step outputs — which is a design of its own),
//! automatic resume after a Core restart (a crashed run stays Failed/Cancelled
//! until a human retries it; auto-running shell commands after a crash
//! without human action would be the wrong default), and killing a turn
//! mid-flight on cancel (cancel takes effect between steps; a turn is already
//! bounded by the provider loop's round cap). One active run per Session is
//! enforced — two chains driving one worktree would interleave turns.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{broadcast, RwLock};
use tracing::{info, warn};

use crate::api::router::AppState;
use crate::api::types::{
    now_utc, MessageRole, SessionStatus, Workflow, WorkflowRun, WorkflowRunStatus, WorkflowStep,
    WorkflowStepFailurePolicy, WorkflowStepKind, WorkflowStepRun, WorkflowStepRunStatus,
};

/// What one finished step means for the chain as a whole. Pure (no I/O) so
/// the sequencing rule is unit-testable without a Core, a provider, or a DB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StepOutcome {
    Advance,
    HaltFailed,
}

fn apply_step_result(step: &WorkflowStep, succeeded: bool) -> StepOutcome {
    if succeeded {
        return StepOutcome::Advance;
    }
    match step.on_failure {
        WorkflowStepFailurePolicy::Continue => StepOutcome::Advance,
        WorkflowStepFailurePolicy::Stop => StepOutcome::HaltFailed,
    }
}

/// Assign server-minted ids to steps that arrive without one, so stored
/// workflows always have stable per-step identity for run state to key on.
/// Shared by the `workflow.create/update` RPCs and the `create_workflow`
/// agent tool — one definition of "a storable step", not two.
pub(crate) fn normalize_steps(mut steps: Vec<WorkflowStep>) -> Vec<WorkflowStep> {
    for step in steps.iter_mut() {
        if step.id.trim().is_empty() {
            step.id = crate::api::types::new_id();
        }
    }
    steps
}

/// Structural validation for create/update. Pure, tested below — a workflow
/// that cannot run must be refused at write time, not discovered mid-run.
pub(crate) fn validate_steps(steps: &[WorkflowStep]) -> anyhow::Result<()> {
    if steps.is_empty() {
        anyhow::bail!("a workflow needs at least one step");
    }
    for (i, step) in steps.iter().enumerate() {
        if step.name.trim().is_empty() {
            anyhow::bail!("step {} has no name", i + 1);
        }
        if let Some(provider) = step.model_provider.as_deref() {
            if crate::api::types::parse_provider_str(provider).is_none() {
                anyhow::bail!(
                    "step '{}' names an unknown model provider '{provider}'",
                    step.name
                );
            }
        }
        match step.kind {
            WorkflowStepKind::Agent => {
                if step.prompt.as_deref().unwrap_or("").trim().is_empty() {
                    anyhow::bail!("agent step '{}' needs a prompt", step.name);
                }
            }
            WorkflowStepKind::Command => {
                if step.command.as_deref().unwrap_or("").trim().is_empty() {
                    anyhow::bail!("command step '{}' needs a command", step.name);
                }
            }
            WorkflowStepKind::ManualGate => {}
        }
    }
    Ok(())
}

fn fresh_step_runs(steps: &[WorkflowStep]) -> Vec<WorkflowStepRun> {
    steps
        .iter()
        .map(|s| WorkflowStepRun {
            step_id: s.id.clone(),
            status: WorkflowStepRunStatus::Pending,
            output: None,
            error: None,
            started_at: None,
            completed_at: None,
        })
        .collect()
}

pub struct WorkflowRunner {
    event_tx: broadcast::Sender<String>,
    /// session_id -> run_id for chains currently driving that Session.
    active_runs: Arc<RwLock<HashMap<String, String>>>,
}

fn emit(event_tx: &broadcast::Sender<String>, method: &str, params: serde_json::Value) {
    let notif = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    });
    if let Ok(s) = serde_json::to_string(&notif) {
        let _ = event_tx.send(s);
    }
}

fn emit_run(event_tx: &broadcast::Sender<String>, run: &WorkflowRun) {
    emit(
        event_tx,
        "workflow.run.update",
        serde_json::json!({ "run": run }),
    );
}

impl WorkflowRunner {
    pub fn new(event_tx: broadcast::Sender<String>) -> Self {
        Self {
            event_tx,
            active_runs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn emit_run(&self, run: &WorkflowRun) {
        emit_run(&self.event_tx, run);
    }

    pub fn validate_for_write(&self, steps: &[WorkflowStep]) -> anyhow::Result<()> {
        validate_steps(steps)
    }

    /// Start a chain. Refuses when the Session already has an active run, or
    /// when the Session belongs to a different repo than the workflow — a
    /// chain writing to a worktree its steps were not written for.
    pub async fn start(
        &self,
        workflow_id: &str,
        session_id: &str,
        app_state: AppState,
    ) -> anyhow::Result<WorkflowRun> {
        let workflow: Workflow = app_state.persistence.get_workflow(workflow_id)?;
        let session = app_state.persistence.get_session(session_id)?;
        // Fail fast on a plan-gated Session: without this, every agent step
        // parks on the same gate one turn at a time and the run looks
        // mysteriously stuck — the exact "clicked Start and nothing happens"
        // state this guard turns into an actionable error instead.
        if workflow
            .steps
            .iter()
            .any(|s| s.kind == WorkflowStepKind::Agent)
            && app_state
                .role_runner
                .implementer_is_gated(session_id)?
                .is_some()
        {
            anyhow::bail!(
                "this Session's plan is not approved yet, so agent steps cannot run — \
                 approve the plan in the Chat thread (or run a commands-only workflow) and start again"
            );
        }
        if session.repo_channel_id != workflow.repo_channel_id {
            anyhow::bail!(
                "session belongs to a different repo than workflow '{}'",
                workflow.name
            );
        }
        {
            let active = self.active_runs.read().await;
            if let Some(existing) = active.get(session_id) {
                anyhow::bail!(
                    "session already has an active workflow run ({existing}) — cancel or wait for it first"
                );
            }
        }
        let run = app_state.persistence.create_workflow_run(
            workflow_id,
            session_id,
            &fresh_step_runs(&workflow.steps),
        )?;
        {
            let mut active = self.active_runs.write().await;
            active.insert(session_id.to_string(), run.id.clone());
        }
        info!(
            "Workflow run {} started: workflow '{}' on session {}",
            run.id, workflow.name, session_id
        );
        self.emit_run(&run);
        self.spawn_drive(run.id.clone(), app_state);
        Ok(run)
    }

    /// Continue a run paused at a manual gate. Anything else is refused — an
    /// approval only ever advances a step that is actually waiting for one.
    pub async fn approve_step(
        &self,
        run_id: &str,
        note: Option<String>,
        app_state: AppState,
    ) -> anyhow::Result<WorkflowRun> {
        let run = app_state.persistence.get_workflow_run(run_id)?;
        if run.status != WorkflowRunStatus::WaitingApproval {
            anyhow::bail!("run is not waiting for approval (status: {:?})", run.status);
        }
        let workflow = app_state.persistence.get_workflow(&run.workflow_id)?;
        let mut steps = run.steps.clone();
        if let Some(step_run) = steps.get_mut(run.current_index) {
            step_run.status = WorkflowStepRunStatus::Completed;
            step_run.output = note;
            step_run.completed_at = Some(now_utc());
        }
        let _ = workflow;
        let run = app_state.persistence.update_workflow_run_step(
            run_id,
            run.current_index + 1,
            &WorkflowRunStatus::Running,
            &steps,
        )?;
        self.emit_run(&run);
        self.spawn_drive(run.id.clone(), app_state);
        Ok(run)
    }

    /// Cancel a run. Takes effect between steps — a turn already executing is
    /// bounded by the provider loop's round cap, so the chain stops at the
    /// next step boundary at the latest.
    pub async fn cancel(&self, run_id: &str, app_state: AppState) -> anyhow::Result<WorkflowRun> {
        let run = app_state.persistence.get_workflow_run(run_id)?;
        match run.status {
            WorkflowRunStatus::Running | WorkflowRunStatus::WaitingApproval => {}
            _ => anyhow::bail!("run is already terminal ({:?})", run.status),
        }
        let run = app_state.persistence.update_workflow_run_step(
            run_id,
            run.current_index,
            &WorkflowRunStatus::Cancelled,
            &run.steps,
        )?;
        {
            let mut active = self.active_runs.write().await;
            active.remove(&run.session_id);
        }
        self.emit_run(&run);
        Ok(run)
    }

    /// Retry a Failed or Cancelled run from the first step that did not
    /// complete. Completed steps are never re-executed — a command that
    /// already ran stays run.
    pub async fn retry(&self, run_id: &str, app_state: AppState) -> anyhow::Result<WorkflowRun> {
        let run = app_state.persistence.get_workflow_run(run_id)?;
        match run.status {
            WorkflowRunStatus::Failed | WorkflowRunStatus::Cancelled => {}
            _ => anyhow::bail!("only failed or cancelled runs can be retried"),
        }
        {
            let active = self.active_runs.read().await;
            if let Some(existing) = active.get(&run.session_id) {
                anyhow::bail!("session already has an active workflow run ({existing})");
            }
        }
        let first_open = run
            .steps
            .iter()
            .position(|s| s.status != WorkflowStepRunStatus::Completed)
            .unwrap_or(run.steps.len());
        let mut steps = run.steps.clone();
        for step_run in steps.iter_mut().skip(first_open) {
            step_run.status = WorkflowStepRunStatus::Pending;
            step_run.error = None;
            step_run.started_at = None;
            step_run.completed_at = None;
        }
        let run = app_state.persistence.update_workflow_run_step(
            run_id,
            first_open,
            &WorkflowRunStatus::Running,
            &steps,
        )?;
        {
            let mut active = self.active_runs.write().await;
            active.insert(run.session_id.clone(), run.id.clone());
        }
        self.emit_run(&run);
        self.spawn_drive(run.id.clone(), app_state);
        Ok(run)
    }

    fn spawn_drive(&self, run_id: String, app_state: AppState) {
        let event_tx = self.event_tx.clone();
        let active_runs = self.active_runs.clone();
        tokio::spawn(async move {
            drive_run(&run_id, app_state, &event_tx, &active_runs).await;
        });
    }

    /// Release a Session's claim. Called exactly once per run, on every path
    /// that reaches a terminal run status — the background task ending any
    /// other way (panic, crash) leaves the claim held, and the next start is
    /// refused until a human cancels/retries, which is the safe direction to
    /// fail: never two chains driving one worktree.
    async fn release(active_runs: &Arc<RwLock<HashMap<String, String>>>, session_id: &str) {
        let mut active = active_runs.write().await;
        active.remove(session_id);
    }
}

/// The chain driver: walks the workflow's steps in order until the run is
/// Complete, Failed, Cancelled, or parked at a manual gate. Every transition
/// is persisted before moving on, so a crash between steps resumes from the
/// persisted cursor instead of re-running a command that already executed.
async fn drive_run(
    run_id: &str,
    app_state: AppState,
    event_tx: &broadcast::Sender<String>,
    active_runs: &Arc<RwLock<HashMap<String, String>>>,
) {
    loop {
        let run = match app_state.persistence.get_workflow_run(run_id) {
            Ok(r) => r,
            Err(e) => {
                warn!("Workflow drive: run {run_id} vanished: {e:?}");
                return;
            }
        };
        if run.status == WorkflowRunStatus::Cancelled {
            WorkflowRunner::release(active_runs, &run.session_id).await;
            emit_run(event_tx, &run);
            return;
        }
        if run.status != WorkflowRunStatus::Running {
            // Completed, Failed, or parked at a gate — nothing to drive.
            // (WaitingApproval keeps its session claim; approve_step resumes.)
            if run.status == WorkflowRunStatus::Completed || run.status == WorkflowRunStatus::Failed
            {
                WorkflowRunner::release(active_runs, &run.session_id).await;
            }
            return;
        }

        let workflow = match app_state.persistence.get_workflow(&run.workflow_id) {
            Ok(w) => w,
            Err(e) => {
                fail_run(
                    &app_state,
                    event_tx,
                    active_runs,
                    &run,
                    &format!("workflow gone: {e}"),
                )
                .await;
                return;
            }
        };
        if workflow.steps.len() != run.steps.len() {
            fail_run(
                &app_state,
                event_tx,
                active_runs,
                &run,
                "workflow was edited while this run was in flight — retry starts from the new definition",
            )
            .await;
            return;
        }
        let Some(step) = workflow.steps.get(run.current_index) else {
            let run = app_state
                .persistence
                .update_workflow_run_step(
                    run_id,
                    run.current_index,
                    &WorkflowRunStatus::Completed,
                    &run.steps,
                )
                .unwrap_or(run);
            WorkflowRunner::release(active_runs, &run.session_id).await;
            emit_run(event_tx, &run);
            return;
        };

        // Mark the step running before executing it, so a crash mid-step
        // resumes *at* this step rather than silently skipping it.
        let mut steps = run.steps.clone();
        if let Some(step_run) = steps.get_mut(run.current_index) {
            step_run.status = WorkflowStepRunStatus::Running;
            step_run.started_at = Some(now_utc());
        }
        let run = app_state
            .persistence
            .update_workflow_run_step(run_id, run.current_index, &run.status, &steps)
            .unwrap_or(run);
        emit_run(event_tx, &run);

        if step.kind == WorkflowStepKind::ManualGate {
            let mut steps = run.steps.clone();
            if let Some(step_run) = steps.get_mut(run.current_index) {
                step_run.status = WorkflowStepRunStatus::WaitingApproval;
            }
            if let Ok(run) = app_state.persistence.update_workflow_run_step(
                run_id,
                run.current_index,
                &WorkflowRunStatus::WaitingApproval,
                &steps,
            ) {
                emit_run(event_tx, &run);
                emit(
                    event_tx,
                    "workflow.step.waiting",
                    serde_json::json!({ "run_id": run_id, "step_id": step.id, "step_name": step.name }),
                );
            }
            return;
        }

        let (succeeded, output, error) = match step.kind {
            WorkflowStepKind::Agent => run_agent_step(&app_state, &run.session_id, step).await,
            WorkflowStepKind::Command => run_command_step(&app_state, &run.session_id, step).await,
            WorkflowStepKind::ManualGate => unreachable!(),
        };

        // A cancel that landed mid-step takes effect here: the step's work
        // already happened, so it is marked Skipped rather than Completed,
        // and the chain stops without advancing.
        let fresh = app_state
            .persistence
            .get_workflow_run(run_id)
            .unwrap_or(run.clone());
        if fresh.status == WorkflowRunStatus::Cancelled {
            let mut steps = fresh.steps.clone();
            if let Some(step_run) = steps.get_mut(fresh.current_index) {
                step_run.status = WorkflowStepRunStatus::Skipped;
                step_run.completed_at = Some(now_utc());
            }
            if let Ok(run) = app_state.persistence.update_workflow_run_step(
                run_id,
                fresh.current_index,
                &WorkflowRunStatus::Cancelled,
                &steps,
            ) {
                WorkflowRunner::release(active_runs, &run.session_id).await;
                emit_run(event_tx, &run);
            }
            return;
        }

        let mut steps = fresh.steps.clone();
        if let Some(step_run) = steps.get_mut(fresh.current_index) {
            step_run.status = if succeeded {
                WorkflowStepRunStatus::Completed
            } else {
                WorkflowStepRunStatus::Failed
            };
            step_run.output = output;
            step_run.error = error;
            step_run.completed_at = Some(now_utc());
        }
        match apply_step_result(step, succeeded) {
            StepOutcome::Advance => {
                if app_state
                    .persistence
                    .update_workflow_run_step(
                        run_id,
                        fresh.current_index + 1,
                        &WorkflowRunStatus::Running,
                        &steps,
                    )
                    .is_err()
                {
                    return;
                }
            }
            StepOutcome::HaltFailed => {
                fail_run(
                    &app_state,
                    event_tx,
                    active_runs,
                    &fresh,
                    &format!("step '{}' failed", step.name),
                )
                .await;
                // Persist the failed step state (fail_run only flips status).
                let _ = app_state.persistence.update_workflow_run_step(
                    run_id,
                    fresh.current_index,
                    &WorkflowRunStatus::Failed,
                    &steps,
                );
                if let Ok(run) = app_state.persistence.get_workflow_run(run_id) {
                    emit_run(event_tx, &run);
                }
                return;
            }
        }
    }
}

async fn fail_run(
    app_state: &AppState,
    event_tx: &broadcast::Sender<String>,
    active_runs: &Arc<RwLock<HashMap<String, String>>>,
    run: &WorkflowRun,
    reason: &str,
) {
    warn!("Workflow run {} failed: {reason}", run.id);
    if let Ok(run) = app_state.persistence.update_workflow_run_step(
        &run.id,
        run.current_index,
        &WorkflowRunStatus::Failed,
        &run.steps,
    ) {
        WorkflowRunner::release(active_runs, &run.session_id).await;
        emit_run(event_tx, &run);
    }
}

/// One Agent step: a real Implementer turn on the Session, with the step's
/// model override (cloud or local runtime) applied through the Session's own
/// columns and restored afterwards — the override is per-step, never leaked
/// into the Session's standing configuration.
async fn run_agent_step(
    app_state: &AppState,
    session_id: &str,
    step: &WorkflowStep,
) -> (bool, Option<String>, Option<String>) {
    let prompt = step.prompt.clone().unwrap_or_default();
    let session = match app_state.persistence.get_session(session_id) {
        Ok(s) => s,
        Err(e) => return (false, None, Some(format!("session gone: {e}"))),
    };
    let prev_provider = session.model_provider.clone();
    let prev_model = session.model_id.clone();
    // The override is stored text validated by `validate_steps`, and the
    // agent loop parses it with the same lenient `parse_provider_str` the
    // Session's own override uses — no enum round-trip in between.
    if app_state
        .persistence
        .update_session_model(
            session_id,
            step.model_provider.clone(),
            step.model_id.clone(),
        )
        .is_err()
    {
        return (
            false,
            None,
            Some("could not apply the step's model override".to_string()),
        );
    }
    if let Err(e) = app_state.persistence.create_message(
        session_id,
        MessageRole::User,
        &format!("⧉ Workflow step `{}`:\n\n{prompt}", step.name),
        vec![],
    ) {
        let _ = app_state
            .persistence
            .update_session_model(session_id, prev_provider, prev_model);
        return (
            false,
            None,
            Some(format!("could not post the step prompt: {e}")),
        );
    }
    let turn = app_state
        .model_manager
        .process_message(session_id, &prompt, app_state.clone())
        .await;
    let _ = app_state
        .persistence
        .update_session_model(session_id, prev_provider, prev_model);
    match turn {
        Err(e) => (false, None, Some(format!("agent turn errored: {e}"))),
        Ok(()) => match app_state.persistence.get_session(session_id) {
            Ok(s) if s.status == SessionStatus::Failed => (
                false,
                None,
                Some("agent turn ended with session status Failed — see the thread".to_string()),
            ),
            Ok(_) => {
                let output = app_state
                    .persistence
                    .list_messages(session_id)
                    .ok()
                    .and_then(|msgs| {
                        msgs.iter()
                            .rev()
                            .find(|m| m.role == MessageRole::Assistant)
                            .map(|m| serde_json::json!({ "message_id": m.id }).to_string())
                    });
                (true, output, None)
            }
            Err(e) => (false, None, Some(format!("session gone after turn: {e}"))),
        },
    }
}

/// One Command step: the shell command through the Session's autonomy gate.
/// A transport error fails the step; a gate refusal (`denied`/`blocked`) or
/// a non-zero exit fails it with the gate's own reason — never silently.
async fn run_command_step(
    app_state: &AppState,
    session_id: &str,
    step: &WorkflowStep,
) -> (bool, Option<String>, Option<String>) {
    let command = step.command.clone().unwrap_or_default();
    let args = serde_json::json!({ "command": command });
    let result = app_state
        .model_manager
        .execute_tool_with_approval(session_id, "run_terminal", args, app_state.clone())
        .await;
    match result {
        Err(e) => (false, None, Some(format!("command failed to execute: {e}"))),
        Ok(v) => {
            let status = &v["status"];
            if status == "denied" {
                return (
                    false,
                    None,
                    Some(
                        v["message"]
                            .as_str()
                            .unwrap_or("command denied")
                            .to_string(),
                    ),
                );
            }
            if status == "blocked" {
                return (
                    false,
                    None,
                    Some(
                        v["reason"]
                            .as_str()
                            .unwrap_or("command blocked by sandbox")
                            .to_string(),
                    ),
                );
            }
            let code = status.as_i64().unwrap_or(0);
            let summary = serde_json::json!({
                "exit_code": code,
                "stdout_tail": v["stdout"].as_str().unwrap_or("").chars().rev().take(2000).collect::<String>().chars().rev().collect::<String>(),
                "stderr_tail": v["stderr"].as_str().unwrap_or("").chars().rev().take(1000).collect::<String>().chars().rev().collect::<String>(),
            })
            .to_string();
            if code != 0 {
                (
                    false,
                    Some(summary),
                    Some(format!("command exited with status {code}")),
                )
            } else {
                (true, Some(summary), None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::WorkflowStepKind;

    fn agent_step(name: &str) -> WorkflowStep {
        WorkflowStep {
            id: format!("step-{name}"),
            name: name.to_string(),
            kind: WorkflowStepKind::Agent,
            prompt: Some("do the thing".to_string()),
            command: None,
            model_provider: None,
            model_id: None,
            on_failure: WorkflowStepFailurePolicy::Stop,
        }
    }

    #[test]
    fn an_empty_workflow_is_refused_at_write_time() {
        assert!(validate_steps(&[]).is_err());
    }

    #[test]
    fn an_agent_step_without_a_prompt_is_refused() {
        let mut step = agent_step("plan");
        step.prompt = None;
        let err = validate_steps(&[step]).unwrap_err();
        assert!(err.to_string().contains("needs a prompt"));
    }

    #[test]
    fn a_command_step_without_a_command_is_refused() {
        let step = WorkflowStep {
            id: "s1".to_string(),
            name: "test".to_string(),
            kind: WorkflowStepKind::Command,
            prompt: None,
            command: Some("   ".to_string()),
            model_provider: None,
            model_id: None,
            on_failure: WorkflowStepFailurePolicy::Stop,
        };
        assert!(validate_steps(&[step]).is_err());
    }

    #[test]
    fn a_manual_gate_needs_nothing_but_a_name() {
        let step = WorkflowStep {
            id: "s1".to_string(),
            name: "human checkpoint".to_string(),
            kind: WorkflowStepKind::ManualGate,
            prompt: None,
            command: None,
            model_provider: None,
            model_id: None,
            on_failure: WorkflowStepFailurePolicy::Stop,
        };
        assert!(validate_steps(&[step]).is_ok());
    }

    #[test]
    fn a_failed_step_halts_unless_it_is_marked_continue() {
        let stop = agent_step("a");
        assert_eq!(apply_step_result(&stop, false), StepOutcome::HaltFailed);
        let mut cont = agent_step("b");
        cont.on_failure = WorkflowStepFailurePolicy::Continue;
        assert_eq!(apply_step_result(&cont, false), StepOutcome::Advance);
        assert_eq!(apply_step_result(&stop, true), StepOutcome::Advance);
    }
}
