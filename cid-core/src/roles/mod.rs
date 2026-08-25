/*!
 * Planner and Reviewer role workflows.
 *
 * Part 5 of the founding brief ships three composable roles. The Implementer is
 * the tool-driving agent loop in `model`; the other two produce documents rather
 * than driving tools, and are modelled here as explicit Session lifecycle stages:
 *
 *   Planner  — Flow 1 step 3: proposes an editable plan before any file is touched.
 *              The human edits/approves it; the Implementer refuses to run without
 *              an approved plan in Co-Pilot and Autonomous autonomy.
 *   Reviewer — Flow 1 step 6: a second pass over the accumulated diff before the
 *              change is presented for approval or opened as a PR.
 *
 * Both are prompt + model-config only, per Part 5 — not independent subsystems.
 */

use std::sync::Arc;

use anyhow::{anyhow, Result};
use chrono::Utc;

use crate::api::types::{
    AgentRole, AutonomyLevel, MessageRole, ReviewFinding, ReviewSeverity, ReviewVerdict,
    SessionPlan, SessionPlanStatus, SessionReview,
};
use crate::model::ModelManager;
use crate::persistence::Persistence;

const PLANNER_SYSTEM: &str = "\
You are the Planner for a coding session. Produce a short, concrete implementation plan \
BEFORE any file is modified. Respond with exactly three markdown sections and nothing else:

## Requirements
What the change must satisfy, as a short bullet list. Include acceptance criteria.

## Approach
Two to five sentences on the technical approach and why.

## Steps
A numbered list of concrete, individually reviewable steps. Each step names the files it \
touches and what it does. Keep steps small enough that a human can approve one at a time.

Do not write code. Do not speculate about files you have not been told exist.";

const REVIEWER_SYSTEM: &str = "\
You are the Reviewer. You are given a unified diff produced by another agent. Flag likely \
bugs, security issues, and scope creep — changes outside what the plan called for. \
Respond with a markdown list where each finding is one line in exactly this form:

SEVERITY | file path | one-sentence description

SEVERITY is one of: critical, warning, info. If the diff is clean, respond with the single \
line: NO FINDINGS. Do not restate the diff. Do not praise the change.";

pub struct RoleRunner {
    persistence: Arc<Persistence>,
    model_manager: Arc<ModelManager>,
}

impl RoleRunner {
    pub fn new(persistence: Arc<Persistence>, model_manager: Arc<ModelManager>) -> Self {
        Self {
            persistence,
            model_manager,
        }
    }

    // -----------------------------------------------------------------------
    // Planner
    // -----------------------------------------------------------------------

    /// Run the Planner for a Session and persist the resulting draft plan.
    ///
    /// Without `force` this only generates when the Session has no plan at all,
    /// so a background re-plan can never overwrite a human's edits or silently
    /// discard an approval. Re-planning is an explicit `force` call.
    pub async fn generate_plan(&self, session_id: &str, force: bool) -> Result<SessionPlan> {
        let session = self.persistence.get_session(session_id)?;

        if !force {
            if let Some(existing) = self.persistence.get_session_plan(session_id)? {
                return Ok(existing);
            }
        }

        let repo = self
            .persistence
            .get_repo_channel(&session.repo_channel_id)?;
        let user_prompt = format!(
            "Repository: {}\nSession: {}\n\nTask:\n{}",
            repo.path, session.title, session.task_description
        );

        let content = match self
            .model_manager
            .complete_text(AgentRole::Planner, PLANNER_SYSTEM, &user_prompt)
            .await
        {
            Ok(Some(text)) if !text.trim().is_empty() => text,
            // Genuinely nothing configured to call.
            Ok(_) => placeholder_plan(
                &session.title,
                &session.task_description,
                "No planning model is configured",
            ),
            // The model *is* configured and was called — it failed. Saying
            // "no model is configured" here sent people to reconfigure a
            // perfectly good setup while the real cause (a 503, a bad key, a
            // retired id) went unreported.
            Err(e) => {
                tracing::warn!(
                    "Planner model call failed for session {}: {:?}",
                    session_id,
                    e
                );
                placeholder_plan(
                    &session.title,
                    &session.task_description,
                    &format!("The planning model was called but failed: {e}"),
                )
            }
        };

        let plan = self.persistence.upsert_session_plan(session_id, &content)?;

        self.persistence.create_message(
            session_id,
            MessageRole::Assistant,
            &format!("**Planner** proposed a plan — review and approve before implementation.\n\n{content}"),
            vec![],
        )?;

        Ok(plan)
    }

