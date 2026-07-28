//! Phase 1 Autonomy Manager
//!
//! Manages per-Mission autonomy levels, command allow-lists for Autonomous mode,
//! and role-to-model resolution for Planner/Implementer/Reviewer.
//!
//! Core concepts per Part 5 and Part 14:
//! - AutonomyLevel: Manual | CoPilot | Autonomous
//! - Command allow-lists: patterns scoped per Workspace/Repo Channel
//! - Path restrictions: allowed_paths + denied_paths for Autonomous mode
//! - Tool-call budgets: max_tool_calls limit
//!
//! Design:
//! - Allow-lists stored in-memory, loaded from persistence
//! - Regex-based command pattern matching
//! - Glob-based path restriction checking
//! - Role resolution via model/provider config

use std::collections::HashMap;
use std::sync::RwLock;

use crate::api::types::{AllowedCommand, AutonomyAllowlist, AutonomyCheckResult, AutonomyLevel};
use chrono::Utc;
use regex::Regex;

pub struct AutonomyManager {
    allowlists: RwLock<HashMap<String, AutonomyAllowlist>>,
}

impl Default for AutonomyManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AutonomyManager {
    pub fn new() -> Self {
        Self {
            allowlists: RwLock::new(HashMap::new()),
        }
    }

    // -----------------------------------------------------------------------
    // Allow-list management
    // -----------------------------------------------------------------------

    /// Get the allowlist for a given scope (workspace/repo id).
    pub fn get_allowlist(&self, scope_id: &str) -> Option<AutonomyAllowlist> {
        let map = self.allowlists.read().ok()?;
        map.get(scope_id).cloned()
    }

    /// Set or update the allowlist for a scope.
    pub fn set_allowlist(
        &self,
        scope_id: &str,
        allowed_commands: Vec<AllowedCommand>,
        allowed_paths: Vec<String>,
        denied_paths: Vec<String>,
        max_tool_calls: Option<usize>,
    ) -> AutonomyAllowlist {
        let mut map = self.allowlists.write().unwrap();
        let existing = map.get(scope_id);
        let now = Utc::now();

        let allowlist = AutonomyAllowlist {
            id: existing
                .map(|e| e.id.clone())
                .unwrap_or_else(crate::api::types::new_id),
            scope: "repo".to_string(),
            scope_id: scope_id.to_string(),
            allowed_commands,
            allowed_paths,
            denied_paths,
            max_tool_calls,
            created_at: existing.map(|e| e.created_at).unwrap_or(now),
            updated_at: now,
        };

        map.insert(scope_id.to_string(), allowlist.clone());
        allowlist
    }

    /// Remove an allowlist for a scope.
    pub fn remove_allowlist(&self, scope_id: &str) -> bool {
        let mut map = self.allowlists.write().unwrap();
        map.remove(scope_id).is_some()
    }

    /// List all allowlists.
    pub fn list_allowlists(&self) -> Vec<AutonomyAllowlist> {
        let map = self.allowlists.read().unwrap();
        map.values().cloned().collect()
    }

    // -----------------------------------------------------------------------
    // Command checking
    // -----------------------------------------------------------------------

