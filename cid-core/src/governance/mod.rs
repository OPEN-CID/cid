/*!
 * Workspace-level governance and policy (Phase 3, Part 14).
 *
 * Sits above the per-repo Autonomous-mode allow-lists in `autonomy`: those say
 * *which commands* may run unattended, this says *who may turn Autonomous mode
 * on at all*, *which repos permit it*, and *how much a Mission may spend*.
 *
 * Every decision returns a reason, so a refusal can be shown to the user and
 * written to the History panel as an audit entry rather than surfacing as a
 * bare "forbidden".
 */

use std::collections::HashMap;
use std::sync::RwLock;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::auth::{Role, Session};

/// Workspace policy. Defaults are deliberately restrictive: Autonomous mode is
/// off, no repo is allow-listed, and there is no spend budget until someone
/// sets one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspacePolicy {
    pub workspace_id: String,
    /// Minimum role permitted to place a Mission in Autonomous mode.
    pub min_role_for_autonomous: Role,
    /// Whether Autonomous mode is permitted anywhere in this Workspace.
    pub autonomous_enabled: bool,
    /// Repos where Autonomous mode is permitted. Empty means none.
    pub autonomous_repos: Vec<String>,
    /// Per-Mission spend ceiling in USD. `None` means unlimited.
    pub mission_spend_cap_usd: Option<f64>,
    /// Rolling daily spend ceiling for the whole Workspace, in USD.
    pub daily_spend_cap_usd: Option<f64>,
    /// Minimum role permitted to approve a plan.
    pub min_role_for_plan_approval: Role,
    /// Minimum role permitted to merge or open a PR.
    pub min_role_for_merge: Role,
    pub updated_at: DateTime<Utc>,
}

impl WorkspacePolicy {
    pub fn default_for(workspace_id: &str) -> Self {
        Self {
            workspace_id: workspace_id.to_string(),
            min_role_for_autonomous: Role::Admin,
            autonomous_enabled: false,
            autonomous_repos: Vec::new(),
            mission_spend_cap_usd: None,
            daily_spend_cap_usd: None,
            min_role_for_plan_approval: Role::Developer,
            min_role_for_merge: Role::Developer,
            updated_at: Utc::now(),
        }
    }
}

/// The outcome of a policy check. Always carries a reason.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum PolicyDecision {
    Allow { reason: String },
    Deny { reason: String },
}

impl PolicyDecision {
    pub fn allowed(&self) -> bool {
        matches!(self, PolicyDecision::Allow { .. })
    }

    pub fn reason(&self) -> &str {
        match self {
            PolicyDecision::Allow { reason } | PolicyDecision::Deny { reason } => reason,
        }
    }

    fn allow(reason: impl Into<String>) -> Self {
        PolicyDecision::Allow {
            reason: reason.into(),
        }
    }

    fn deny(reason: impl Into<String>) -> Self {
        PolicyDecision::Deny {
            reason: reason.into(),
        }
    }
}

/// A recorded spend event, used for cap enforcement and for the audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpendRecord {
    pub mission_id: String,
    pub workspace_id: String,
    pub usd: f64,
    pub at: DateTime<Utc>,
    pub note: Option<String>,
}

pub struct GovernanceManager {
    policies: RwLock<HashMap<String, WorkspacePolicy>>,
    spend: RwLock<Vec<SpendRecord>>,
}

impl Default for GovernanceManager {
    fn default() -> Self {
        Self::new()
    }
}

impl GovernanceManager {
    pub fn new() -> Self {
        Self {
            policies: RwLock::new(HashMap::new()),
            spend: RwLock::new(Vec::new()),
        }
    }

    pub fn get_policy(&self, workspace_id: &str) -> WorkspacePolicy {
        self.policies
            .read()
            .unwrap()
            .get(workspace_id)
            .cloned()
            .unwrap_or_else(|| WorkspacePolicy::default_for(workspace_id))
    }

    /// Replace a Workspace's policy. Only Admins and above may do this — the
    /// check is here rather than at the call site so no caller can skip it.
    pub fn set_policy(
        &self,
        actor: &Session,
        mut policy: WorkspacePolicy,
    ) -> Result<WorkspacePolicy> {
        crate::auth::require(actor, Role::Admin, "change Workspace policy")?;
        policy.updated_at = Utc::now();
        self.policies
            .write()
            .unwrap()
            .insert(policy.workspace_id.clone(), policy.clone());
        Ok(policy)
    }

