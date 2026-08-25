/*!
 * GitLab and Bitbucket bridges (Phase 3, Part 16).
 *
 * Parity with the existing GitHub bridge: connect a Repo Channel to a remote,
 * turn an issue into a Session, open a merge/pull request, and read its status
 * back into the thread.
 *
 * GitLab and Bitbucket differ in URL shape, auth header, and field names but
 * not in workflow, so the workflow lives here once and each provider supplies
 * its own request construction and response mapping.
 */

use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::api::types::{IsolationMode, Session};
use crate::persistence::Persistence;

const USER_AGENT: &str = "cid-core";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForgeKind {
    GitLab,
    Bitbucket,
}

impl ForgeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ForgeKind::GitLab => "gitlab",
            ForgeKind::Bitbucket => "bitbucket",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "gitlab" => Some(ForgeKind::GitLab),
            "bitbucket" => Some(ForgeKind::Bitbucket),
            _ => None,
        }
    }

    fn default_base(&self) -> &'static str {
        match self {
            ForgeKind::GitLab => "https://gitlab.com/api/v4",
            ForgeKind::Bitbucket => "https://api.bitbucket.org/2.0",
        }
    }

    /// What a "pull request" is called on this forge, for user-facing text.
    pub fn change_request_noun(&self) -> &'static str {
        match self {
            ForgeKind::GitLab => "merge request",
            ForgeKind::Bitbucket => "pull request",
        }
    }
}

/// A connected forge remote for one Repo Channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeConfig {
    pub repo_path: String,
    pub kind: ForgeKind,
    /// `group/project` on GitLab, `workspace/repo_slug` on Bitbucket.
    pub project: String,
    /// Self-hosted instances override this; empty means the provider default.
    pub base_url: Option<String>,
    pub connected: bool,
    pub has_token: bool,
}

impl ForgeConfig {
    fn api_base(&self) -> String {
        self.base_url
            .as_deref()
            .map(|b| b.trim_end_matches('/').to_string())
            .unwrap_or_else(|| self.kind.default_base().to_string())
    }
}

/// An issue, normalized across forges.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeIssue {
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub author: String,
    pub labels: Vec<String>,
    pub url: String,
    pub updated_at: Option<DateTime<Utc>>,
}

/// A merge/pull request, normalized across forges.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeChangeRequest {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub source_branch: String,
    pub target_branch: String,
    pub url: String,
    pub draft: bool,
}

pub struct ForgeManager {
    persistence: Arc<Persistence>,
    client: reqwest::Client,
}

impl ForgeManager {
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

    // ---- Tokens ----

    fn token_key(kind: ForgeKind, project: &str) -> String {
        format!("{}_token_{}", kind.as_str(), project.replace('/', "_"))
    }

    /// Look for a token in OS credential storage first, then the environment.
    /// Never logs the value.
    fn get_token(&self, kind: ForgeKind, project: &str) -> Option<String> {
        let key = Self::token_key(kind, project);
        if let Ok(entry) = keyring::Entry::new("com.cid.dev", &key) {
            if let Ok(pw) = entry.get_password() {
                if !pw.trim().is_empty() {
                    return Some(pw);
                }
            }
        }
        let env_var = match kind {
            ForgeKind::GitLab => "GITLAB_TOKEN",
            ForgeKind::Bitbucket => "BITBUCKET_TOKEN",
        };
        if let Ok(v) = std::env::var(env_var) {
            if !v.trim().is_empty() {
                return Some(v);
            }
        }
        if let Ok(entry) = keyring::Entry::new("com.cid.dev", &format!("{}_token", kind.as_str())) {
            if let Ok(pw) = entry.get_password() {
                if !pw.trim().is_empty() {
                    return Some(pw);
                }
            }
        }
        None
    }

    fn store_token(&self, kind: ForgeKind, project: &str, token: &str) -> Result<()> {
        let key = Self::token_key(kind, project);
        keyring::Entry::new("com.cid.dev", &key)
            .and_then(|e| e.set_password(token))
            .map_err(|e| anyhow::anyhow!("Failed to store {} token: {e}", kind.as_str()))
    }

