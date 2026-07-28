//! Phase 1 GitHub bridge — Repo Channel ↔ GitHub remote
//! - Connect a Repo Channel to a GitHub remote (owner/repo)
//! - Issue list/get + issue → Mission (mirrors Copilot issue→PR flow)
//! - PR create/list/status with git push + API
//! - Off by default, user enables per Repo Channel via `github.connect`
//!
//! Security:
//! - Token stored in OS keyring (com.cid.dev / github_token_{owner}_{repo}), never plaintext in DB
//! - Env fallback GITHUB_TOKEN for CI
//! - Token never logged (redacted)
//! - Rate limit handling via headers

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tracing::{debug, info, warn};

use crate::api::types::{AutonomyLevel, GitHubConfig, GitHubIssue, GitHubPr, Mission, SessionMode};
use crate::persistence::Persistence;

const GITHUB_API_BASE: &str = "https://api.github.com";
const USER_AGENT: &str = "cid-core/0.1.0 (github-bridge)";

// ---------------------------------------------------------------------------
// GitHubManager
// ---------------------------------------------------------------------------

pub struct GitHubManager {
    persistence: Arc<Persistence>,
    client: reqwest::Client,
}

impl GitHubManager {
    pub fn new(persistence: Arc<Persistence>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(5))
            .user_agent(USER_AGENT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            persistence,
            client,
        }
    }

    // ========== Token helpers (keyring + env) ==========

    fn token_key(owner: &str, repo: &str) -> String {
        format!("github_token_{}_{}", owner, repo)
    }

    /// Retrieve token from keyring or env. Does NOT log token.
    fn get_token(&self, owner: &str, repo: &str) -> Option<String> {
        // 1. keyring per repo
        let key = Self::token_key(owner, repo);
        if let Ok(entry) = keyring::Entry::new("com.cid.dev", &key) {
            if let Ok(pw) = entry.get_password() {
                if !pw.trim().is_empty() {
                    debug!("GitHub token found in keyring for {}/{}", owner, repo);
                    return Some(pw);
                }
            }
        }
        // 2. env var (useful for CI)
        if let Ok(env_token) = std::env::var("GITHUB_TOKEN") {
            if !env_token.trim().is_empty() {
                debug!("GitHub token found in env GITHUB_TOKEN");
                return Some(env_token);
            }
        }
        // 3. global keyring entry (fallback for backwards compat, service-level)
        //    Attempt to read com.cid.dev / github_token (generic)
        if let Ok(entry) = keyring::Entry::new("com.cid.dev", "github_token") {
            if let Ok(pw) = entry.get_password() {
                if !pw.trim().is_empty() {
                    debug!("GitHub token found in global keyring entry");
                    return Some(pw);
                }
            }
        }
        // 4. settings github_token persisted as fallback (if present)
        if let Ok(settings) = self.persistence.get_settings() {
            if let Some(t) = settings.github_token {
                if !t.trim().is_empty() && !t.contains("...") {
                    debug!("GitHub token found in settings fallback");
                    return Some(t);
                }
            }
        }

        None
    }

    /// Does a token exist (keyring or env)?
    pub fn has_token(&self, owner: &str, repo: &str) -> bool {
        self.get_token(owner, repo).is_some()
    }

    fn store_token(&self, owner: &str, repo: &str, token: &str) -> Result<()> {
        let key = Self::token_key(owner, repo);
        let entry =
            keyring::Entry::new("com.cid.dev", &key).context("Failed to create keyring entry")?;
        entry
            .set_password(token)
            .context("Failed to store token in keyring")?;
        info!(
            "Stored GitHub token in keyring for {}/{} (key={})",
            owner, repo, key
        );
        Ok(())
    }

    // ========== Auth header builder, rate limit handling ==========

    fn build_request(
        &self,
        method: reqwest::Method,
        url: &str,
        token: Option<&str>,
    ) -> reqwest::RequestBuilder {
        let mut rb = self
            .client
            .request(method, url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", USER_AGENT);

        if let Some(t) = token {
            rb = rb.bearer_auth(t);
        }
        rb
    }

    async fn handle_response(&self, resp: reqwest::Response) -> Result<reqwest::Response> {
        let status = resp.status();
        let headers = resp.headers().clone();

        // Rate limit handling
        if let Some(remaining) = headers.get("x-ratelimit-remaining") {
            if let Ok(rem_str) = remaining.to_str() {
                if let Ok(rem) = rem_str.parse::<i32>() {
                    if rem == 0 {
                        if let Some(reset) = headers
                            .get("x-ratelimit-reset")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|s| s.parse::<i64>().ok())
                        {
                            let reset_dt =
                                DateTime::<Utc>::from_timestamp(reset, 0).unwrap_or_else(Utc::now);
                            warn!("GitHub rate limit exhausted, resets at {}", reset_dt);
                            bail!(
                                "GitHub rate limit exceeded, resets at {} (retry after)",
                                reset_dt
                            );
                        } else {
                            warn!("GitHub rate limit exhausted");
                            bail!("GitHub API rate limit exceeded");
                        }
                    }
                }
            }
        }

        if status == reqwest::StatusCode::FORBIDDEN {
            // Could be rate limit or permission
            if let Some(remaining) = headers.get("x-ratelimit-remaining") {
                if remaining.to_str().unwrap_or("1") == "0" {
                    bail!("GitHub API rate limit exceeded (403)");
                }
            }
            // Let caller handle 403 as permission error, but include body for context
            // We don't return error here yet, let caller inspect status
        }

        if status == reqwest::StatusCode::UNAUTHORIZED {
            bail!("GitHub unauthorized: invalid or expired token");
        }

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            bail!("GitHub API rate limit (429) — please retry after a minute");
        }

        if !status.is_success() {
            let status_code = status;
            let body = resp.text().await.unwrap_or_default();
            // Truncate body to avoid huge logs
            let body_trunc = if body.len() > 500 {
                &body[..500]
            } else {
                &body
            };
            bail!("GitHub API error {}: {}", status_code, body_trunc);
        }

        Ok(resp)
    }

    // ========== Core API: connect ==========

    /// Connect a repo path to GitHub remote, validate token, store config.
    /// token param is optional — if None, tries existing keyring/env token.
    /// Validates via GET /user if token present.
    pub async fn connect(
        &self,
        repo_path: &str,
        owner: &str,
        repo: &str,
        token: Option<String>,
    ) -> Result<GitHubConfig> {
        if owner.trim().is_empty() || repo.trim().is_empty() {
            bail!("owner and repo must not be empty");
        }
        if repo_path.trim().is_empty() {
            bail!("repo_path must not be empty");
        }

        // Validate repo_path exists (best-effort)
        if !Path::new(repo_path).exists() {
            warn!(
                "connect: repo_path does not exist on filesystem: {}",
                repo_path
            );
            // Not fatal — allow connecting for future cloning?
        }

        // Resolve token to use for validation
        let token_to_use: Option<String> = if let Some(t) = token.clone() {
            if t.trim().is_empty() {
                None
            } else {
                Some(t.trim().to_string())
            }
        } else {
            self.get_token(owner, repo)
        };

        // Validate token via GET /user if token present
        if let Some(ref t) = token_to_use {
            debug!(
                "Validating GitHub token via GET /user for {}/{}",
                owner, repo
            );
            let url = format!("{}/user", GITHUB_API_BASE);
            let req = self.build_request(reqwest::Method::GET, &url, Some(t));
            let resp = req
                .send()
                .await
                .context("Failed to call GitHub API /user")?;

            // Don't log token, just status
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                let snippet = if body.len() > 300 {
                    &body[..300]
                } else {
                    &body
                };
                bail!("GitHub token validation failed ({}): {}", status, snippet);
            }
            info!("GitHub token validated for {}/{}", owner, repo);
        } else {
            info!(
                "No GitHub token provided for {}/{}, connecting without auth (public-only)",
                owner, repo
            );
        }

        // Store token if provided explicitly
        if let Some(ref t) = token {
            if !t.trim().is_empty() {
                self.store_token(owner, repo, t.trim())?;
            }
        }

        // Persist config
        let has_token = self.has_token(owner, repo);
        let config = GitHubConfig {
            repo_path: repo_path.to_string(),
            owner: owner.to_string(),
            repo: repo.to_string(),
            connected: true,
            has_token,
        };

        let saved = self
            .persistence
            .save_github_config(&config)
            .context("Failed to save GitHub config")?;
        let mut saved_with_token = saved;
        saved_with_token.has_token = self.has_token(owner, repo);

        info!(
            "GitHub connected: {} -> {}/{} (has_token={})",
            repo_path, owner, repo, saved_with_token.has_token
        );
        Ok(saved_with_token)
    }

    /// Get config for a repo_path, with has_token evaluated.
    pub fn get_config(&self, repo_path: &str) -> Result<Option<GitHubConfig>> {
        let maybe = self.persistence.get_github_config(repo_path)?;
        if let Some(mut cfg) = maybe {
            cfg.has_token = self.has_token(&cfg.owner, &cfg.repo);
            Ok(Some(cfg))
        } else {
            Ok(None)
        }
    }

    /// Internal helper to get config or bail if not connected.
    fn require_config(&self, repo_path: &str) -> Result<GitHubConfig> {
        let cfg = self
            .persistence
            .get_github_config(repo_path)
            .context("Failed to query github config")?
            .ok_or_else(|| anyhow::anyhow!("GitHub not connected for repo_path: {}", repo_path))?;
        Ok(cfg)
    }

    // ========== Issues ==========

    pub async fn list_issues(&self, repo_path: &str, state: &str) -> Result<Vec<GitHubIssue>> {
        let cfg = self.require_config(repo_path)?;
        let token = self.get_token(&cfg.owner, &cfg.repo);

        let state = if state.trim().is_empty() {
            "open"
        } else {
            state
        };
        let url = format!(
            "{}/repos/{}/{}/issues?state={}&per_page=100",
            GITHUB_API_BASE, cfg.owner, cfg.repo, state
        );

        debug!("Listing GitHub issues: {}", url);
        let req = self.build_request(reqwest::Method::GET, &url, token.as_deref());
        let resp = req.send().await.context("Failed to list issues")?;
        let resp = self.handle_response(resp).await?;

        let api_issues: Vec<ApiIssueRaw> =
            resp.json().await.context("Failed to parse issues JSON")?;

        let mut issues = Vec::new();
        for raw in api_issues {
            // Filter out PRs (GitHub returns PRs in issues endpoint with pull_request field)
            if raw.pull_request.is_some() {
                continue;
            }
            if let Some(issue) = ApiIssueRaw::into_github_issue(raw) {
                issues.push(issue);
            }
        }

        info!(
            "Listed {} issues for {}/{} (state={})",
            issues.len(),
            cfg.owner,
            cfg.repo,
            state
        );
        Ok(issues)
    }

    pub async fn get_issue(&self, repo_path: &str, number: u64) -> Result<GitHubIssue> {
        let cfg = self.require_config(repo_path)?;
        let token = self.get_token(&cfg.owner, &cfg.repo);

        let url = format!(
            "{}/repos/{}/{}/issues/{}",
            GITHUB_API_BASE, cfg.owner, cfg.repo, number
        );
        debug!("Getting GitHub issue: {}", url);

        let req = self.build_request(reqwest::Method::GET, &url, token.as_deref());
        let resp = req.send().await.context("Failed to get issue")?;
        let resp = self.handle_response(resp).await?;

        let raw: ApiIssueRaw = resp.json().await.context("Failed to parse issue JSON")?;
        let issue = ApiIssueRaw::into_github_issue(raw)
            .ok_or_else(|| anyhow::anyhow!("Failed to convert issue"))?;

        Ok(issue)
    }

    // ========== Issue → Mission ==========

    pub async fn issue_to_mission(
        &self,
        repo_path: &str,
        issue_number: u64,
        session_mode: Option<SessionMode>,
    ) -> Result<Mission> {
        // Fetch issue
        let issue = self.get_issue(repo_path, issue_number).await?;

        // Find repo channel by path
        let repo_channel = match self.persistence.get_repo_channel_by_path(repo_path) {
            Ok(rc) => rc,
            Err(_) => {
                // Fallback: try list and find by path substring
                let repos = self.persistence.list_repo_channels()?;
                repos.into_iter().find(|r| r.path == repo_path)
                    .ok_or_else(|| anyhow::anyhow!("RepoChannel not found for path: {}. Please connect repo first via repo.connect", repo_path))?
            }
        };

        let title = issue.title.clone();
        let body = issue.body.clone().unwrap_or_default();

        // Build task per spec: "Fix GitHub issue #{number}: {title}\n\n{body}\n\nURL: {url}"
        let task_description = format!(
            "Fix GitHub issue #{}: {}\n\n{}\n\nURL: {}",
            issue.number, issue.title, body, issue.url
        );

        let mode = session_mode.unwrap_or(SessionMode::Worktree);

        // Create mission via persistence
        let mission = self
            .persistence
            .create_mission(
                &repo_channel.id,
                &title,
                &task_description,
                mode,
                AutonomyLevel::CoPilot,
            )
            .context("Failed to create mission from issue")?;

        // Also create a system message linking the issue (for traceability)
        let link_msg = format!(
            "🔗 Linked to GitHub issue #{}: {}\n\nURL: {}\n\nLabels: {}\nState: {}",
            issue.number,
            issue.title,
            issue.url,
            issue.labels.join(", "),
            issue.state
        );
        // Best-effort, ignore errors
        let _ = self.persistence.create_message(
            &mission.id,
            crate::api::types::MessageRole::System,
            &link_msg,
            vec![],
        );

        info!(
            "Created mission {} from GitHub issue #{} for repo {}",
            mission.id, issue.number, repo_path
        );
        Ok(mission)
    }

    // ========== PRs ==========

    /// Ensure head branch exists locally and push to remote.
    async fn ensure_branch_pushed(&self, repo_path: &str, head_branch: &str) -> Result<()> {
        let repo_path_owned = repo_path.to_string();
        let head_branch_owned = head_branch.to_string();

        // Use spawn_blocking for git CLI operations (sync)
        tokio::task::spawn_blocking(move || -> Result<()> {
            // Check if branch exists locally: git show-ref --verify refs/heads/<branch>
            let verify = std::process::Command::new("git")
                .args([
                    "show-ref",
                    "--verify",
                    &format!("refs/heads/{}", head_branch_owned),
                ])
                .current_dir(&repo_path_owned)
                .output();

            match verify {
                Ok(out) if out.status.success() => { /* exists */ }
                _ => {
                    // Try rev-parse --verify <branch>
                    let out2 = std::process::Command::new("git")
                        .args(["rev-parse", "--verify", &head_branch_owned])
                        .current_dir(&repo_path_owned)
                        .output()
                        .context("Failed to verify branch existence")?;
                    if !out2.status.success() {
                        bail!(
                            "Local branch '{}' does not exist in repo {}",
                            head_branch_owned,
                            repo_path_owned
                        );
                    }
                }
            }

            // Push
            debug!(
                "Pushing branch {} to origin in {}",
                head_branch_owned, repo_path_owned
            );
            let push = std::process::Command::new("git")
                .args(["push", "origin", &head_branch_owned])
                .current_dir(&repo_path_owned)
                .output()
                .context("Failed to run git push")?;

            if !push.status.success() {
                let stderr = String::from_utf8_lossy(&push.stderr);
                let stdout = String::from_utf8_lossy(&push.stdout);
                // If already up-to-date or no upstream needed, still consider success? Check stderr
                // If push fails, return error with output but redacted
                bail!(
                    "git push origin {} failed: {}\n{}",
                    head_branch_owned,
                    stderr,
                    stdout
                );
            }

            Ok(())
        })
        .await
        .context("Join error in git push task")??;

        Ok(())
    }

    pub async fn create_pr(
        &self,
        repo_path: &str,
        title: &str,
        body: Option<&str>,
        head_branch: &str,
        base_branch: Option<&str>,
    ) -> Result<GitHubPr> {
        if title.trim().is_empty() {
            bail!("PR title must not be empty");
        }
        if head_branch.trim().is_empty() {
            bail!("head_branch must not be empty");
        }

        let cfg = self.require_config(repo_path)?;
        let token = self
            .get_token(&cfg.owner, &cfg.repo)
            .ok_or_else(|| anyhow::anyhow!("GitHub token required to create PR"))?;

        // Ensure remote branch exists
        self.ensure_branch_pushed(repo_path, head_branch)
            .await
            .context("Failed to push head branch before creating PR")?;

        let base = base_branch.unwrap_or("main").to_string();
        let url = format!("{}/repos/{}/{}/pulls", GITHUB_API_BASE, cfg.owner, cfg.repo);

        let payload = serde_json::json!({
            "title": title,
            "body": body.unwrap_or(""),
            "head": head_branch,
            "base": base,
        });

        debug!(
            "Creating PR for {}/{}: head={} base={}",
            cfg.owner, cfg.repo, head_branch, base
        );

        let req = self
            .build_request(reqwest::Method::POST, &url, Some(&token))
            .json(&payload);
        let resp = req
            .send()
            .await
            .context("Failed to create PR via GitHub API")?;
        let resp = self.handle_response(resp).await?;

        let raw: ApiPrRaw = resp
            .json()
            .await
            .context("Failed to parse PR creation response")?;
        let pr = ApiPrRaw::into_github_pr(raw)
            .ok_or_else(|| anyhow::anyhow!("Failed to convert PR response"))?;

        info!(
            "Created PR #{} for {}/{}: {}",
            pr.number, cfg.owner, cfg.repo, pr.url
        );
        Ok(pr)
    }

    pub async fn list_prs(&self, repo_path: &str) -> Result<Vec<GitHubPr>> {
        let cfg = self.require_config(repo_path)?;
        let token = self.get_token(&cfg.owner, &cfg.repo);

        // Default to open PRs, per spec
        let url = format!(
            "{}/repos/{}/{}/pulls?state=open&per_page=100",
            GITHUB_API_BASE, cfg.owner, cfg.repo
        );
        debug!("Listing PRs: {}", url);

        let req = self.build_request(reqwest::Method::GET, &url, token.as_deref());
        let resp = req.send().await.context("Failed to list PRs")?;
        let resp = self.handle_response(resp).await?;

        let raw_prs: Vec<ApiPrRaw> = resp.json().await.context("Failed to parse PR list")?;
        let prs = raw_prs
            .into_iter()
            .filter_map(ApiPrRaw::into_github_pr)
            .collect::<Vec<_>>();

        info!("Listed {} PRs for {}/{}", prs.len(), cfg.owner, cfg.repo);
        Ok(prs)
    }

    pub async fn get_pr_status(&self, repo_path: &str, pr_number: u64) -> Result<GitHubPr> {
        let cfg = self.require_config(repo_path)?;
        let token = self.get_token(&cfg.owner, &cfg.repo);

        let url = format!(
            "{}/repos/{}/{}/pulls/{}",
            GITHUB_API_BASE, cfg.owner, cfg.repo, pr_number
        );
        debug!("Getting PR status: {}", url);

        let req = self.build_request(reqwest::Method::GET, &url, token.as_deref());
        let resp = req.send().await.context("Failed to get PR")?;
        let resp = self.handle_response(resp).await?;

        let raw: ApiPrRaw = resp.json().await.context("Failed to parse PR status")?;
        let pr =
            ApiPrRaw::into_github_pr(raw).ok_or_else(|| anyhow::anyhow!("Failed to convert PR"))?;
        Ok(pr)
    }
}