    /// May this user put this Mission into Autonomous mode on this repo?
    ///
    /// All three conditions must hold: the Workspace permits Autonomous mode at
    /// all, the repo is on its list, and the user's role clears the bar.
    pub fn can_enable_autonomous(
        &self,
        actor: &Session,
        workspace_id: &str,
        repo_path: &str,
    ) -> PolicyDecision {
        let policy = self.get_policy(workspace_id);

        if !policy.autonomous_enabled {
            return PolicyDecision::deny(
                "Autonomous mode is disabled for this Workspace. An Admin can enable it in \
                 Workspace policy.",
            );
        }
        if !policy
            .autonomous_repos
            .iter()
            .any(|r| paths_match(r, repo_path))
        {
            return PolicyDecision::deny(format!(
                "This Workspace permits Autonomous mode, but '{repo_path}' is not on its \
                 allowed-repo list."
            ));
        }
        if !actor.role.satisfies(policy.min_role_for_autonomous) {
            return PolicyDecision::deny(format!(
                "Role '{}' cannot enable Autonomous mode; '{}' or higher is required.",
                actor.role.as_str(),
                policy.min_role_for_autonomous.as_str()
            ));
        }
        PolicyDecision::allow(format!(
            "'{}' may enable Autonomous mode on '{repo_path}'.",
            actor.username
        ))
    }

    pub fn can_approve_plan(&self, actor: &Session, workspace_id: &str) -> PolicyDecision {
        let policy = self.get_policy(workspace_id);
        if actor.role.satisfies(policy.min_role_for_plan_approval) {
            PolicyDecision::allow(format!("'{}' may approve plans.", actor.username))
        } else {
            PolicyDecision::deny(format!(
                "Role '{}' cannot approve plans; '{}' or higher is required.",
                actor.role.as_str(),
                policy.min_role_for_plan_approval.as_str()
            ))
        }
    }

    pub fn can_merge(&self, actor: &Session, workspace_id: &str) -> PolicyDecision {
        let policy = self.get_policy(workspace_id);
        if actor.role.satisfies(policy.min_role_for_merge) {
            PolicyDecision::allow(format!("'{}' may merge or open PRs.", actor.username))
        } else {
            PolicyDecision::deny(format!(
                "Role '{}' cannot merge; '{}' or higher is required.",
                actor.role.as_str(),
                policy.min_role_for_merge.as_str()
            ))
        }
    }

    /// Whether a Mission may spend `additional_usd` more.
    ///
    /// Checked before the spend, not after, so a cap actually prevents overrun
    /// rather than reporting it.
    pub fn check_spend(
        &self,
        workspace_id: &str,
        mission_id: &str,
        additional_usd: f64,
    ) -> PolicyDecision {
        let policy = self.get_policy(workspace_id);
        let spend = self.spend.read().unwrap();

        if let Some(cap) = policy.mission_spend_cap_usd {
            let so_far: f64 = spend
                .iter()
                .filter(|r| r.mission_id == mission_id)
                .map(|r| r.usd)
                .sum();
            if so_far + additional_usd > cap {
                return PolicyDecision::deny(format!(
                    "Mission spend cap reached: ${:.2} spent, ${:.2} requested, cap ${:.2}.",
                    so_far, additional_usd, cap
                ));
            }
        }

        if let Some(cap) = policy.daily_spend_cap_usd {
            let since = Utc::now() - chrono::Duration::hours(24);
            let today: f64 = spend
                .iter()
                .filter(|r| r.workspace_id == workspace_id && r.at >= since)
                .map(|r| r.usd)
                .sum();
            if today + additional_usd > cap {
                return PolicyDecision::deny(format!(
                    "Workspace daily spend cap reached: ${:.2} in the last 24h, ${:.2} \
                     requested, cap ${:.2}.",
                    today, additional_usd, cap
                ));
            }
        }

        PolicyDecision::allow("Within spend budget.")
    }

    pub fn record_spend(
        &self,
        workspace_id: &str,
        mission_id: &str,
        usd: f64,
        note: Option<String>,
    ) -> SpendRecord {
        let record = SpendRecord {
            mission_id: mission_id.to_string(),
            workspace_id: workspace_id.to_string(),
            usd,
            at: Utc::now(),
            note,
        };
        self.spend.write().unwrap().push(record.clone());
        record
    }

    pub fn mission_spend(&self, mission_id: &str) -> f64 {
        self.spend
            .read()
            .unwrap()
            .iter()
            .filter(|r| r.mission_id == mission_id)
            .map(|r| r.usd)
            .sum()
    }