    pub fn has_token(&self, kind: ForgeKind, project: &str) -> bool {
        self.get_token(kind, project).is_some()
    }

    /// Apply the provider's auth scheme. GitLab uses a bearer token;
    /// Bitbucket app passwords use HTTP basic as `user:app_password`.
    fn authorize(
        &self,
        req: reqwest::RequestBuilder,
        kind: ForgeKind,
        token: Option<&str>,
    ) -> reqwest::RequestBuilder {
        match (kind, token) {
            (_, None) => req,
            (ForgeKind::GitLab, Some(t)) => req.header("PRIVATE-TOKEN", t),
            (ForgeKind::Bitbucket, Some(t)) => {
                if let Some((user, pass)) = t.split_once(':') {
                    req.basic_auth(user, Some(pass))
                } else {
                    req.bearer_auth(t)
                }
            }
        }
    }

    // ---- Connection ----

    pub async fn connect(
        &self,
        repo_path: &str,
        kind: ForgeKind,
        project: &str,
        base_url: Option<String>,
        token: Option<String>,
    ) -> Result<ForgeConfig> {
        if repo_path.trim().is_empty() {
            bail!("repo_path must not be empty");
        }
        if !project.contains('/') {
            bail!(
                "project must be '{}' for {}",
                match kind {
                    ForgeKind::GitLab => "group/project",
                    ForgeKind::Bitbucket => "workspace/repo_slug",
                },
                kind.as_str()
            );
        }

        if let Some(t) = token.as_ref().filter(|t| !t.trim().is_empty()) {
            self.store_token(kind, project, t.trim())?;
        }

        let config = ForgeConfig {
            repo_path: repo_path.to_string(),
            kind,
            project: project.to_string(),
            base_url: base_url.filter(|b| !b.trim().is_empty()),
            connected: true,
            has_token: self.has_token(kind, project),
        };

        // Verify the project is actually reachable before recording it as
        // connected, so a typo surfaces now rather than at first use.
        match self.fetch_project(&config).await {
            Ok(()) => info!("{} connected: {} -> {}", kind.as_str(), repo_path, project),
            Err(e) => bail!(
                "Could not reach {} project '{}': {e}",
                kind.as_str(),
                project
            ),
        }

        self.persistence.save_forge_config(&config)?;
        Ok(config)
    }

