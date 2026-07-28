/*!
 * Configurable role profiles (Phase 4, Part A).
 *
 * The Phase 4 brief's own resolution of the "AI Operating System" proposal:
 * kept as configurable profiles on top of the existing three-role engine
 * (Part 5), not ten independent agent subsystems with their own memory and
 * budget. A profile is still exactly a prompt + tool-permission set + model
 * config running through the same Mission, worktree, and model router as
 * Planner/Implementer/Reviewer — never its own subsystem.
 *
 * A Mission's Planner can invoke a profile as an additional scoped subagent
 * when the task calls for it ("this touches auth code — also run the
 * Security Reviewer profile"), sharing the parent Mission's worktree per the
 * same rule Phase 2's subagents already follow.
 */

use std::sync::Arc;

use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::persistence::Persistence;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileScope {
    Workspace,
    Repo,
}

/// The tools a profile may use. Distinct from the Autonomous-mode command
/// allow-list (Part 14) — this restricts *tool categories* a profile may
/// invoke at all, checked before a call reaches that finer-grained gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ToolPermission {
    ReadFile,
    WriteFile,
    RunTerminal,
    GitOps,
    McpTools,
}

impl ToolPermission {
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolPermission::ReadFile => "read_file",
            ToolPermission::WriteFile => "write_file",
            ToolPermission::RunTerminal => "run_terminal",
            ToolPermission::GitOps => "git_ops",
            ToolPermission::McpTools => "mcp_tools",
        }
    }

    /// Map a real tool-call name (as used by the model router's tool loop) to
    /// the permission category it falls under. Unrecognized tool names are
    /// treated as `WriteFile` — the most restrictive real category — so an
    /// unknown tool fails closed rather than silently bypassing the check.
    pub fn for_tool_name(tool_name: &str) -> ToolPermission {
        match tool_name {
            "read_file" | "list_files" | "git_status" | "git_diff" => ToolPermission::ReadFile,
            "write_file" | "edit_file" => ToolPermission::WriteFile,
            "run_terminal" => ToolPermission::RunTerminal,
            "git_commit" => ToolPermission::GitOps,
            name if name.starts_with("mcp_") => ToolPermission::McpTools,
            _ => ToolPermission::WriteFile,
        }
    }
}

/// A named role profile: a prompt, a model config, and the tool categories it
/// may use — nothing more. Enforcement lives in `PermissionCheck`, not here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleProfile {
    pub id: String,
    pub name: String,
    pub description: String,
    pub scope: ProfileScope,
    /// Workspace id or Repo Channel id, depending on `scope`.
    pub scope_id: String,
    pub system_prompt: String,
    pub model_provider: Option<String>,
    pub model_id: Option<String>,
    pub allowed_tools: Vec<ToolPermission>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleProfileInput {
    pub name: String,
    pub description: String,
    pub scope: ProfileScope,
    pub scope_id: String,
    pub system_prompt: String,
    pub model_provider: Option<String>,
    pub model_id: Option<String>,
    pub allowed_tools: Vec<ToolPermission>,
}

pub struct RoleProfileManager {
    persistence: Arc<Persistence>,
}

impl RoleProfileManager {
    pub fn new(persistence: Arc<Persistence>) -> Self {
        Self { persistence }
    }

    pub fn create(&self, input: RoleProfileInput) -> Result<RoleProfile> {
        validate(&input)?;
        self.persistence.create_role_profile(input)
    }

    pub fn update(&self, id: &str, input: RoleProfileInput) -> Result<RoleProfile> {
        validate(&input)?;
        self.persistence.update_role_profile(id, input)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        self.persistence.delete_role_profile(id)
    }

    pub fn get(&self, id: &str) -> Result<RoleProfile> {
        self.persistence.get_role_profile(id)
    }

    /// List profiles visible to a Repo Channel: its own repo-scoped profiles
    /// plus every Workspace-scoped profile, nearest-scope-first — the same
    /// resolution order Part 12's Skills layering already uses.
    pub fn list_for_repo(
        &self,
        workspace_id: &str,
        repo_channel_id: &str,
    ) -> Result<Vec<RoleProfile>> {
        let mut repo_scoped = self
            .persistence
            .list_role_profiles(ProfileScope::Repo, repo_channel_id)?;
        let workspace_scoped = self
            .persistence
            .list_role_profiles(ProfileScope::Workspace, workspace_id)?;
        repo_scoped.extend(workspace_scoped);
        Ok(repo_scoped)
    }