    /// Generate a minimal plan and immediately approve it — the vibe-coding
    /// preset (Phase 5): reduced Planner ceremony for a quick, low-stakes
    /// change. This shortens *planning*, not review: the Implementer still
    /// runs under whichever autonomy level the Session was created with, so
    /// Co-Pilot's per-tool-call approval, the diff viewer, and History are
    /// completely unaffected — a vibe Session is still logged, still diffed,
    /// still reviewable.
    ///
    /// Skips the model call entirely (no Requirements/Approach/Steps
    /// ceremony to draft) — a one-line plan naming the task is enough to
    /// satisfy the plan-approval gate without pretending a quick fix needs a
    /// formal planning document.
    pub fn generate_vibe_plan(&self, session_id: &str) -> Result<SessionPlan> {
        let session = self.persistence.get_session(session_id)?;
        let content = format!(
            "## Steps\n1. {}\n\n_Vibe-coding preset: reduced planning ceremony for a quick, low-stakes change. The Implementer still runs under Co-Pilot's per-tool-call approval unless this Session is Autonomous._",
            session.task_description
        );
        self.persistence.upsert_session_plan(session_id, &content)?;
        let approved = self.persistence.set_session_plan_status(
            session_id,
            SessionPlanStatus::Approved,
            Some("vibe-preset"),
        )?;

        self.persistence.create_message(
            session_id,
            MessageRole::System,
            "**Vibe mode**: plan auto-approved for a quick, low-stakes change. Tool calls still require your approval per the Session's autonomy level.",
            vec![],
        )?;

        Ok(approved)
    }

    /// Persist a human-edited plan. Editing returns an approved plan to draft,
    /// because the approval applied to the previous text, not this one.
    pub fn update_plan(&self, session_id: &str, content: &str) -> Result<SessionPlan> {
        if content.trim().is_empty() {
            return Err(anyhow!("plan content cannot be empty"));
        }
        self.persistence.upsert_session_plan(session_id, content)
    }

    pub fn approve_plan(&self, session_id: &str, approved_by: Option<&str>) -> Result<SessionPlan> {
        let plan = self
            .persistence
            .get_session_plan(session_id)?
            .ok_or_else(|| anyhow!("Session {} has no plan to approve", session_id))?;
        if plan.content.trim().is_empty() {
            return Err(anyhow!("cannot approve an empty plan"));
        }
        let approved = self.persistence.set_session_plan_status(
            session_id,
            SessionPlanStatus::Approved,
            approved_by,
        )?;
        self.persistence.create_message(
            session_id,
            MessageRole::System,
            "Plan approved. The Implementer may now execute it.",
            vec![],
        )?;
        Ok(approved)
    }

    pub fn reject_plan(&self, session_id: &str, reason: Option<&str>) -> Result<SessionPlan> {
        let rejected = self.persistence.set_session_plan_status(
            session_id,
            SessionPlanStatus::Rejected,
            None,
        )?;
        let note = match reason {
            Some(r) if !r.trim().is_empty() => format!("Plan rejected: {r}"),
            _ => "Plan rejected.".to_string(),
        };
        self.persistence
            .create_message(session_id, MessageRole::System, &note, vec![])?;
        Ok(rejected)
    }