    async fn fetch_project(&self, config: &ForgeConfig) -> Result<()> {
        let url = match config.kind {
            ForgeKind::GitLab => format!(
                "{}/projects/{}",
                config.api_base(),
                urlencoding_encode(&config.project)
            ),
            ForgeKind::Bitbucket => {
                format!("{}/repositories/{}", config.api_base(), config.project)
            }
        };
        let token = self.get_token(config.kind, &config.project);
        let resp = self
            .authorize(self.client.get(&url), config.kind, token.as_deref())
            .send()
            .await
            .context("request failed")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("{} — {}", status, truncate(&body, 200));
        }
        Ok(())
    }

    pub fn get_config(&self, repo_path: &str) -> Result<Option<ForgeConfig>> {
        let cfg = self.persistence.get_forge_config(repo_path)?;
        Ok(cfg.map(|mut c| {
            c.has_token = self.has_token(c.kind, &c.project);
            c
        }))
    }

    pub fn disconnect(&self, repo_path: &str) -> Result<()> {
        self.persistence.delete_forge_config(repo_path)
    }

    fn config_for(&self, repo_path: &str) -> Result<ForgeConfig> {
        self.get_config(repo_path)?.ok_or_else(|| {
            anyhow::anyhow!("No GitLab or Bitbucket remote is connected for {repo_path}")
        })
    }

    // ---- Issues ----

    pub async fn list_issues(&self, repo_path: &str, state: &str) -> Result<Vec<ForgeIssue>> {
        let config = self.config_for(repo_path)?;
        let url = match config.kind {
            ForgeKind::GitLab => format!(
                "{}/projects/{}/issues?state={}&per_page=50",
                config.api_base(),
                urlencoding_encode(&config.project),
                if state == "all" { "all" } else { state }
            ),
            ForgeKind::Bitbucket => format!(
                "{}/repositories/{}/issues?pagelen=50",
                config.api_base(),
                config.project
            ),
        };

        let json = self.get_json(&config, &url).await?;
        let items = match config.kind {
            ForgeKind::GitLab => json.as_array().cloned().unwrap_or_default(),
            // Bitbucket paginates under `values`.
            ForgeKind::Bitbucket => json["values"].as_array().cloned().unwrap_or_default(),
        };

        Ok(items
            .iter()
            .filter_map(|v| parse_issue(config.kind, v))
            .collect())
    }

    pub async fn get_issue(&self, repo_path: &str, number: u64) -> Result<ForgeIssue> {
        let config = self.config_for(repo_path)?;
        let url = match config.kind {
            ForgeKind::GitLab => format!(
                "{}/projects/{}/issues/{}",
                config.api_base(),
                urlencoding_encode(&config.project),
                number
            ),
            ForgeKind::Bitbucket => format!(
                "{}/repositories/{}/issues/{}",
                config.api_base(),
                config.project,
                number
            ),
        };
        let json = self.get_json(&config, &url).await?;
        parse_issue(config.kind, &json).ok_or_else(|| {
            anyhow::anyhow!(
                "Could not read issue #{number} from {}",
                config.kind.as_str()
            )
        })
    }

    /// Turn an issue into a Session — the same issue→Session trigger the GitHub
    /// bridge provides, so all three forges behave identically in the UI.
    pub async fn issue_to_session(
        &self,
        repo_path: &str,
        issue_number: u64,
        isolation_mode: Option<IsolationMode>,
    ) -> Result<Session> {
        let config = self.config_for(repo_path)?;
        let issue = self.get_issue(repo_path, issue_number).await?;
        let channel = self.persistence.get_repo_channel_by_path(repo_path)?;

        let task = format!(
            "{}\n\nFrom {} issue #{} ({})\n\n{}",
            issue.title,
            config.kind.as_str(),
            issue.number,
            issue.url,
            issue.body.unwrap_or_default()
        );

        let session = self.persistence.create_session(
            &channel.id,
            &format!("#{} {}", issue.number, issue.title),
            &task,
            isolation_mode.unwrap_or(IsolationMode::Worktree),
            crate::api::types::AutonomyLevel::CoPilot,
        )?;

        info!(
            "{} issue #{} became Session {}",
            config.kind.as_str(),
            issue.number,
            session.id
        );
        Ok(session)
    }

    // ---- Merge / pull requests ----

    pub async fn create_change_request(
        &self,
        repo_path: &str,
        title: &str,
        body: Option<&str>,
        source_branch: &str,
        target_branch: Option<&str>,
    ) -> Result<ForgeChangeRequest> {
        let config = self.config_for(repo_path)?;
        if !config.has_token {
            bail!(
                "Opening a {} needs a token; connect with one first",
                config.kind.change_request_noun()
            );
        }
        let target = target_branch.unwrap_or("main");

        let (url, payload) = match config.kind {
            ForgeKind::GitLab => (
                format!(
                    "{}/projects/{}/merge_requests",
                    config.api_base(),
                    urlencoding_encode(&config.project)
                ),
                serde_json::json!({
                    "source_branch": source_branch,
                    "target_branch": target,
                    "title": title,
                    "description": body.unwrap_or(""),
                }),
            ),
            ForgeKind::Bitbucket => (
                format!(
                    "{}/repositories/{}/pullrequests",
                    config.api_base(),
                    config.project
                ),
                serde_json::json!({
                    "title": title,
                    "description": body.unwrap_or(""),
                    "source": { "branch": { "name": source_branch } },
                    "destination": { "branch": { "name": target } },
                }),
            ),
        };

        let token = self.get_token(config.kind, &config.project);
        let resp = self
            .authorize(
                self.client.post(&url).json(&payload),
                config.kind,
                token.as_deref(),
            )
            .send()
            .await
            .context("request failed")?;

        let status = resp.status();
        let json: serde_json::Value = resp.json().await.unwrap_or_default();
        if !status.is_success() {
            bail!(
                "Failed to open {} ({}): {}",
                config.kind.change_request_noun(),
                status,
                truncate(&json.to_string(), 300)
            );
        }

        parse_change_request(config.kind, &json).ok_or_else(|| {
            anyhow::anyhow!("Unexpected response shape from {}", config.kind.as_str())
        })
    }

    pub async fn list_change_requests(&self, repo_path: &str) -> Result<Vec<ForgeChangeRequest>> {
        let config = self.config_for(repo_path)?;
        let url = match config.kind {
            ForgeKind::GitLab => format!(
                "{}/projects/{}/merge_requests?state=opened&per_page=50",
                config.api_base(),
                urlencoding_encode(&config.project)
            ),
            ForgeKind::Bitbucket => format!(
                "{}/repositories/{}/pullrequests?state=OPEN&pagelen=50",
                config.api_base(),
                config.project
            ),
        };
        let json = self.get_json(&config, &url).await?;
        let items = match config.kind {
            ForgeKind::GitLab => json.as_array().cloned().unwrap_or_default(),
            ForgeKind::Bitbucket => json["values"].as_array().cloned().unwrap_or_default(),
        };
        Ok(items
            .iter()
            .filter_map(|v| parse_change_request(config.kind, v))
            .collect())
    }

    pub async fn change_request_status(
        &self,
        repo_path: &str,
        number: u64,
    ) -> Result<ForgeChangeRequest> {
        let config = self.config_for(repo_path)?;
        let url = match config.kind {
            ForgeKind::GitLab => format!(
                "{}/projects/{}/merge_requests/{}",
                config.api_base(),
                urlencoding_encode(&config.project),
                number
            ),
            ForgeKind::Bitbucket => format!(
                "{}/repositories/{}/pullrequests/{}",
                config.api_base(),
                config.project,
                number
            ),
        };
        let json = self.get_json(&config, &url).await?;
        parse_change_request(config.kind, &json).ok_or_else(|| {
            anyhow::anyhow!(
                "Could not read {} #{number}",
                config.kind.change_request_noun()
            )
        })
    }

    async fn get_json(&self, config: &ForgeConfig, url: &str) -> Result<serde_json::Value> {
        debug!("{} GET {}", config.kind.as_str(), url);
        let token = self.get_token(config.kind, &config.project);
        let resp = self
            .authorize(self.client.get(url), config.kind, token.as_deref())
            .send()
            .await
            .context("request failed")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            warn!("{} returned {} for {}", config.kind.as_str(), status, url);
            bail!(
                "{} API returned {}: {}",
                config.kind.as_str(),
                status,
                truncate(&text, 300)
            );
        }
        Ok(serde_json::from_str(&text).unwrap_or(serde_json::Value::Null))
    }
}

