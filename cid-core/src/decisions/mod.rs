/*!
 * Decisions view and deployment record (Phase 4, Part A).
 *
 * Two small, distinct pieces the Phase 4 brief scopes together because both
 * close real gaps in what a Session surfaces about itself:
 *
 *   - **Decisions**: the ADR log (already required since Part 0 rule 1)
 *     surfaced *per-Session* — which ADRs exist in the repo, and which ones
 *     a Session actually touched or is relevant to, linked inline rather than
 *     only discoverable by browsing `docs/adr/`.
 *   - **Deployment record**: a log of what was deployed, when, and where.
 *     Explicitly not an orchestrator — CID gains the ability to *display*
 *     that a deployment happened, never to perform one. Entries arrive by
 *     manual entry or a CI webhook.
 */

use std::sync::Arc;

use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::persistence::Persistence;

// ---------------------------------------------------------------------------
// Decisions (ADR) view
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdrSummary {
    pub number: String,
    pub title: String,
    pub path: String,
    pub status: Option<String>,
}

/// Read every `docs/adr/NNNN-*.md` file in a repo and pull out a summary.
/// Real ADR *content* stays in the files — this is a listing, not a second
/// copy of the document.
pub fn list_adrs(repo_path: &str) -> Vec<AdrSummary> {
    let adr_dir = std::path::Path::new(repo_path).join("docs").join("adr");
    let Ok(entries) = std::fs::read_dir(&adr_dir) else {
        return vec![];
    };

    let mut summaries: Vec<AdrSummary> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("md"))
        .filter_map(|e| {
            let file_name = e.file_name().to_string_lossy().to_string();
            // Skip the template — it isn't a real decision.
            if file_name.eq_ignore_ascii_case("0000-template.md") {
                return None;
            }
            let number = file_name.split('-').next()?.to_string();
            if !number.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let content = std::fs::read_to_string(e.path()).ok()?;
            let title = parse_adr_title(&content).unwrap_or_else(|| file_name.clone());
            let status = parse_adr_status(&content);
            Some(AdrSummary {
                number,
                title,
                path: e.path().to_string_lossy().replace('\\', "/"),
                status,
            })
        })
        .collect();

    summaries.sort_by(|a, b| a.number.cmp(&b.number));
    summaries
}

/// Which ADRs a Session is relevant to: those explicitly referenced by number
/// or filename in the Session's task description, plan, or messages — the
/// same "nearest concrete reference wins" spirit as Part 12's context
/// resolution, not a fuzzy content match that would produce noisy results.
pub fn adrs_relevant_to_session(repo_path: &str, session_text: &[&str]) -> Vec<AdrSummary> {
    let all = list_adrs(repo_path);
    let combined = session_text.join("\n").to_lowercase();

    all.into_iter()
        .filter(|adr| {
            combined.contains(&format!("adr {}", adr.number))
                || combined.contains(&format!("adr-{}", adr.number))
                || combined.contains(&format!("adr{}", adr.number))
                || combined.contains(&adr.path.to_lowercase())
        })
        .collect()
}

fn parse_adr_title(content: &str) -> Option<String> {
    content
        .lines()
        .find(|l| l.trim_start().starts_with("# "))
        .map(|l| l.trim_start_matches('#').trim().to_string())
}