    /// Check whether a terminal command is allowed under Autonomous mode.
    /// Considers:
    /// 1. Command pattern matching against allowed_commands
    /// 2. Path restrictions (if command references paths)
    /// 3. Whether the matched pattern requires explicit approval
    pub fn check_command(
        &self,
        scope_id: &str,
        command: &str,
        workdir: Option<&str>,
    ) -> AutonomyCheckResult {
        let allowlist = match self.get_allowlist(scope_id) {
            Some(a) => a,
            None => {
                return AutonomyCheckResult {
                    allowed: false,
                    reason: format!("No autonomy allowlist configured for scope '{}'", scope_id),
                    requires_approval: true,
                    matched_pattern: None,
                }
            }
        };

        let trimmed = command.trim();

        // Check against allowed command patterns
        let mut matched = false;
        let mut matched_pattern = None;
        let mut requires_approval = false;

        for allowed in &allowlist.allowed_commands {
            let pattern = &allowed.pattern;
            // Compile regex pattern. If compile fails, treat as literal prefix match.
            let pattern_matches = match Regex::new(pattern) {
                Ok(re) => re.is_match(trimmed),
                Err(_) => trimmed.starts_with(pattern),
            };

            if pattern_matches {
                matched = true;
                matched_pattern = Some(pattern.clone());
                requires_approval = allowed.requires_approval;
                break;
            }
        }

        if !matched {
            return AutonomyCheckResult {
                allowed: false,
                reason: format!(
                    "Command '{}' does not match any allowed pattern for scope '{}'",
                    trimmed, scope_id
                ),
                requires_approval: true,
                matched_pattern: None,
            };
        }

        // Check paths referenced in command against restrictions
        let path_check = self.check_command_paths(
            trimmed,
            workdir,
            &allowlist.allowed_paths,
            &allowlist.denied_paths,
        );

        if !path_check.allowed {
            return path_check;
        }

        AutonomyCheckResult {
            allowed: true,
            reason: format!(
                "Command '{}' matches allowed pattern '{}'",
                trimmed,
                matched_pattern.as_deref().unwrap_or("")
            ),
            requires_approval,
            matched_pattern,
        }
    }

    /// Check if any paths referenced in the command are within allowed directories.
    fn check_command_paths(
        &self,
        command: &str,
        workdir: Option<&str>,
        allowed_paths: &[String],
        denied_paths: &[String],
    ) -> AutonomyCheckResult {
        // Extract potential paths from the command.
        // For Phase 1, we use a simple heuristic: look for arguments that look like paths.
        let potential_paths = self.extract_potential_paths(command);

        // Denied paths check (most restrictive first)
        for path in &potential_paths {
            for denied in denied_paths {
                if path.starts_with(denied) || self.glob_matches(path, denied) {
                    return AutonomyCheckResult {
                        allowed: false,
                        reason: format!("Path '{}' matches denied pattern '{}'", path, denied),
                        requires_approval: true,
                        matched_pattern: None,
                    };
                }
            }
        }

        // If allowed_paths is set, check at least one path matches (or workdir is in allowed paths)
        if !allowed_paths.is_empty() && !potential_paths.is_empty() {
            let all_in_allowed = potential_paths.iter().all(|path| {
                allowed_paths
                    .iter()
                    .any(|allowed| path.starts_with(allowed) || self.glob_matches(path, allowed))
            });

            if !all_in_allowed {
                // Also check if workdir is allowed (common case: working in a worktree)
                if let Some(wd) = workdir {
                    let wd_allowed = allowed_paths
                        .iter()
                        .any(|a| wd.starts_with(a) || self.glob_matches(wd, a));
                    if !wd_allowed {
                        return AutonomyCheckResult {
                            allowed: false,
                            reason: format!(
                                "Paths in command are outside allowed directories. Allowed: {:?}",
                                allowed_paths
                            ),
                            requires_approval: true,
                            matched_pattern: None,
                        };
                    }
                } else {
                    return AutonomyCheckResult {
                        allowed: false,
                        reason: format!(
                            "Paths in command are outside allowed directories. Allowed: {:?}",
                            allowed_paths
                        ),
                        requires_approval: true,
                        matched_pattern: None,
                    };
                }
            }
        }

        AutonomyCheckResult {
            allowed: true,
            reason: "Paths are within allowed directories".to_string(),
            requires_approval: false,
            matched_pattern: None,
        }
    }

    /// Simple glob matching (supports * and **)
    fn glob_matches(&self, path: &str, pattern: &str) -> bool {
        if pattern.contains('*') {
            let re_str = pattern
                .replace("**", "___DOUBLE_STAR___")
                .replace('*', "[^/\\\\]*")
                .replace("___DOUBLE_STAR___", ".*");
            if let Ok(re) = Regex::new(&format!("^{}$", re_str)) {
                return re.is_match(path);
            }
        }
        false
    }