// ---- Response mapping ----

fn parse_issue(kind: ForgeKind, v: &serde_json::Value) -> Option<ForgeIssue> {
    match kind {
        ForgeKind::GitLab => Some(ForgeIssue {
            number: v["iid"].as_u64()?,
            title: v["title"].as_str().unwrap_or_default().to_string(),
            body: v["description"].as_str().map(|s| s.to_string()),
            state: v["state"].as_str().unwrap_or("unknown").to_string(),
            author: v["author"]["username"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            labels: v["labels"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|l| l.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            url: v["web_url"].as_str().unwrap_or_default().to_string(),
            updated_at: parse_time(v["updated_at"].as_str()),
        }),
        ForgeKind::Bitbucket => Some(ForgeIssue {
            number: v["id"].as_u64()?,
            title: v["title"].as_str().unwrap_or_default().to_string(),
            body: v["content"]["raw"].as_str().map(|s| s.to_string()),
            state: v["state"].as_str().unwrap_or("unknown").to_string(),
            author: v["reporter"]["display_name"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            labels: v["kind"]
                .as_str()
                .map(|k| vec![k.to_string()])
                .unwrap_or_default(),
            url: v["links"]["html"]["href"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            updated_at: parse_time(v["updated_on"].as_str()),
        }),
    }
}

fn parse_change_request(kind: ForgeKind, v: &serde_json::Value) -> Option<ForgeChangeRequest> {
    match kind {
        ForgeKind::GitLab => Some(ForgeChangeRequest {
            number: v["iid"].as_u64()?,
            title: v["title"].as_str().unwrap_or_default().to_string(),
            state: v["state"].as_str().unwrap_or("unknown").to_string(),
            source_branch: v["source_branch"].as_str().unwrap_or_default().to_string(),
            target_branch: v["target_branch"].as_str().unwrap_or_default().to_string(),
            url: v["web_url"].as_str().unwrap_or_default().to_string(),
            draft: v["draft"]
                .as_bool()
                .or_else(|| v["work_in_progress"].as_bool())
                .unwrap_or(false),
        }),
        ForgeKind::Bitbucket => Some(ForgeChangeRequest {
            number: v["id"].as_u64()?,
            title: v["title"].as_str().unwrap_or_default().to_string(),
            state: v["state"].as_str().unwrap_or("unknown").to_string(),
            source_branch: v["source"]["branch"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            target_branch: v["destination"]["branch"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            url: v["links"]["html"]["href"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            draft: false,
        }),
    }
}

fn parse_time(s: Option<&str>) -> Option<DateTime<Utc>> {
    s.and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .map(|d| d.with_timezone(&Utc))
}

/// GitLab addresses projects as a URL-encoded `group/project` path.
fn urlencoding_encode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forge_kind_parses_and_round_trips() {
        assert_eq!(ForgeKind::parse("gitlab"), Some(ForgeKind::GitLab));
        assert_eq!(ForgeKind::parse("BitBucket"), Some(ForgeKind::Bitbucket));
        assert_eq!(
            ForgeKind::parse("github"),
            None,
            "GitHub has its own bridge"
        );
        assert_eq!(ForgeKind::GitLab.as_str(), "gitlab");
    }

    #[test]
    fn change_request_noun_matches_the_forge() {
        assert_eq!(ForgeKind::GitLab.change_request_noun(), "merge request");
        assert_eq!(ForgeKind::Bitbucket.change_request_noun(), "pull request");
    }

    #[test]
    fn project_paths_are_url_encoded_for_gitlab() {
        assert_eq!(urlencoding_encode("group/project"), "group%2Fproject");
        assert_eq!(urlencoding_encode("a-b_c.d"), "a-b_c.d");
        assert_eq!(urlencoding_encode("sub/group/proj"), "sub%2Fgroup%2Fproj");
    }

    #[test]
    fn api_base_falls_back_to_the_provider_default() {
        let cfg = ForgeConfig {
            repo_path: "/r".into(),
            kind: ForgeKind::GitLab,
            project: "g/p".into(),
            base_url: None,
            connected: true,
            has_token: false,
        };
        assert_eq!(cfg.api_base(), "https://gitlab.com/api/v4");

        let self_hosted = ForgeConfig {
            base_url: Some("https://gitlab.internal/api/v4/".into()),
            ..cfg
        };
        assert_eq!(
            self_hosted.api_base(),
            "https://gitlab.internal/api/v4",
            "a trailing slash must not produce a double slash in request URLs"
        );
    }

    #[test]
    fn gitlab_issues_map_to_the_normalized_shape() {
        let raw = serde_json::json!({
            "iid": 42,
            "title": "Fix login",
            "description": "It breaks",
            "state": "opened",
            "author": { "username": "alice" },
            "labels": ["bug", "auth"],
            "web_url": "https://gitlab.com/g/p/-/issues/42",
            "updated_at": "2026-07-01T10:00:00Z"
        });
        let issue = parse_issue(ForgeKind::GitLab, &raw).unwrap();
        assert_eq!(
            issue.number, 42,
            "GitLab uses iid, not id, for the visible number"
        );
        assert_eq!(issue.author, "alice");
        assert_eq!(issue.labels, vec!["bug", "auth"]);
        assert!(issue.updated_at.is_some());
    }

    #[test]
    fn bitbucket_issues_map_to_the_normalized_shape() {
        let raw = serde_json::json!({
            "id": 7,
            "title": "Add export",
            "content": { "raw": "please" },
            "state": "new",
            "reporter": { "display_name": "Bob" },
            "kind": "enhancement",
            "links": { "html": { "href": "https://bitbucket.org/w/r/issues/7" } },
            "updated_on": "2026-07-01T10:00:00+00:00"
        });
        let issue = parse_issue(ForgeKind::Bitbucket, &raw).unwrap();
        assert_eq!(issue.number, 7);
        assert_eq!(issue.body.as_deref(), Some("please"));
        assert_eq!(issue.author, "Bob");
        assert_eq!(issue.labels, vec!["enhancement"]);
    }

    #[test]
    fn an_issue_without_a_number_is_skipped_rather_than_guessed_at() {
        let raw = serde_json::json!({ "title": "no id" });
        assert!(parse_issue(ForgeKind::GitLab, &raw).is_none());
        assert!(parse_issue(ForgeKind::Bitbucket, &raw).is_none());
    }

    #[test]
    fn gitlab_merge_requests_map_including_draft_state() {
        let raw = serde_json::json!({
            "iid": 5, "title": "WIP work", "state": "opened",
            "source_branch": "cid/feature", "target_branch": "main",
            "web_url": "https://gitlab.com/g/p/-/merge_requests/5",
            "work_in_progress": true
        });
        let mr = parse_change_request(ForgeKind::GitLab, &raw).unwrap();
        assert_eq!(mr.number, 5);
        assert_eq!(mr.source_branch, "cid/feature");
        assert!(mr.draft, "work_in_progress should map to draft");
    }

    #[test]
    fn bitbucket_pull_requests_map_their_nested_branch_names() {
        let raw = serde_json::json!({
            "id": 11, "title": "Add export", "state": "OPEN",
            "source": { "branch": { "name": "cid/export" } },
            "destination": { "branch": { "name": "develop" } },
            "links": { "html": { "href": "https://bitbucket.org/w/r/pull-requests/11" } }
        });
        let pr = parse_change_request(ForgeKind::Bitbucket, &raw).unwrap();
        assert_eq!(pr.source_branch, "cid/export");
        assert_eq!(pr.target_branch, "develop");
        assert_eq!(pr.state, "OPEN");
    }

    #[test]
    fn missing_optional_fields_do_not_panic() {
        let raw = serde_json::json!({ "iid": 1 });
        let issue = parse_issue(ForgeKind::GitLab, &raw).unwrap();
        assert_eq!(issue.title, "");
        assert!(issue.body.is_none());
        assert!(issue.labels.is_empty());
        assert!(issue.updated_at.is_none());
    }

    #[test]
    fn connecting_rejects_a_project_without_a_namespace() {
        let p = Arc::new(crate::persistence::Persistence::new_in_memory().unwrap());
        let mgr = ForgeManager::new(p);
        let err = tokio_test::block_on(mgr.connect(
            "/repo",
            ForgeKind::GitLab,
            "just-a-name",
            None,
            None,
        ))
        .unwrap_err();
        assert!(err.to_string().contains("group/project"), "{err}");
    }

    #[test]
    fn operations_without_a_connected_remote_explain_themselves() {
        let p = Arc::new(crate::persistence::Persistence::new_in_memory().unwrap());
        let mgr = ForgeManager::new(p);
        let err = tokio_test::block_on(mgr.list_issues("/nope", "opened")).unwrap_err();
        assert!(
            err.to_string().contains("No GitLab or Bitbucket remote"),
            "{err}"
        );
    }

    #[test]
    fn token_keys_are_namespaced_per_forge_and_project() {
        let a = ForgeManager::token_key(ForgeKind::GitLab, "group/project");
        let b = ForgeManager::token_key(ForgeKind::Bitbucket, "group/project");
        assert_ne!(
            a, b,
            "two forges for the same path must not share a credential"
        );
        assert!(
            !a.contains('/'),
            "keyring keys should not carry path separators"
        );
    }
}