    pub fn workspace_spend_24h(&self, workspace_id: &str) -> f64 {
        let since = Utc::now() - chrono::Duration::hours(24);
        self.spend
            .read()
            .unwrap()
            .iter()
            .filter(|r| r.workspace_id == workspace_id && r.at >= since)
            .map(|r| r.usd)
            .sum()
    }

    pub fn spend_records(&self, mission_id: Option<&str>) -> Vec<SpendRecord> {
        self.spend
            .read()
            .unwrap()
            .iter()
            .filter(|r| mission_id.map(|m| r.mission_id == m).unwrap_or(true))
            .cloned()
            .collect()
    }
}

/// Compare repo paths tolerantly across separator style, trailing slashes,
/// and OS-level path spelling (a policy entry typed by hand vs. a
/// `\\?\`-prefixed or 8.3-short-name form of the same directory that Windows
/// can hand back for the identical path) — same seam
/// `path_confine::normalize_stored_path` closed for `repo_channels.path`
/// itself; a policy's `autonomous_repos` list is compared against that
/// already-normalized column, so it needs the same canonicalization or a
/// hand-typed entry silently never matches.
fn paths_match(a: &str, b: &str) -> bool {
    fn norm(p: &str) -> String {
        crate::path_confine::normalize_stored_path(p)
            .trim_end_matches(['/', '\\'])
            .replace('\\', "/")
            .to_lowercase()
    }
    norm(a) == norm(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(role: Role, username: &str) -> Session {
        Session {
            token: "t".into(),
            user_id: format!("id-{username}"),
            username: username.into(),
            role,
            expires_at: Utc::now() + chrono::Duration::hours(1),
        }
    }

    fn permissive_policy(workspace: &str, repo: &str) -> WorkspacePolicy {
        WorkspacePolicy {
            autonomous_enabled: true,
            autonomous_repos: vec![repo.to_string()],
            ..WorkspacePolicy::default_for(workspace)
        }
    }

    #[test]
    fn autonomous_mode_is_off_by_default() {
        let gov = GovernanceManager::new();
        let decision = gov.can_enable_autonomous(&session(Role::Owner, "owner"), "ws", "/repo");
        assert!(
            !decision.allowed(),
            "default policy must not permit Autonomous mode"
        );
        assert!(
            decision.reason().contains("disabled"),
            "{}",
            decision.reason()
        );
    }

    #[test]
    fn autonomous_requires_the_repo_to_be_allow_listed() {
        let gov = GovernanceManager::new();
        gov.set_policy(
            &session(Role::Admin, "a"),
            permissive_policy("ws", "/allowed"),
        )
        .unwrap();

        assert!(gov
            .can_enable_autonomous(&session(Role::Admin, "a"), "ws", "/allowed")
            .allowed());

        let denied = gov.can_enable_autonomous(&session(Role::Admin, "a"), "ws", "/other");
        assert!(!denied.allowed());
        assert!(
            denied.reason().contains("allowed-repo list"),
            "{}",
            denied.reason()
        );
    }

    #[test]
    fn autonomous_requires_a_sufficient_role() {
        let gov = GovernanceManager::new();
        gov.set_policy(&session(Role::Admin, "a"), permissive_policy("ws", "/repo"))
            .unwrap();

        let denied = gov.can_enable_autonomous(&session(Role::Developer, "dev"), "ws", "/repo");
        assert!(!denied.allowed(), "default bar is Admin");
        assert!(denied.reason().contains("developer"), "{}", denied.reason());

        assert!(gov
            .can_enable_autonomous(&session(Role::Admin, "adm"), "ws", "/repo")
            .allowed());
    }

    #[test]
    fn repo_matching_tolerates_separator_and_case_differences() {
        let gov = GovernanceManager::new();
        gov.set_policy(
            &session(Role::Admin, "a"),
            permissive_policy("ws", "C:\\Projects\\App"),
        )
        .unwrap();

        assert!(gov
            .can_enable_autonomous(&session(Role::Admin, "a"), "ws", "c:/projects/app/")
            .allowed());
    }

    #[test]
    fn only_admins_can_change_policy() {
        let gov = GovernanceManager::new();
        let err = gov
            .set_policy(
                &session(Role::Developer, "dev"),
                permissive_policy("ws", "/repo"),
            )
            .unwrap_err();
        assert!(err.to_string().contains("Workspace policy"), "{err}");
        assert!(
            !gov.get_policy("ws").autonomous_enabled,
            "policy must not have changed"
        );
    }

    #[test]
    fn plan_approval_and_merge_bars_are_enforced() {
        let gov = GovernanceManager::new();
        let policy = WorkspacePolicy {
            min_role_for_plan_approval: Role::Reviewer,
            min_role_for_merge: Role::Admin,
            ..WorkspacePolicy::default_for("ws")
        };
        gov.set_policy(&session(Role::Owner, "o"), policy).unwrap();

        assert!(gov
            .can_approve_plan(&session(Role::Reviewer, "r"), "ws")
            .allowed());
        assert!(!gov
            .can_approve_plan(&session(Role::Viewer, "v"), "ws")
            .allowed());

        assert!(!gov
            .can_merge(&session(Role::Developer, "d"), "ws")
            .allowed());
        assert!(gov.can_merge(&session(Role::Admin, "a"), "ws").allowed());
    }

    #[test]
    fn no_spend_cap_means_no_limit() {
        let gov = GovernanceManager::new();
        assert!(gov.check_spend("ws", "m1", 1_000_000.0).allowed());
    }

    #[test]
    fn a_mission_spend_cap_blocks_before_the_overrun() {
        let gov = GovernanceManager::new();
        let policy = WorkspacePolicy {
            mission_spend_cap_usd: Some(10.0),
            ..WorkspacePolicy::default_for("ws")
        };
        gov.set_policy(&session(Role::Admin, "a"), policy).unwrap();

        gov.record_spend("ws", "m1", 8.0, None);
        assert!(gov.check_spend("ws", "m1", 1.5).allowed());

        let denied = gov.check_spend("ws", "m1", 5.0);
        assert!(!denied.allowed(), "the cap must be checked before spending");
        assert!(
            denied.reason().contains("Mission spend cap"),
            "{}",
            denied.reason()
        );
    }

    #[test]
    fn a_mission_cap_does_not_leak_across_missions() {
        let gov = GovernanceManager::new();
        let policy = WorkspacePolicy {
            mission_spend_cap_usd: Some(10.0),
            ..WorkspacePolicy::default_for("ws")
        };
        gov.set_policy(&session(Role::Admin, "a"), policy).unwrap();

        gov.record_spend("ws", "m1", 9.9, None);
        assert!(
            gov.check_spend("ws", "m2", 9.0).allowed(),
            "each Mission has its own cap"
        );
    }

    #[test]
    fn a_daily_workspace_cap_aggregates_across_missions() {
        let gov = GovernanceManager::new();
        let policy = WorkspacePolicy {
            daily_spend_cap_usd: Some(20.0),
            ..WorkspacePolicy::default_for("ws")
        };
        gov.set_policy(&session(Role::Admin, "a"), policy).unwrap();

        gov.record_spend("ws", "m1", 12.0, None);
        gov.record_spend("ws", "m2", 7.0, None);

        let denied = gov.check_spend("ws", "m3", 5.0);
        assert!(!denied.allowed());
        assert!(
            denied.reason().contains("daily spend cap"),
            "{}",
            denied.reason()
        );
    }

    #[test]
    fn spend_in_another_workspace_does_not_count_against_this_one() {
        let gov = GovernanceManager::new();
        let policy = WorkspacePolicy {
            daily_spend_cap_usd: Some(20.0),
            ..WorkspacePolicy::default_for("ws")
        };
        gov.set_policy(&session(Role::Admin, "a"), policy).unwrap();

        gov.record_spend("other-ws", "m1", 100.0, None);
        assert!(gov.check_spend("ws", "m2", 10.0).allowed());
    }

    #[test]
    fn spend_totals_are_queryable_for_the_audit_trail() {
        let gov = GovernanceManager::new();
        gov.record_spend("ws", "m1", 2.5, Some("planner".into()));
        gov.record_spend("ws", "m1", 1.5, Some("implementer".into()));
        gov.record_spend("ws", "m2", 4.0, None);

        assert!((gov.mission_spend("m1") - 4.0).abs() < 1e-9);
        assert!((gov.workspace_spend_24h("ws") - 8.0).abs() < 1e-9);
        assert_eq!(gov.spend_records(Some("m1")).len(), 2);
        assert_eq!(gov.spend_records(None).len(), 3);
    }

    #[test]
    fn every_decision_carries_a_usable_reason() {
        let gov = GovernanceManager::new();
        for decision in [
            gov.can_enable_autonomous(&session(Role::Viewer, "v"), "ws", "/r"),
            gov.can_approve_plan(&session(Role::Viewer, "v"), "ws"),
            gov.can_merge(&session(Role::Developer, "d"), "ws"),
            gov.check_spend("ws", "m", 1.0),
        ] {
            assert!(
                decision.reason().len() > 10,
                "a decision must explain itself: {:?}",
                decision
            );
        }
    }
}