    pub fn get_plan(&self, session_id: &str) -> Result<Option<SessionPlan>> {
        self.persistence.get_session_plan(session_id)
    }

    /// Whether the Implementer is allowed to execute for this Session right now.
    ///
    /// Manual autonomy has no plan gate — the human is driving. Co-Pilot and
    /// Autonomous both require an approved plan, which is what makes "approve the
    /// plan, then it runs" a real gate rather than a UI convention.
    pub fn implementer_is_gated(&self, session_id: &str) -> Result<Option<String>> {
        let session = self.persistence.get_session(session_id)?;
        if session.autonomy_level == AutonomyLevel::Manual {
            return Ok(None);
        }
        match self.persistence.get_session_plan(session_id)? {
            Some(plan) if plan.status == SessionPlanStatus::Approved => Ok(None),
            Some(plan) => Ok(Some(format!(
                "The plan for this Session is {:?}, not approved. Approve it before the Implementer runs.",
                plan.status
            ))),
            None => Ok(Some(
                "This Session has no plan yet. Run the Planner and approve its plan first.".to_string(),
            )),
        }
    }

    // -----------------------------------------------------------------------
    // Reviewer
    // -----------------------------------------------------------------------

    /// Run the Reviewer over a Session's accumulated diff and persist the result.
    pub async fn run_review(&self, session_id: &str, diff: &str) -> Result<SessionReview> {
        let session = self.persistence.get_session(session_id)?;
        let plan = self.persistence.get_session_plan(session_id)?;

        if diff.trim().is_empty() {
            let review = SessionReview {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: session_id.to_string(),
                verdict: ReviewVerdict::Clean,
                findings: vec![],
                raw_output: "No changes to review.".to_string(),
                created_at: Utc::now(),
            };
            return self.persistence.save_session_review(&review);
        }

        let user_prompt = format!(
            "Session: {}\n\nApproved plan:\n{}\n\nDiff under review:\n```diff\n{}\n```",
            session.title,
            plan.as_ref()
                .map(|p| p.content.as_str())
                .unwrap_or("(no plan recorded)"),
            truncate_diff(diff)
        );

        let raw = match self
            .model_manager
            .complete_text(AgentRole::Reviewer, REVIEWER_SYSTEM, &user_prompt)
            .await
        {
            Ok(Some(text)) if !text.trim().is_empty() => text,
            Ok(_) => "Reviewer not run: no model credentials configured for the Reviewer role."
                .to_string(),
            Err(e) => {
                tracing::warn!(
                    "Reviewer model call failed for session {}: {:?}",
                    session_id,
                    e
                );
                format!("Reviewer could not run: {e}")
            }
        };

        let findings = parse_findings(&raw);
        let verdict = verdict_for(&findings, &raw);

        let review = SessionReview {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            verdict,
            findings,
            raw_output: raw.clone(),
            created_at: Utc::now(),
        };
        let saved = self.persistence.save_session_review(&review)?;

        let summary = if saved.findings.is_empty() {
            format!("**Reviewer**: {:?} — no findings.", saved.verdict)
        } else {
            let lines: Vec<String> = saved
                .findings
                .iter()
                .map(|f| format!("- **{:?}** `{}` — {}", f.severity, f.file, f.description))
                .collect();
            format!(
                "**Reviewer**: {:?} — {} finding(s).\n\n{}",
                saved.verdict,
                saved.findings.len(),
                lines.join("\n")
            )
        };
        self.persistence
            .create_message(session_id, MessageRole::Assistant, &summary, vec![])?;

        Ok(saved)
    }

    pub fn latest_review(&self, session_id: &str) -> Result<Option<SessionReview>> {
        self.persistence.get_latest_session_review(session_id)
    }
}