fn parse_adr_status(content: &str) -> Option<String> {
    for line in content.lines().take(20) {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("**Status:**") {
            return Some(rest.trim().to_string());
        }
        if let Some(rest) = trimmed.strip_prefix("Status:") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Deployment record
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentRecord {
    pub id: String,
    pub session_id: String,
    pub environment: String,
    pub commit_or_tag: String,
    pub ci_run_url: Option<String>,
    pub note: Option<String>,
    pub source: DeploymentSource,
    pub deployed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentSource {
    Manual,
    CiWebhook,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeploymentRecordInput {
    pub session_id: String,
    pub environment: String,
    pub commit_or_tag: String,
    pub ci_run_url: Option<String>,
    pub note: Option<String>,
}

pub struct DeploymentLog {
    persistence: Arc<Persistence>,
}

impl DeploymentLog {
    pub fn new(persistence: Arc<Persistence>) -> Self {
        Self { persistence }
    }

    /// Record a deployment. This is the entire surface — there is no "deploy"
    /// method anywhere in CID, deliberately. See the module doc.
    pub fn record(
        &self,
        input: DeploymentRecordInput,
        source: DeploymentSource,
    ) -> Result<DeploymentRecord> {
        if input.environment.trim().is_empty() {
            bail!("environment must not be empty");
        }
        if input.commit_or_tag.trim().is_empty() {
            bail!("commit_or_tag must not be empty");
        }
        self.persistence.get_session(&input.session_id)?;

        let record = DeploymentRecord {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: input.session_id,
            environment: input.environment,
            commit_or_tag: input.commit_or_tag,
            ci_run_url: input.ci_run_url,
            note: input.note,
            source,
            deployed_at: Utc::now(),
        };
        self.persistence.save_deployment_record(&record)?;
        Ok(record)
    }

    pub fn for_session(&self, session_id: &str) -> Result<Vec<DeploymentRecord>> {
        self.persistence.list_deployment_records(session_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_adr(dir: &std::path::Path, filename: &str, content: &str) {
        let adr_dir = dir.join("docs").join("adr");
        std::fs::create_dir_all(&adr_dir).unwrap();
        std::fs::write(adr_dir.join(filename), content).unwrap();
    }

    // ---- Decisions ----

    #[test]
    fn lists_adrs_with_title_and_status() {
        let dir = tempfile::tempdir().unwrap();
        write_adr(
            dir.path(),
            "0012-core-access-control.md",
            "# ADR 0012 — Core access control\n\n**Status:** Accepted\n\nBody text.\n",
        );

        let adrs = list_adrs(&dir.path().to_string_lossy());
        assert_eq!(adrs.len(), 1);
        assert_eq!(adrs[0].number, "0012");
        assert!(adrs[0].title.contains("Core access control"));
        assert_eq!(adrs[0].status.as_deref(), Some("Accepted"));
    }

    #[test]
    fn skips_the_template_file() {
        let dir = tempfile::tempdir().unwrap();
        write_adr(dir.path(), "0000-template.md", "# Template\n");
        write_adr(
            dir.path(),
            "0001-real-decision.md",
            "# ADR 0001 — Real decision\n",
        );

        let adrs = list_adrs(&dir.path().to_string_lossy());
        assert_eq!(adrs.len(), 1);
        assert_eq!(adrs[0].number, "0001");
    }

    #[test]
    fn a_repo_with_no_adr_directory_returns_an_empty_list_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(list_adrs(&dir.path().to_string_lossy()).is_empty());
    }

    #[test]
    fn adrs_relevant_to_a_session_are_found_by_explicit_reference() {
        let dir = tempfile::tempdir().unwrap();
        write_adr(
            dir.path(),
            "0011-windows-sandbox-boundary.md",
            "# ADR 0011 — Sandbox\n",
        );
        write_adr(
            dir.path(),
            "0012-core-access-control.md",
            "# ADR 0012 — Access control\n",
        );

        let relevant = adrs_relevant_to_session(
            &dir.path().to_string_lossy(),
            &["Fix the bug described in ADR 0011 about the sandbox boundary"],
        );
        assert_eq!(relevant.len(), 1);
        assert_eq!(relevant[0].number, "0011");
    }

    #[test]
    fn unrelated_sessions_surface_no_adrs() {
        let dir = tempfile::tempdir().unwrap();
        write_adr(
            dir.path(),
            "0011-windows-sandbox-boundary.md",
            "# ADR 0011 — Sandbox\n",
        );

        let relevant = adrs_relevant_to_session(
            &dir.path().to_string_lossy(),
            &["Add a new button to the UI"],
        );
        assert!(relevant.is_empty());
    }

    // ---- Deployment record ----

    fn deployment_log() -> (DeploymentLog, Arc<Persistence>, String) {
        let persistence = Arc::new(Persistence::new_in_memory().unwrap());
        let ws = persistence.list_workspaces().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let channel = persistence
            .connect_repo(&repo.path().to_string_lossy(), Some(&ws[0].id))
            .unwrap();
        let session = persistence
            .create_session(
                &channel.id,
                "t",
                "task",
                crate::api::types::IsolationMode::Shared,
                crate::api::types::AutonomyLevel::Manual,
            )
            .unwrap();
        (
            DeploymentLog::new(persistence.clone()),
            persistence,
            session.id,
        )
    }

    #[test]
    fn records_and_lists_a_manual_deployment() {
        let (log, _p, session_id) = deployment_log();
        let record = log
            .record(
                DeploymentRecordInput {
                    session_id: session_id.clone(),
                    environment: "production".into(),
                    commit_or_tag: "v1.2.3".into(),
                    ci_run_url: Some("https://ci.example.com/run/1".into()),
                    note: Some("hotfix".into()),
                },
                DeploymentSource::Manual,
            )
            .unwrap();
        assert_eq!(record.source, DeploymentSource::Manual);

        let all = log.for_session(&session_id).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].environment, "production");
    }

    #[test]
    fn a_ci_webhook_deployment_is_tagged_with_its_source() {
        let (log, _p, session_id) = deployment_log();
        let record = log
            .record(
                DeploymentRecordInput {
                    session_id,
                    environment: "staging".into(),
                    commit_or_tag: "abc123".into(),
                    ci_run_url: None,
                    note: None,
                },
                DeploymentSource::CiWebhook,
            )
            .unwrap();
        assert_eq!(record.source, DeploymentSource::CiWebhook);
    }

    #[test]
    fn rejects_a_record_with_no_environment_or_commit() {
        let (log, _p, session_id) = deployment_log();
        let base = DeploymentRecordInput {
            session_id: session_id.clone(),
            environment: "".into(),
            commit_or_tag: "v1".into(),
            ci_run_url: None,
            note: None,
        };
        assert!(log.record(base, DeploymentSource::Manual).is_err());

        let base2 = DeploymentRecordInput {
            session_id,
            environment: "prod".into(),
            commit_or_tag: "".into(),
            ci_run_url: None,
            note: None,
        };
        assert!(log.record(base2, DeploymentSource::Manual).is_err());
    }

    #[test]
    fn rejects_a_deployment_for_a_session_that_does_not_exist() {
        let (log, _p, _session_id) = deployment_log();
        let input = DeploymentRecordInput {
            session_id: "no-such-session".into(),
            environment: "prod".into(),
            commit_or_tag: "v1".into(),
            ci_run_url: None,
            note: None,
        };
        assert!(log.record(input, DeploymentSource::Manual).is_err());
    }

    #[test]
    fn deployments_are_scoped_per_session() {
        let (log, persistence, session_id) = deployment_log();
        let ws = persistence.list_workspaces().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let channel = persistence
            .connect_repo(&repo.path().to_string_lossy(), Some(&ws[0].id))
            .unwrap();
        let other_session = persistence
            .create_session(
                &channel.id,
                "other",
                "task",
                crate::api::types::IsolationMode::Shared,
                crate::api::types::AutonomyLevel::Manual,
            )
            .unwrap();

        log.record(
            DeploymentRecordInput {
                session_id: session_id.clone(),
                environment: "prod".into(),
                commit_or_tag: "v1".into(),
                ci_run_url: None,
                note: None,
            },
            DeploymentSource::Manual,
        )
        .unwrap();
        log.record(
            DeploymentRecordInput {
                session_id: other_session.id.clone(),
                environment: "prod".into(),
                commit_or_tag: "v2".into(),
                ci_run_url: None,
                note: None,
            },
            DeploymentSource::Manual,
        )
        .unwrap();

        assert_eq!(log.for_session(&session_id).unwrap().len(), 1);
        assert_eq!(log.for_session(&other_session.id).unwrap().len(), 1);
    }
}
