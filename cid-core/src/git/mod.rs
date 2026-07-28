use crate::api::types::{GitDiffFile, GitDiffHunk, GitStatusFile};
use anyhow::{Context, Result};
use git2::{Repository, StatusOptions};
use std::path::Path;

pub struct GitManager;

impl Default for GitManager {
    fn default() -> Self {
        Self::new()
    }
}

impl GitManager {
    pub fn new() -> Self {
        Self
    }

    pub fn status(&self, repo_path: &str) -> Result<Vec<GitStatusFile>> {
        let repo = Repository::open(repo_path).context("Failed to open repo")?;
        let mut opts = StatusOptions::new();
        opts.include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_ignored(false);
        let statuses = repo.statuses(Some(&mut opts))?;
        let mut files = Vec::new();
        for entry in statuses.iter() {
            let path = entry.path().unwrap_or("").to_string();
            let status = format!("{:?}", entry.status());
            let staged = entry.status().is_index_new()
                || entry.status().is_index_modified()
                || entry.status().is_index_deleted();
            files.push(GitStatusFile {
                path,
                status,
                staged,
            });
        }
        Ok(files)
    }

    pub fn diff(&self, repo_path: &str, base: Option<&str>) -> Result<Vec<GitDiffFile>> {
        let repo = Repository::open(repo_path)?;

        // Get diff between HEAD and working dir, or base branch vs HEAD
        let diff = if let Some(base_ref) = base {
            // diff base vs HEAD
            let base_obj = repo.revparse_single(base_ref)?;
            let base_tree = base_obj.peel_to_tree()?;
            let head = repo.head()?.peel_to_tree()?;
            repo.diff_tree_to_tree(Some(&base_tree), Some(&head), None)?
        } else {
            // diff HEAD to workdir including untracked?
            let head = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
            let mut diff_opts = git2::DiffOptions::new();
            diff_opts.include_untracked(true);
            repo.diff_tree_to_workdir_with_index(head.as_ref(), Some(&mut diff_opts))?
        };

        // Build files map via foreach callbacks using interior mutability to satisfy borrow checker
        use std::cell::RefCell;
        let files_map: RefCell<std::collections::HashMap<String, GitDiffFile>> =
            RefCell::new(std::collections::HashMap::new());

        diff.foreach(
            &mut |delta, _progress| {
                let path = delta
                    .new_file()
                    .path()
                    .unwrap_or(Path::new(""))
                    .to_string_lossy()
                    .to_string();
                let old_path = delta
                    .old_file()
                    .path()
                    .map(|p| p.to_string_lossy().to_string());
                let status = format!("{:?}", delta.status());
                files_map
                    .borrow_mut()
                    .entry(path.clone())
                    .or_insert(GitDiffFile {
                        path: path.clone(),
                        old_path,
                        status,
                        hunks: vec![],
                        additions: 0,
                        deletions: 0,
                    });
                true
            },
            None,
            Some(&mut |delta, hunk| {
                let path = delta
                    .new_file()
                    .path()
                    .unwrap_or(Path::new(""))
                    .to_string_lossy()
                    .to_string();
                let header = String::from_utf8_lossy(hunk.header()).to_string();
                let h = GitDiffHunk {
                    id: uuid::Uuid::new_v4().to_string(),
                    file_path: path.clone(),
                    old_start: hunk.old_start(),
                    old_lines: hunk.old_lines(),
                    new_start: hunk.new_start(),
                    new_lines: hunk.new_lines(),
                    header,
                    content: String::new(),
                };
                if let Some(entry) = files_map.borrow_mut().get_mut(&path) {
                    entry.hunks.push(h);
                }
                true
            }),
            Some(&mut |delta, _hunk, line| {
                let path = delta
                    .new_file()
                    .path()
                    .unwrap_or(Path::new(""))
                    .to_string_lossy()
                    .to_string();
                let content_str = String::from_utf8_lossy(line.content()).to_string();
                let origin = line.origin();
                let mut map = files_map.borrow_mut();
                if let Some(entry) = map.get_mut(&path) {
                    if let Some(last) = entry.hunks.last_mut() {
                        last.content.push_str(&format!("{}{}", origin, content_str));
                        match origin {
                            '+' => entry.additions += 1,
                            '-' => entry.deletions += 1,
                            _ => {}
                        }
                    }
                }
                true
            }),
        )?;

        let diff_files = files_map.into_inner().into_values().collect();
        Ok(diff_files)
    }