// ---------------------------------------------------------------------------
// Raw GitHub API structs → our types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ApiUser {
    login: String,
}

#[derive(Debug, Deserialize)]
struct ApiIssueRaw {
    id: u64,
    number: u64,
    title: String,
    body: Option<String>,
    state: String,
    user: Option<ApiUser>,
    #[serde(default)]
    labels: Vec<serde_json::Value>,
    created_at: String,
    updated_at: String,
    html_url: String,
    pull_request: Option<serde_json::Value>,
}

impl ApiIssueRaw {
    fn into_github_issue(raw: Self) -> Option<GitHubIssue> {
        let author = raw
            .user
            .map(|u| u.login)
            .unwrap_or_else(|| "unknown".to_string());

        let labels: Vec<String> = raw
            .labels
            .iter()
            .filter_map(|v| {
                if let Some(s) = v.as_str() {
                    Some(s.to_string())
                } else if let Some(obj) = v.as_object() {
                    obj.get("name")
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect();

        let created_at = raw
            .created_at
            .parse::<DateTime<Utc>>()
            .unwrap_or_else(|_| Utc::now());
        let updated_at = raw
            .updated_at
            .parse::<DateTime<Utc>>()
            .unwrap_or_else(|_| Utc::now());

        Some(GitHubIssue {
            id: raw.id,
            number: raw.number,
            title: raw.title,
            body: raw.body,
            state: raw.state,
            author,
            labels,
            created_at,
            updated_at,
            url: raw.html_url,
        })
    }
}

#[derive(Debug, Deserialize)]
struct ApiPrBranch {
    #[serde(rename = "ref")]
    ref_field: String,
}

#[derive(Debug, Deserialize)]
struct ApiPrRaw {
    id: u64,
    number: u64,
    title: String,
    body: Option<String>,
    state: String,
    head: ApiPrBranch,
    base: ApiPrBranch,
    user: Option<ApiUser>,
    html_url: String,
    created_at: String,
    // Deserialized for shape-completeness against GitHub's real API response;
    // not currently surfaced to callers.
    #[serde(default)]
    #[allow(dead_code)]
    updated_at: Option<String>,
}

impl ApiPrRaw {
    fn into_github_pr(raw: Self) -> Option<GitHubPr> {
        let author = raw
            .user
            .map(|u| u.login)
            .unwrap_or_else(|| "unknown".to_string());
        let created_at = raw
            .created_at
            .parse::<DateTime<Utc>>()
            .unwrap_or_else(|_| Utc::now());

        Some(GitHubPr {
            id: raw.id,
            number: raw.number,
            title: raw.title,
            body: raw.body,
            state: raw.state,
            head_branch: raw.head.ref_field,
            base_branch: raw.base.ref_field,
            author,
            url: raw.html_url,
            created_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::Persistence;

    #[test]
    fn test_token_key_format() {
        let key = GitHubManager::token_key("octocat", "hello-world");
        assert_eq!(key, "github_token_octocat_hello-world");
    }

    #[tokio::test]
    async fn test_github_manager_new() {
        let persistence = Arc::new(Persistence::new_in_memory().unwrap());
        let gm = GitHubManager::new(persistence);
        // No token stored for owner/repo in fresh keyring
        // This might fail if GITHUB_TOKEN env var is set in CI
        let has_token = gm.has_token("owner", "repo");
        if std::env::var("GITHUB_TOKEN").is_ok() {
            // In CI with GITHUB_TOKEN set, has_token may return true
            println!("Skipping strict token check because GITHUB_TOKEN is set");
        } else {
            assert!(
                !has_token,
                "Expected no token for 'owner/repo' without keyring entry"
            );
        }
    }

    #[test]
    fn test_issue_raw_into() {
        let json = r#"{
            "id": 1,
            "number": 2,
            "title": "Bug",
            "body": "body text",
            "state": "open",
            "user": {"login": "alice"},
            "labels": [{"name": "bug"}, "enhancement"],
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-02T00:00:00Z",
            "html_url": "https://github.com/o/r/issues/2"
        }"#;
        let raw: ApiIssueRaw = serde_json::from_str(json).unwrap();
        let issue = ApiIssueRaw::into_github_issue(raw).unwrap();
        assert_eq!(issue.number, 2);
        assert_eq!(issue.labels.len(), 2);
        assert!(issue.labels.contains(&"bug".to_string()));
    }

    #[test]
    fn test_pr_raw_into() {
        let json = r#"{
            "id": 10,
            "number": 5,
            "title": "Fix",
            "body": "fix body",
            "state": "open",
            "head": {"ref": "feature"},
            "base": {"ref": "main"},
            "user": {"login": "bob"},
            "html_url": "https://github.com/o/r/pull/5",
            "created_at": "2024-01-03T00:00:00Z"
        }"#;
        let raw: ApiPrRaw = serde_json::from_str(json).unwrap();
        let pr = ApiPrRaw::into_github_pr(raw).unwrap();
        assert_eq!(pr.number, 5);
        assert_eq!(pr.head_branch, "feature");
    }
}