    /// Whether `profile` may invoke `tool_name`. This is the enforcement path
    /// — a restricted profile must actually be restricted, not merely display
    /// a restriction in its settings.
    pub fn check_permission(&self, profile: &RoleProfile, tool_name: &str) -> PermissionCheck {
        check_tool_permission(profile, tool_name)
    }
}

/// Free-function form of the same check, callable from the tool-execution
/// path (`model::execute_tool_direct_in`) without needing a manager instance
/// or persistence access — this logic is pure.
pub fn check_tool_permission(profile: &RoleProfile, tool_name: &str) -> PermissionCheck {
    let required = ToolPermission::for_tool_name(tool_name);
    if profile.allowed_tools.contains(&required) {
        PermissionCheck::Allowed
    } else {
        PermissionCheck::Denied {
            reason: format!(
                "Profile '{}' is not permitted to use '{}' (requires {} permission)",
                profile.name,
                tool_name,
                required.as_str()
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PermissionCheck {
    Allowed,
    Denied { reason: String },
}

impl PermissionCheck {
    pub fn is_allowed(&self) -> bool {
        matches!(self, PermissionCheck::Allowed)
    }
}

fn validate(input: &RoleProfileInput) -> Result<()> {
    if input.name.trim().is_empty() {
        bail!("Profile name must not be empty");
    }
    if input.system_prompt.trim().is_empty() {
        bail!("Profile system_prompt must not be empty");
    }
    if input.scope_id.trim().is_empty() {
        bail!("Profile scope_id must not be empty");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager() -> RoleProfileManager {
        RoleProfileManager::new(Arc::new(
            crate::persistence::Persistence::new_in_memory().unwrap(),
        ))
    }

    fn input(
        name: &str,
        scope: ProfileScope,
        scope_id: &str,
        tools: Vec<ToolPermission>,
    ) -> RoleProfileInput {
        RoleProfileInput {
            name: name.to_string(),
            description: "test profile".to_string(),
            scope,
            scope_id: scope_id.to_string(),
            system_prompt: "You are a specialized reviewer.".to_string(),
            model_provider: Some("anthropic".to_string()),
            model_id: Some("claude-3-5-sonnet".to_string()),
            allowed_tools: tools,
        }
    }

    #[test]
    fn creates_and_fetches_a_profile() {
        let mgr = manager();
        let created = mgr
            .create(input(
                "Security Reviewer",
                ProfileScope::Workspace,
                "ws-1",
                vec![ToolPermission::ReadFile],
            ))
            .unwrap();
        assert_eq!(created.name, "Security Reviewer");

        let fetched = mgr.get(&created.id).unwrap();
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.allowed_tools, vec![ToolPermission::ReadFile]);
    }

    #[test]
    fn rejects_a_profile_with_no_name_or_no_prompt() {
        let mgr = manager();
        let mut bad = input("", ProfileScope::Workspace, "ws-1", vec![]);
        assert!(mgr.create(bad.clone()).is_err());

        bad.name = "Has Name".to_string();
        bad.system_prompt = "".to_string();
        assert!(mgr.create(bad).is_err());
    }

    #[test]
    fn listing_for_a_repo_includes_repo_and_workspace_scoped_profiles() {
        let mgr = manager();
        mgr.create(input("Repo Profile", ProfileScope::Repo, "repo-1", vec![]))
            .unwrap();
        mgr.create(input(
            "Workspace Profile",
            ProfileScope::Workspace,
            "ws-1",
            vec![],
        ))
        .unwrap();
        mgr.create(input(
            "Other Repo Profile",
            ProfileScope::Repo,
            "repo-2",
            vec![],
        ))
        .unwrap();

        let visible = mgr.list_for_repo("ws-1", "repo-1").unwrap();
        let names: Vec<&str> = visible.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"Repo Profile"));
        assert!(names.contains(&"Workspace Profile"));
        assert!(
            !names.contains(&"Other Repo Profile"),
            "a different repo's profile must not leak in"
        );
    }

    #[test]
    fn updating_a_profile_persists_the_change() {
        let mgr = manager();
        let created = mgr
            .create(input("Draft Name", ProfileScope::Workspace, "ws-1", vec![]))
            .unwrap();

        let mut updated_input = input(
            "Final Name",
            ProfileScope::Workspace,
            "ws-1",
            vec![ToolPermission::ReadFile],
        );
        updated_input.description = "updated".to_string();
        let updated = mgr.update(&created.id, updated_input).unwrap();
        assert_eq!(updated.name, "Final Name");

        let fetched = mgr.get(&created.id).unwrap();
        assert_eq!(fetched.name, "Final Name");
        assert_eq!(fetched.allowed_tools, vec![ToolPermission::ReadFile]);
    }

    #[test]
    fn deleting_a_profile_removes_it() {
        let mgr = manager();
        let created = mgr
            .create(input("Temp", ProfileScope::Workspace, "ws-1", vec![]))
            .unwrap();
        mgr.delete(&created.id).unwrap();
        assert!(mgr.get(&created.id).is_err());
    }

    // ---- Permission enforcement — the part that must actually restrict ----

    #[test]
    fn a_profile_without_write_permission_is_denied_a_write_call() {
        let mgr = manager();
        let profile = mgr
            .create(input(
                "Read-Only Reviewer",
                ProfileScope::Workspace,
                "ws-1",
                vec![ToolPermission::ReadFile],
            ))
            .unwrap();

        let check = mgr.check_permission(&profile, "write_file");
        assert!(
            !check.is_allowed(),
            "a read-only profile must not be allowed to write"
        );
        if let PermissionCheck::Denied { reason } = check {
            assert!(reason.contains("write_file"));
        } else {
            panic!("expected Denied");
        }
    }

    #[test]
    fn a_profile_with_write_permission_is_allowed_a_write_call() {
        let mgr = manager();
        let profile = mgr
            .create(input(
                "Implementer Profile",
                ProfileScope::Workspace,
                "ws-1",
                vec![ToolPermission::ReadFile, ToolPermission::WriteFile],
            ))
            .unwrap();
        assert!(mgr.check_permission(&profile, "write_file").is_allowed());
        assert!(mgr.check_permission(&profile, "edit_file").is_allowed());
    }

    #[test]
    fn terminal_and_git_permissions_are_independent_of_write() {
        let mgr = manager();
        let profile = mgr
            .create(input(
                "Writer Only",
                ProfileScope::Workspace,
                "ws-1",
                vec![ToolPermission::WriteFile],
            ))
            .unwrap();
        assert!(!mgr.check_permission(&profile, "run_terminal").is_allowed());
        assert!(!mgr.check_permission(&profile, "git_commit").is_allowed());
    }

    #[test]
    fn an_unrecognized_tool_name_fails_closed() {
        let mgr = manager();
        let profile = mgr
            .create(input(
                "Broad Profile",
                ProfileScope::Workspace,
                "ws-1",
                vec![ToolPermission::ReadFile, ToolPermission::RunTerminal],
            ))
            .unwrap();
        // An unrecognized tool name maps to WriteFile (the strictest real
        // category), which this profile does not have — must deny, not allow
        // by default.
        assert!(!mgr
            .check_permission(&profile, "some_future_tool_nobody_named_yet")
            .is_allowed());
    }

    #[test]
    fn mcp_tool_calls_require_the_mcp_permission_specifically() {
        let mgr = manager();
        let profile = mgr
            .create(input(
                "No MCP",
                ProfileScope::Workspace,
                "ws-1",
                vec![ToolPermission::ReadFile, ToolPermission::WriteFile],
            ))
            .unwrap();
        assert!(!mgr.check_permission(&profile, "mcp_call_jira").is_allowed());

        let with_mcp = mgr
            .create(input(
                "With MCP",
                ProfileScope::Workspace,
                "ws-1",
                vec![ToolPermission::McpTools],
            ))
            .unwrap();
        assert!(mgr
            .check_permission(&with_mcp, "mcp_call_jira")
            .is_allowed());
    }

    #[test]
    fn tool_name_mapping_is_stable_for_every_real_tool_the_agent_loop_uses() {
        assert_eq!(
            ToolPermission::for_tool_name("read_file"),
            ToolPermission::ReadFile
        );
        assert_eq!(
            ToolPermission::for_tool_name("list_files"),
            ToolPermission::ReadFile
        );
        assert_eq!(
            ToolPermission::for_tool_name("git_status"),
            ToolPermission::ReadFile
        );
        assert_eq!(
            ToolPermission::for_tool_name("git_diff"),
            ToolPermission::ReadFile
        );
        assert_eq!(
            ToolPermission::for_tool_name("write_file"),
            ToolPermission::WriteFile
        );
        assert_eq!(
            ToolPermission::for_tool_name("edit_file"),
            ToolPermission::WriteFile
        );
        assert_eq!(
            ToolPermission::for_tool_name("run_terminal"),
            ToolPermission::RunTerminal
        );
        assert_eq!(
            ToolPermission::for_tool_name("git_commit"),
            ToolPermission::GitOps
        );
    }
}
