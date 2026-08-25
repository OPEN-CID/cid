use std::path::Path;

/// Detects `AGENTS.md`/`SKILL.md` files on disk. The actual system-prompt
/// assembly (review_prompt.md §1.2: delimiting + sanitizing untrusted repo
/// content, layering Workspace/Repo/Session) lives in
/// `SkillsManager::build_system_context` (`skills/mod.rs`) — this struct
/// used to duplicate a second, weaker version of that same job that nothing
/// but its own test called; see git history if you need it, it added
/// nothing `SkillsManager` didn't already do better.
pub struct ContextManager;

impl Default for ContextManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextManager {
    pub fn new() -> Self {
        Self
    }

    /// Detect AGENTS.md in repo root or nested
    pub fn detect_agents_md(&self, repo_path: &str) -> Option<String> {
        let path = Path::new(repo_path);
        // Check root
        let candidates = [
            path.join("AGENTS.md"),
            path.join(".github").join("AGENTS.md"),
            path.join("docs").join("AGENTS.md"),
        ];

        for candidate in &candidates {
            if candidate.exists() {
                if let Ok(content) = std::fs::read_to_string(candidate) {
                    return Some(content);
                }
            }
        }

        // Check for nested AGENTS.md (monorepo) - just get root if any
        // For Phase 0 we only load root one
        None
    }

    pub fn list_skills_md(&self, repo_path: &str) -> Vec<(String, String)> {
        // Look for SKILL.md files
        let mut skills = Vec::new();
        let path = Path::new(repo_path);
        self.recursive_find_skill_md(path, &mut skills, 0);
        skills
    }

    fn recursive_find_skill_md(
        &self,
        dir: &Path,
        skills: &mut Vec<(String, String)>,
        depth: usize,
    ) {
        if depth > 3 {
            return;
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if name.starts_with('.') || name == "node_modules" || name == "target" {
                            continue;
                        }
                    }
                    self.recursive_find_skill_md(&path, skills, depth + 1);
                } else if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                    if file_name == "SKILL.md" || file_name == "skill.md" {
                        if let Ok(content) = std::fs::read_to_string(&path) {
                            skills.push((path.to_string_lossy().to_string(), content));
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_agents_md_finds_a_root_level_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("AGENTS.md"), "# Rules").unwrap();
        let mgr = ContextManager::new();
        assert_eq!(
            mgr.detect_agents_md(tmp.path().to_str().unwrap()),
            Some("# Rules".to_string())
        );
    }

    #[test]
    fn detect_agents_md_returns_none_when_absent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mgr = ContextManager::new();
        assert_eq!(mgr.detect_agents_md(tmp.path().to_str().unwrap()), None);
    }
}