    /// Extract potential file paths from a command string.
    fn extract_potential_paths(&self, command: &str) -> Vec<String> {
        let mut paths = Vec::new();
        let parts: Vec<&str> = command.split_whitespace().collect();

        for part in parts {
            let trimmed = part.trim_matches(|c| c == '"' || c == '\'' || c == '`');
            // Heuristic: looks like a path if it contains / or \\ or starts with .
            if trimmed.contains('/')
                || trimmed.contains('\\')
                || trimmed.starts_with('.')
                || trimmed.starts_with('/')
                || trimmed.starts_with("~/")
            {
                paths.push(trimmed.to_string());
            }
        }

        paths
    }

    // -----------------------------------------------------------------------
    // Tool-call budget enforcement
    // -----------------------------------------------------------------------

    /// Check if the mission has exceeded its max tool call budget.
    pub fn check_budget(&self, scope_id: &str, current_tool_calls: usize) -> AutonomyCheckResult {
        let allowlist = match self.get_allowlist(scope_id) {
            Some(a) => a,
            None => {
                return AutonomyCheckResult {
                    allowed: true,
                    reason: "No budget configured".to_string(),
                    requires_approval: false,
                    matched_pattern: None,
                }
            }
        };

        if let Some(max) = allowlist.max_tool_calls {
            if current_tool_calls >= max {
                return AutonomyCheckResult {
                    allowed: false,
                    reason: format!(
                        "Tool call budget exhausted: {}/{} tool calls used",
                        current_tool_calls, max
                    ),
                    requires_approval: true,
                    matched_pattern: None,
                };
            }
        }

        AutonomyCheckResult {
            allowed: true,
            reason: format!(
                "Budget OK: {}/{:?} tool calls",
                current_tool_calls, allowlist.max_tool_calls
            ),
            requires_approval: false,
            matched_pattern: None,
        }
    }

    // -----------------------------------------------------------------------
    // Default allow-lists
    // -----------------------------------------------------------------------

    /// Create a sensible default allowlist for a repo.
    /// Allows common development commands but blocks destructive operations.
    pub fn create_default_allowlist(&self, scope_id: &str) -> AutonomyAllowlist {
        self.set_allowlist(
            scope_id,
            vec![
                AllowedCommand {
                    pattern: r"^(ls|dir|pwd)$".to_string(),
                    description: Some("List directory contents".to_string()),
                    requires_approval: false,
                },
                AllowedCommand {
                    pattern: r"^cat ".to_string(),
                    description: Some("Read file contents".to_string()),
                    requires_approval: false,
                },
                AllowedCommand {
                    pattern: r"^(cargo|npm|yarn|pnpm|pip|poetry|go|rustc|make|cmake|npx) "
                        .to_string(),
                    description: Some("Build and package manager commands".to_string()),
                    requires_approval: false,
                },
                AllowedCommand {
                    pattern: r"^git (status|diff|log|branch|add|commit) ".to_string(),
                    description: Some("Safe git operations".to_string()),
                    requires_approval: false,
                },
                AllowedCommand {
                    pattern: r"^(echo|printf|print) ".to_string(),
                    description: Some("Print/output commands".to_string()),
                    requires_approval: false,
                },
                AllowedCommand {
                    pattern: r"^(cargo test|npm test|yarn test|pytest|go test) ".to_string(),
                    description: Some("Run tests".to_string()),
                    requires_approval: false,
                },
                AllowedCommand {
                    pattern: r"^git (push|pull|fetch|merge|rebase) ".to_string(),
                    description: Some("Git operations that modify remotes".to_string()),
                    requires_approval: true,
                },
            ],
            vec![
                // Default allowed paths - worktree directory etc
            ],
            vec![
                "/etc/".to_string(),
                "/boot/".to_string(),
                "C:\\Windows\\".to_string(),
                "C:\\Windows\\System32\\".to_string(),
                "/System/".to_string(),
                "**/.git/config".to_string(),
            ],
            Some(200),
        )
    }