/// Diffs can be far larger than a model's context. Keep the head, which is where
/// review value concentrates, and say plainly that the rest was dropped.
fn truncate_diff(diff: &str) -> String {
    const MAX: usize = 60_000;
    if diff.len() <= MAX {
        return diff.to_string();
    }
    let cut = diff
        .char_indices()
        .take_while(|(i, _)| *i < MAX)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    format!(
        "{}\n\n[diff truncated at {} of {} bytes for review]",
        &diff[..cut],
        cut,
        diff.len()
    )
}

/// `reason` is stated verbatim rather than assumed. This used to always claim
/// "No planning model is configured", including when a model *was* configured
/// and the call failed — which sent people to fix settings that were already
/// correct while the actual cause (a retired model id, a 503, a rejected key)
/// appeared only in Core's log.
fn placeholder_plan(title: &str, task: &str, reason: &str) -> String {
    format!(
        "## Requirements\n\
         - {title}\n\
         - Derived from the session task below; no model output was available to expand it.\n\n\
         ## Approach\n\
         {reason}. This is a placeholder plan, recorded so the Session still has an \
         explicit, editable, approvable plan document. Edit it before approving.\n\n\
         ## Steps\n\
         1. Review and rewrite this plan by hand, or fix the model configuration and press Re-plan.\n\n\
         ---\n\
         Original task: {task}"
    )
}

/// Parse the Reviewer's `SEVERITY | file | description` lines. Lines that don't
/// match the shape are ignored rather than guessed at.
fn parse_findings(raw: &str) -> Vec<ReviewFinding> {
    if raw.trim().eq_ignore_ascii_case("NO FINDINGS") {
        return vec![];
    }
    raw.lines()
        .filter_map(|line| {
            let line = line.trim().trim_start_matches(['-', '*', ' ']);
            let parts: Vec<&str> = line.splitn(3, '|').map(|p| p.trim()).collect();
            if parts.len() != 3 || parts[2].is_empty() {
                return None;
            }
            let severity = match parts[0].to_ascii_lowercase().as_str() {
                "critical" => ReviewSeverity::Critical,
                "warning" => ReviewSeverity::Warning,
                "info" => ReviewSeverity::Info,
                _ => return None,
            };
            Some(ReviewFinding {
                severity,
                file: parts[1].to_string(),
                description: parts[2].to_string(),
            })
        })
        .collect()
}