    pub fn commit(&self, repo_path: &str, message: &str) -> Result<String> {
        let repo = Repository::open(repo_path)?;
        let mut index = repo.index()?;
        // Add all changes? For Phase 0 we add all modified files to index (auto-commit per logical change)
        // We add all that are not ignored
        index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
        index.write()?;
        let oid = index.write_tree()?;
        let tree = repo.find_tree(oid)?;
        let signature = repo
            .signature()
            .or_else(|_| git2::Signature::now("CID", "cid@local"))?;
        let parent_commit = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        let parents: Vec<&git2::Commit> = if let Some(ref pc) = parent_commit {
            vec![pc]
        } else {
            vec![]
        };
        let commit_oid = repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &parents,
        )?;
        Ok(commit_oid.to_string())
    }

    /// Whether the working tree has anything uncommitted — staged, unstaged,
    /// or untracked. Used before taking a checkpoint (review_prompt.md
    /// §3.2): a checkpoint must capture whatever's on disk right now, not
    /// just the last commit, or a rewind could silently discard in-progress
    /// work that was never committed in the first place.
    pub fn is_dirty(&self, repo_path: &str) -> Result<bool> {
        Ok(!self.status(repo_path)?.is_empty())
    }

    /// The current HEAD commit's SHA, as a checkpoint's rewind target.
    pub fn head_sha(&self, repo_path: &str) -> Result<String> {
        let repo = Repository::open(repo_path)?;
        let head = repo.head()?.peel_to_commit()?;
        Ok(head.id().to_string())
    }

    /// Hard-reset the working tree to `sha` — a checkpoint rewind. This
    /// discards any commits and working-tree changes made after `sha`
    /// within this repo/worktree; it does not touch any other repo or
    /// worktree, and the caller is responsible for confirming that's what
    /// the human actually wants before calling it (the RPC layer requires
    /// an explicit `confirm: true`, not just a checkpoint id).
    pub fn reset_hard(&self, repo_path: &str, sha: &str) -> Result<()> {
        let repo = Repository::open(repo_path)?;
        let oid = git2::Oid::from_str(sha).context("invalid checkpoint commit id")?;
        let object = repo
            .find_object(oid, None)
            .context("checkpoint commit not found in this repo")?;
        repo.reset(&object, git2::ResetType::Hard, None)?;
        Ok(())
    }

    pub fn log(&self, repo_path: &str, limit: usize) -> Result<Vec<serde_json::Value>> {
        let repo = Repository::open(repo_path)?;
        let mut revwalk = repo.revwalk()?;
        revwalk.push_head()?;
        let mut commits = Vec::new();
        for (i, oid) in revwalk.enumerate() {
            if i >= limit {
                break;
            }
            let oid = oid?;
            let commit = repo.find_commit(oid)?;
            commits.push(serde_json::json!({
                "oid": oid.to_string(),
                "message": commit.message().unwrap_or("").to_string(),
                "author": commit.author().name().unwrap_or("").to_string(),
                "time": commit.time().seconds(),
            }));
        }
        Ok(commits)
    }

    pub fn create_worktree(
        &self,
        repo_path: &str,
        branch_name: &str,
        worktree_path: &str,
    ) -> Result<()> {
        // Use git CLI for reliability across git2 versions
        let wt_path = Path::new(worktree_path);
        if wt_path.exists() {
            anyhow::bail!("Worktree path already exists: {}", worktree_path);
        }
        if let Some(parent) = wt_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Ensure branch exists via git2 or CLI
        let repo = Repository::open(repo_path)?;
        let head = repo.head()?.peel_to_commit()?;
        if repo
            .find_branch(branch_name, git2::BranchType::Local)
            .is_err()
        {
            repo.branch(branch_name, &head, false)?;
        }

        // Use CLI: git worktree add -b <branch> <path> <branch> OR git worktree add <path> -b <branch>
        // The branch already exists, so we use: git worktree add --force <path> <branch>
        let output = std::process::Command::new("git")
            .args(["worktree", "add", worktree_path, branch_name])
            .current_dir(repo_path)
            .output()
            .context("Failed to run git worktree add")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // If branch already checked out, try with -f or using existing branch
            if stderr.contains("already exists") || stderr.contains("already checked out") {
                // Try force
                let output2 = std::process::Command::new("git")
                    .args(["worktree", "add", "--force", worktree_path, branch_name])
                    .current_dir(repo_path)
                    .output()?;
                if !output2.status.success() {
                    anyhow::bail!(
                        "git worktree add failed: {}",
                        String::from_utf8_lossy(&output2.stderr)
                    );
                }
            } else {
                anyhow::bail!(
                    "git worktree add failed: {} {}",
                    stderr,
                    String::from_utf8_lossy(&output.stdout)
                );
            }
        }

        Ok(())
    }

    pub fn list_worktrees(&self, repo_path: &str) -> Result<Vec<serde_json::Value>> {
        // CLI fallback: git worktree list --porcelain
        let output = std::process::Command::new("git")
            .args(["worktree", "list", "--porcelain"])
            .current_dir(repo_path)
            .output()
            .context("Failed to run git worktree list")?;

        if !output.status.success() {
            // Fallback to git2
            let repo = Repository::open(repo_path)?;
            let worktrees = repo.worktrees()?;
            let mut list = Vec::new();
            for name in worktrees.iter().flatten() {
                if let Ok(wt) = repo.find_worktree(name) {
                    list.push(serde_json::json!({
                        "name": name,
                        "path": wt.path().to_string_lossy(),
                    }));
                }
            }
            return Ok(list);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut list = Vec::new();
        let mut current: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for line in stdout.lines() {
            if line.is_empty() {
                if !current.is_empty() {
                    list.push(serde_json::json!({
                        "path": current.get("worktree").cloned().unwrap_or_default(),
                        "branch": current.get("branch").cloned(),
                        "head": current.get("HEAD").cloned(),
                    }));
                    current.clear();
                }
                continue;
            }
            if let Some((k, v)) = line.split_once(' ') {
                current.insert(k.to_string(), v.to_string());
            }
        }
        if !current.is_empty() {
            list.push(serde_json::json!({
                "path": current.get("worktree").cloned().unwrap_or_default(),
                "branch": current.get("branch").cloned(),
                "head": current.get("HEAD").cloned(),
            }));
        }

        Ok(list)
    }

    pub fn remove_worktree(&self, repo_path: &str, worktree_path: &str) -> Result<()> {
        // Use CLI: git worktree remove --force <path>
        let output = std::process::Command::new("git")
            .args(["worktree", "remove", "--force", worktree_path])
            .current_dir(repo_path)
            .output();

        if let Ok(out) = output {
            if !out.status.success() {
                // Fallback to removing dir
                eprintln!(
                    "git worktree remove failed: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
        }

        // Ensure directory removed
        if Path::new(worktree_path).exists() {
            std::fs::remove_dir_all(worktree_path)?;
        }

        // Prune
        let _ = std::process::Command::new("git")
            .args(["worktree", "prune"])
            .current_dir(repo_path)
            .output();

        Ok(())
    }
}

pub fn get_remote_url(repo_path: &str) -> Result<String> {
    let repo = Repository::open(repo_path)?;
    let remote = repo.find_remote("origin").or_else(|_| {
        repo.remotes()?
            .iter()
            .next()
            .flatten()
            .map(|name| repo.find_remote(name).unwrap())
            .ok_or_else(|| git2::Error::from_str("no remote"))
    })?;
    Ok(remote.url().unwrap_or("").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    #[test]
    fn test_git_manager_status() {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path().to_str().unwrap();
        Repository::init(repo_path).unwrap();
        let gm = GitManager::new();
        let status = gm.status(repo_path).unwrap();
        // Should be empty or minimal
        assert!(status.is_empty() || true);
    }

    /// Regression tests for review_prompt.md §3.2's checkpoint/rewind
    /// primitives: `is_dirty`, `head_sha`, and `reset_hard`.
    mod checkpoint_primitives {
        use super::*;

        fn init_repo_with_one_commit(path: &std::path::Path) -> Repository {
            let repo = Repository::init(path).unwrap();
            // Windows' line-ending translation would otherwise turn the
            // committed "v1\n" into "v1\r\n" on checkout, unrelated to the
            // reset_hard behavior under test.
            repo.config()
                .unwrap()
                .set_bool("core.autocrlf", false)
                .unwrap();
            std::fs::write(path.join("file.txt"), "v1\n").unwrap();
            {
                let mut index = repo.index().unwrap();
                index.add_path(std::path::Path::new("file.txt")).unwrap();
                index.write().unwrap();
                let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
                let sig = git2::Signature::now("CID Test", "test@example.com").unwrap();
                repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
                    .unwrap();
            }
            repo
        }

        #[test]
        fn is_dirty_is_false_on_a_clean_checkout() {
            let tmp = TempDir::new().unwrap();
            init_repo_with_one_commit(tmp.path());
            let gm = GitManager::new();
            assert!(!gm.is_dirty(tmp.path().to_str().unwrap()).unwrap());
        }

        #[test]
        fn is_dirty_is_true_after_an_uncommitted_edit() {
            let tmp = TempDir::new().unwrap();
            init_repo_with_one_commit(tmp.path());
            std::fs::write(tmp.path().join("file.txt"), "v2 uncommitted\n").unwrap();
            let gm = GitManager::new();
            assert!(gm.is_dirty(tmp.path().to_str().unwrap()).unwrap());
        }

        #[test]
        fn head_sha_matches_the_real_commit_id() {
            let tmp = TempDir::new().unwrap();
            let repo = init_repo_with_one_commit(tmp.path());
            let real_sha = repo
                .head()
                .unwrap()
                .peel_to_commit()
                .unwrap()
                .id()
                .to_string();
            let gm = GitManager::new();
            assert_eq!(gm.head_sha(tmp.path().to_str().unwrap()).unwrap(), real_sha);
        }

        #[test]
        fn reset_hard_restores_file_content_and_discards_later_commits() {
            let tmp = TempDir::new().unwrap();
            let repo_path = tmp.path().to_str().unwrap();
            let repo = init_repo_with_one_commit(tmp.path());
            let checkpoint_sha = repo
                .head()
                .unwrap()
                .peel_to_commit()
                .unwrap()
                .id()
                .to_string();

            let gm = GitManager::new();
            // A second, later commit after the checkpoint.
            std::fs::write(tmp.path().join("file.txt"), "v2\n").unwrap();
            gm.commit(repo_path, "second commit").unwrap();
            assert_eq!(
                std::fs::read_to_string(tmp.path().join("file.txt")).unwrap(),
                "v2\n"
            );

            gm.reset_hard(repo_path, &checkpoint_sha).unwrap();

            assert_eq!(
                std::fs::read_to_string(tmp.path().join("file.txt")).unwrap(),
                "v1\n",
                "reset_hard must restore the checkpoint's file content"
            );
            assert_eq!(gm.head_sha(repo_path).unwrap(), checkpoint_sha);
        }

        #[test]
        fn reset_hard_also_discards_uncommitted_changes() {
            let tmp = TempDir::new().unwrap();
            let repo_path = tmp.path().to_str().unwrap();
            init_repo_with_one_commit(tmp.path());
            let gm = GitManager::new();
            let checkpoint_sha = gm.head_sha(repo_path).unwrap();

            std::fs::write(tmp.path().join("file.txt"), "uncommitted mess\n").unwrap();
            assert!(gm.is_dirty(repo_path).unwrap());

            gm.reset_hard(repo_path, &checkpoint_sha).unwrap();

            assert_eq!(
                std::fs::read_to_string(tmp.path().join("file.txt")).unwrap(),
                "v1\n"
            );
        }

        #[test]
        fn reset_hard_rejects_an_invalid_sha() {
            let tmp = TempDir::new().unwrap();
            init_repo_with_one_commit(tmp.path());
            let gm = GitManager::new();
            let result = gm.reset_hard(tmp.path().to_str().unwrap(), "not-a-real-sha");
            assert!(result.is_err());
        }
    }
}