    // -----------------------------------------------------------------------
    // Autonomy level helpers
    // -----------------------------------------------------------------------

    /// Determine if an action requires approval based on the current autonomy level.
    pub fn requires_approval(
        &self,
        level: &AutonomyLevel,
        scope_id: &str,
        command: &str,
        is_mcp_tool: bool,
    ) -> bool {
        match level {
            AutonomyLevel::Manual => true,
            AutonomyLevel::CoPilot => true,
            AutonomyLevel::Autonomous => {
                if is_mcp_tool {
                    // MCP tools always require extra scrutiny
                    true
                } else {
                    let check = self.check_command(scope_id, command, None);
                    check.requires_approval || !check.allowed
                }
            }
        }
    }

    /// Validate that a level transition is allowed.
    pub fn can_transition_to(
        &self,
        from: &AutonomyLevel,
        to: &AutonomyLevel,
        has_allowlist: bool,
    ) -> bool {
        match (from, to) {
            (_, AutonomyLevel::Manual) => true,
            (AutonomyLevel::Manual, _) => true,
            (AutonomyLevel::CoPilot, AutonomyLevel::Autonomous) => has_allowlist,
            (AutonomyLevel::CoPilot, AutonomyLevel::CoPilot) => true,
            (AutonomyLevel::Autonomous, _) => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_autonomy_manager() {
        let mgr = AutonomyManager::new();
        assert!(mgr.list_allowlists().is_empty());
    }

    #[test]
    fn test_set_and_get_allowlist() {
        let mgr = AutonomyManager::new();
        mgr.set_allowlist(
            "repo-1",
            vec![AllowedCommand {
                pattern: r"^cargo build".to_string(),
                description: Some("Build".to_string()),
                requires_approval: false,
            }],
            vec![],
            vec![],
            Some(100),
        );

        let al = mgr.get_allowlist("repo-1").unwrap();
        assert_eq!(al.allowed_commands.len(), 1);
        assert_eq!(al.max_tool_calls, Some(100));
    }

    #[test]
    fn test_remove_allowlist() {
        let mgr = AutonomyManager::new();
        mgr.create_default_allowlist("repo-1");
        assert!(mgr.get_allowlist("repo-1").is_some());
        assert!(mgr.remove_allowlist("repo-1"));
        assert!(mgr.get_allowlist("repo-1").is_none());
    }

    #[test]
    fn test_check_command_allowed() {
        let mgr = AutonomyManager::new();
        mgr.create_default_allowlist("repo-1");

        let result = mgr.check_command("repo-1", "cargo build", None);
        assert!(
            result.allowed,
            "Should allow 'cargo build': {}",
            result.reason
        );
        assert!(!result.requires_approval);
    }

    #[test]
    fn test_check_command_requires_approval() {
        let mgr = AutonomyManager::new();
        mgr.create_default_allowlist("repo-1");

        let result = mgr.check_command("repo-1", "git push origin main", None);
        assert!(result.allowed, "Should allow 'git push': {}", result.reason);
        assert!(result.requires_approval, "Git push should require approval");
    }

    #[test]
    fn test_check_command_not_allowed() {
        let mgr = AutonomyManager::new();
        mgr.create_default_allowlist("repo-1");

        let result = mgr.check_command("repo-1", "rm -rf /", None);
        assert!(!result.allowed, "Should deny 'rm -rf /'");
        assert!(result.requires_approval);
    }

    #[test]
    fn test_check_command_no_allowlist() {
        let mgr = AutonomyManager::new();
        let result = mgr.check_command("unknown-repo", "ls", None);
        assert!(!result.allowed);
        assert!(result.requires_approval);
    }

    #[test]
    fn test_check_budget() {
        let mgr = AutonomyManager::new();
        mgr.create_default_allowlist("repo-1");

        let result = mgr.check_budget("repo-1", 50);
        assert!(result.allowed);

        let result = mgr.check_budget("repo-1", 200);
        assert!(!result.allowed);
        assert!(result.requires_approval);
    }

    #[test]
    fn test_check_budget_no_limit() {
        let mgr = AutonomyManager::new();
        mgr.set_allowlist("repo-1", vec![], vec![], vec![], None);

        let result = mgr.check_budget("repo-1", 99999);
        assert!(result.allowed);
    }

    #[test]
    fn test_check_command_paths_denied() {
        let mgr = AutonomyManager::new();
        mgr.set_allowlist(
            "repo-1",
            vec![AllowedCommand {
                pattern: r"^cat ".to_string(),
                description: Some("Read file".to_string()),
                requires_approval: false,
            }],
            vec!["/safe/".to_string()],
            vec!["/etc/".to_string()],
            None,
        );

        let result = mgr.check_command("repo-1", "cat /etc/passwd", None);
        assert!(!result.allowed, "Should deny access to /etc/");
    }

    #[test]
    fn test_extract_potential_paths() {
        let mgr = AutonomyManager::new();
        let paths = mgr.extract_potential_paths(
            "cargo build --manifest-path ./Cargo.toml /absolute/path /etc/hosts",
        );
        assert!(paths.contains(&"./Cargo.toml".to_string()));
        assert!(paths.contains(&"/absolute/path".to_string()));
        assert!(paths.contains(&"/etc/hosts".to_string()));
    }

    #[test]
    fn test_requires_approval_manual() {
        let mgr = AutonomyManager::new();
        mgr.create_default_allowlist("repo-1");
        assert!(mgr.requires_approval(&AutonomyLevel::Manual, "repo-1", "ls", false));
    }

    #[test]
    fn test_requires_approval_copilot() {
        let mgr = AutonomyManager::new();
        mgr.create_default_allowlist("repo-1");
        assert!(mgr.requires_approval(&AutonomyLevel::CoPilot, "repo-1", "ls", false));
    }

    #[test]
    fn test_requires_approval_autonomous_allowed_command() {
        let mgr = AutonomyManager::new();
        mgr.create_default_allowlist("repo-1");
        assert!(!mgr.requires_approval(&AutonomyLevel::Autonomous, "repo-1", "cargo build", false));
    }

    #[test]
    fn test_requires_approval_autonomous_mcp_tool() {
        let mgr = AutonomyManager::new();
        mgr.create_default_allowlist("repo-1");
        assert!(mgr.requires_approval(&AutonomyLevel::Autonomous, "repo-1", "ls", true));
    }

    #[test]
    fn test_can_transition_to() {
        let mgr = AutonomyManager::new();
        mgr.create_default_allowlist("repo-1");

        // Manual can upgrade anywhere
        assert!(mgr.can_transition_to(&AutonomyLevel::Manual, &AutonomyLevel::CoPilot, false));
        assert!(mgr.can_transition_to(&AutonomyLevel::Manual, &AutonomyLevel::Autonomous, false));

        // CoPilot -> Autonomous requires allowlist
        assert!(mgr.can_transition_to(&AutonomyLevel::CoPilot, &AutonomyLevel::Autonomous, true));
        assert!(!mgr.can_transition_to(&AutonomyLevel::CoPilot, &AutonomyLevel::Autonomous, false));

        // Can always downgrade
        assert!(mgr.can_transition_to(&AutonomyLevel::Autonomous, &AutonomyLevel::CoPilot, false));
        assert!(mgr.can_transition_to(&AutonomyLevel::Autonomous, &AutonomyLevel::Manual, false));
    }

    #[test]
    fn test_default_allowlist_has_safe_commands() {
        let mgr = AutonomyManager::new();
        let _al = mgr.create_default_allowlist("repo-1");

        let allowed_build = mgr.check_command("repo-1", "cargo build", None);
        assert!(allowed_build.allowed);

        let allowed_test = mgr.check_command("repo-1", "cargo test", None);
        assert!(allowed_test.allowed);

        let denied_dangerous = mgr.check_command("repo-1", "rm -rf /", None);
        assert!(!denied_dangerous.allowed);
    }
}