fn verdict_for(findings: &[ReviewFinding], raw: &str) -> ReviewVerdict {
    if raw.starts_with("Reviewer could not run") || raw.starts_with("Reviewer not run") {
        return ReviewVerdict::NotRun;
    }
    if findings
        .iter()
        .any(|f| f.severity == ReviewSeverity::Critical)
    {
        ReviewVerdict::ChangesRequested
    } else if findings.is_empty() {
        ReviewVerdict::Clean
    } else {
        ReviewVerdict::CommentsOnly
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::IsolationMode;

    fn test_runner() -> (RoleRunner, Arc<Persistence>, String) {
        let persistence = Arc::new(Persistence::new_in_memory().unwrap());
        let model_manager = Arc::new(ModelManager::new(persistence.clone()));
        let runner = RoleRunner::new(persistence.clone(), model_manager);
        let repo = persistence
            .connect_repo("/tmp/vibe-test-repo", None)
            .unwrap();
        let session = persistence
            .create_session(
                &repo.id,
                "Quick fix",
                "Fix a typo in the README",
                IsolationMode::Shared,
                AutonomyLevel::CoPilot,
            )
            .unwrap();
        (runner, persistence, session.id)
    }

    #[test]
    fn vibe_plan_is_already_approved() {
        let (runner, _persistence, session_id) = test_runner();
        let plan = runner.generate_vibe_plan(&session_id).unwrap();

        assert_eq!(plan.status, SessionPlanStatus::Approved);
        assert_eq!(plan.approved_by.as_deref(), Some("vibe-preset"));
        assert!(runner.implementer_is_gated(&session_id).unwrap().is_none());
    }

    #[test]
    fn vibe_plan_includes_the_task_description() {
        let (runner, _persistence, session_id) = test_runner();
        let plan = runner.generate_vibe_plan(&session_id).unwrap();
        assert!(plan.content.contains("Fix a typo in the README"));
    }

    #[test]
    fn parses_well_formed_findings() {
        let raw = "critical | src/auth.rs | Token is compared with == instead of a constant-time compare\n\
                   warning | src/api.rs | New endpoint has no test";
        let findings = parse_findings(raw);
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].severity, ReviewSeverity::Critical);
        assert_eq!(findings[0].file, "src/auth.rs");
        assert_eq!(findings[1].severity, ReviewSeverity::Warning);
    }

    #[test]
    fn tolerates_markdown_bullets() {
        let findings = parse_findings("- info | README.md | Docs drifted from the code");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, ReviewSeverity::Info);
    }

    #[test]
    fn ignores_lines_that_do_not_match_the_shape() {
        let raw = "Here are my findings:\n\
                   critical | src/a.rs | Real finding\n\
                   Some trailing prose without pipes";
        assert_eq!(parse_findings(raw).len(), 1);
    }

    #[test]
    fn ignores_unknown_severity_rather_than_guessing() {
        assert!(parse_findings("nitpick | src/a.rs | style").is_empty());
    }

    #[test]
    fn no_findings_sentinel_yields_empty() {
        assert!(parse_findings("NO FINDINGS").is_empty());
    }

    #[test]
    fn verdict_escalates_on_critical() {
        let findings = parse_findings("critical | a.rs | boom");
        assert_eq!(
            verdict_for(&findings, "critical | a.rs | boom"),
            ReviewVerdict::ChangesRequested
        );
    }

    #[test]
    fn verdict_is_comments_only_without_critical() {
        let findings = parse_findings("warning | a.rs | meh");
        assert_eq!(
            verdict_for(&findings, "warning | a.rs | meh"),
            ReviewVerdict::CommentsOnly
        );
    }

    #[test]
    fn verdict_is_clean_with_no_findings() {
        assert_eq!(verdict_for(&[], "NO FINDINGS"), ReviewVerdict::Clean);
    }

    #[test]
    fn verdict_reports_not_run_when_the_model_was_unavailable() {
        let raw = "Reviewer not run: no model credentials configured for the Reviewer role.";
        assert_eq!(verdict_for(&[], raw), ReviewVerdict::NotRun);
    }

    #[test]
    fn truncate_keeps_short_diffs_intact() {
        let diff = "diff --git a/a b/a\n+one line";
        assert_eq!(truncate_diff(diff), diff);
    }

    #[test]
    fn truncate_marks_long_diffs() {
        let diff = "x".repeat(70_000);
        let out = truncate_diff(&diff);
        assert!(out.len() < diff.len());
        assert!(out.contains("diff truncated"));
    }

    #[test]
    fn placeholder_plan_has_all_three_sections() {
        let plan = placeholder_plan(
            "Add OAuth",
            "wire up login",
            "No planning model is configured",
        );
        assert!(plan.contains("## Requirements"));
        assert!(plan.contains("## Approach"));
        assert!(plan.contains("## Steps"));
        assert!(plan.contains("wire up login"));
    }

    /// A configured-but-failing model must not be reported as "not configured".
    /// That message sent people to re-do settings that were already correct
    /// while the real cause stayed in Core's log.
    #[test]
    fn a_failed_model_call_reports_the_real_reason_not_a_missing_config() {
        let plan = placeholder_plan(
            "Add OAuth",
            "wire up login",
            "The planning model was called but failed: Google returned 503 Service Unavailable",
        );
        assert!(plan.contains("503 Service Unavailable"), "got: {plan}");
        assert!(
            !plan.contains("No planning model is configured"),
            "must not claim the model is unconfigured when it was called"
        );
    }
}
