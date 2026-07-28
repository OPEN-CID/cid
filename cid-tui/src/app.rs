//! Application state for the CID TUI.

use serde::Deserialize;
use serde_json::Value;

use crate::api::CoreClient;

#[derive(Debug, Clone, Deserialize)]
pub struct RepoChannel {
    pub id: String,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Mission {
    pub id: String,
    pub title: String,
    pub status: String,
    pub autonomy_level: String,
    pub repo_channel_id: String,
    // Worktree-mode Missions diff against their own worktree; shared-clone
    // Missions (this is None) diff against the Repo Channel's own path.
    pub worktree_path: Option<String>,
}

/// Mirrors `GitDiffHunk` (`cid-core/src/api/types.rs`) — the same shape
/// `src/components/diff/DiffViewer.tsx` renders on the web/desktop side.
#[derive(Debug, Clone, Deserialize)]
pub struct DiffHunk {
    // Part of Core's real GitDiffHunk shape; this view is read-only (see
    // `ui::draw_diff`), so nothing here needs a hunk's id to act on it yet.
    #[allow(dead_code)]
    pub id: String,
    pub header: String,
    pub content: String,
}

/// Mirrors `GitDiffFile`.
#[derive(Debug, Clone, Deserialize)]
pub struct DiffFile {
    pub path: String,
    pub status: String,
    pub hunks: Vec<DiffHunk>,
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatMessage {
    // Part of Core's real message shape; not yet used as a React-style `key`
    // since messages are rendered as a plain Vec, not a keyed list.
    #[allow(dead_code)]
    pub id: String,
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PendingApproval {
    pub tool_call_id: String,
    pub tool_name: String,
    pub arguments: Value,
}

/// Which pane has keyboard focus. `Diff` replaces the whole body (mission
/// list + thread) with the diff view rather than sitting alongside it —
/// there isn't screen width in a terminal for both at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    MissionList,
    Thread,
    Composer,
    Diff,
}

pub struct App {
    pub client: CoreClient,
    pub connected: bool,
    pub status_line: String,

    pub repos: Vec<RepoChannel>,
    pub missions: Vec<Mission>,
    pub selected_mission_index: usize,

    pub messages: Vec<ChatMessage>,
    pub pending_approvals: Vec<PendingApproval>,
    pub selected_approval_index: usize,

    pub composer: String,
    pub focus: Focus,
    pub should_quit: bool,
    pub last_error: Option<String>,

    pub diff_files: Vec<DiffFile>,
    pub selected_diff_file_index: usize,
}

impl App {
    pub fn new(client: CoreClient) -> Self {
        Self {
            client,
            connected: false,
            status_line: "Connecting…".to_string(),
            repos: Vec::new(),
            missions: Vec::new(),
            selected_mission_index: 0,
            messages: Vec::new(),
            pending_approvals: Vec::new(),
            selected_approval_index: 0,
            composer: String::new(),
            focus: Focus::MissionList,
            should_quit: false,
            last_error: None,
            diff_files: Vec::new(),
            selected_diff_file_index: 0,
        }
    }

    pub fn selected_mission(&self) -> Option<&Mission> {
        self.missions.get(self.selected_mission_index)
    }

    /// A worktree-mode Mission diffs against its own worktree; a
    /// shared-clone Mission (`worktree_path: None`) diffs against the Repo
    /// Channel's own path directly — same fallback `DiffViewer.tsx` uses.
    fn repo_path_for_selected_mission(&self) -> Option<String> {
        let mission = self.selected_mission()?;
        if let Some(wt) = &mission.worktree_path {
            return Some(wt.clone());
        }
        self.repos
            .iter()
            .find(|r| r.id == mission.repo_channel_id)
            .map(|r| r.path.clone())
    }

    pub async fn refresh_diff(&mut self) {
        let Some(repo_path) = self.repo_path_for_selected_mission() else {
            self.diff_files.clear();
            return;
        };
        match self
            .client
            .call("git.diff", serde_json::json!({ "repo_path": repo_path }))
            .await
        {
            Ok(result) => {
                if let Ok(parsed) = serde_json::from_value::<Vec<DiffFile>>(result) {
                    self.diff_files = parsed;
                    if self.selected_diff_file_index >= self.diff_files.len() {
                        self.selected_diff_file_index = self.diff_files.len().saturating_sub(1);
                    }
                }
            }
            Err(e) => {
                self.last_error = Some(e.to_string());
                self.diff_files.clear();
            }
        }
    }

    /// Full state refresh: connection, repo channels, missions across all of
    /// them, and — if a Mission is selected — its thread and pending
    /// approvals. Called on a fixed interval by the event loop, and
    /// immediately after any action that changes state (send, approve, deny).
    pub async fn refresh(&mut self) {
        match self.client.health().await {
            Ok(_) => {
                self.connected = true;
                self.last_error = None;
            }
            Err(e) => {
                self.connected = false;
                self.status_line = format!("Core unreachable: {e}");
                return;
            }
        }

        if let Ok(repos) = self.client.call("repo.list", serde_json::json!({})).await {
            if let Ok(parsed) = serde_json::from_value::<Vec<RepoChannel>>(repos) {
                self.repos = parsed;
            }
        }

        let mut all_missions = Vec::new();
        for repo in &self.repos {
            if let Ok(result) = self
                .client
                .call(
                    "mission.list",
                    serde_json::json!({ "repo_channel_id": repo.id }),
                )
                .await
            {
                if let Ok(mut parsed) = serde_json::from_value::<Vec<Mission>>(result) {
                    all_missions.append(&mut parsed);
                }
            }
        }
        self.missions = all_missions;
        if self.selected_mission_index >= self.missions.len() && !self.missions.is_empty() {
            self.selected_mission_index = self.missions.len() - 1;
        }

        self.status_line = format!(
            "{} repo channel(s), {} mission(s)",
            self.repos.len(),
            self.missions.len()
        );

        if let Some(mission) = self.selected_mission().cloned() {
            self.refresh_mission_detail(&mission.id).await;
        } else {
            self.messages.clear();
            self.pending_approvals.clear();
        }

        // Kept live on the same cadence as the thread — a stale diff view
        // during an active Mission would defeat the point of watching it.
        if self.focus == Focus::Diff {
            self.refresh_diff().await;
        }
    }

    async fn refresh_mission_detail(&mut self, mission_id: &str) {
        if let Ok(result) = self
            .client
            .call(
                "message.list",
                serde_json::json!({ "mission_id": mission_id }),
            )
            .await
        {
            if let Ok(parsed) = serde_json::from_value::<Vec<ChatMessage>>(result) {
                self.messages = parsed;
            }
        }
    }

    /// Apply an event received from the background WebSocket listener
    /// (`events::listen`) — the push side of state that HTTP polling alone
    /// cannot see in time: a tool call waiting on approval right now.
    pub fn apply_event(&mut self, event: crate::events::CoreEvent) {
        use crate::events::CoreEvent;
        match event {
            CoreEvent::ToolCallRequest {
                mission_id,
                tool_call_id,
                tool_name,
                arguments,
            } => {
                if self.selected_mission().is_some_and(|m| m.id == mission_id) {
                    self.pending_approvals.push(PendingApproval {
                        tool_call_id,
                        tool_name,
                        arguments,
                    });
                }
            }
            CoreEvent::ToolCallComplete { tool_call_id, .. } => {
                self.pending_approvals
                    .retain(|a| a.tool_call_id != tool_call_id);
                if self.selected_approval_index >= self.pending_approvals.len()
                    && self.selected_approval_index > 0
                {
                    self.selected_approval_index -= 1;
                }
            }
            CoreEvent::MissionChanged => {}
        }
    }

    pub async fn send_message(&mut self) {
        let Some(mission) = self.selected_mission().cloned() else {
            return;
        };
        let content = self.composer.trim().to_string();
        if content.is_empty() {
            return;
        }
        self.composer.clear();

        if let Err(e) = self
            .client
            .call(
                "mission.send_message",
                serde_json::json!({ "mission_id": mission.id, "content": content }),
            )
            .await
        {
            self.last_error = Some(e.to_string());
        }
        self.refresh_mission_detail(&mission.id).await;
    }

    pub async fn approve_selected(&mut self, approved: bool) {
        let Some(mission) = self.selected_mission().cloned() else {
            return;
        };
        let Some(approval) = self
            .pending_approvals
            .get(self.selected_approval_index)
            .cloned()
        else {
            return;
        };

        if let Err(e) = self
            .client
            .call(
                "mission.approve_tool",
                serde_json::json!({
                    "mission_id": mission.id,
                    "tool_call_id": approval.tool_call_id,
                    "approved": approved,
                }),
            )
            .await
        {
            self.last_error = Some(e.to_string());
        }
        self.pending_approvals
            .retain(|a| a.tool_call_id != approval.tool_call_id);
        if self.selected_approval_index >= self.pending_approvals.len()
            && self.selected_approval_index > 0
        {
            self.selected_approval_index -= 1;
        }
    }

    pub fn select_next_mission(&mut self) {
        if !self.missions.is_empty() {
            self.selected_mission_index = (self.selected_mission_index + 1) % self.missions.len();
            self.on_mission_selection_changed();
        }
    }

    pub fn select_prev_mission(&mut self) {
        if !self.missions.is_empty() {
            self.selected_mission_index =
                (self.selected_mission_index + self.missions.len() - 1) % self.missions.len();
            self.on_mission_selection_changed();
        }
    }

    /// Pending approvals are tracked only for the currently selected Mission
    /// (see `apply_event`) — switching away must not leave a stale approval
    /// card showing for a Mission that isn't on screen anymore.
    fn on_mission_selection_changed(&mut self) {
        self.pending_approvals.clear();
        self.selected_approval_index = 0;
        self.messages.clear();
        self.diff_files.clear();
        self.selected_diff_file_index = 0;
    }

    pub fn select_next_diff_file(&mut self) {
        if !self.diff_files.is_empty() {
            self.selected_diff_file_index =
                (self.selected_diff_file_index + 1) % self.diff_files.len();
        }
    }

    pub fn select_prev_diff_file(&mut self) {
        if !self.diff_files.is_empty() {
            self.selected_diff_file_index =
                (self.selected_diff_file_index + self.diff_files.len() - 1) % self.diff_files.len();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::CoreClient;

    fn mission(worktree_path: Option<&str>) -> Mission {
        Mission {
            id: "mission-1".into(),
            title: "Test Mission".into(),
            status: "running".into(),
            autonomy_level: "co_pilot".into(),
            repo_channel_id: "repo-1".into(),
            worktree_path: worktree_path.map(String::from),
        }
    }

    fn repo() -> RepoChannel {
        RepoChannel {
            id: "repo-1".into(),
            name: "test-repo".into(),
            path: "/repos/test-repo".into(),
        }
    }

    fn app_with(mission: Option<Mission>) -> App {
        let mut app = App::new(CoreClient::new("127.0.0.1", 5919, None));
        app.repos = vec![repo()];
        if let Some(m) = mission {
            app.missions = vec![m];
            app.selected_mission_index = 0;
        }
        app
    }

    #[test]
    fn repo_path_prefers_the_missions_own_worktree() {
        let app = app_with(Some(mission(Some("/worktrees/mission-1"))));
        assert_eq!(
            app.repo_path_for_selected_mission().as_deref(),
            Some("/worktrees/mission-1")
        );
    }

    #[test]
    fn repo_path_falls_back_to_the_repo_channel_for_a_shared_clone_mission() {
        let app = app_with(Some(mission(None)));
        assert_eq!(
            app.repo_path_for_selected_mission().as_deref(),
            Some("/repos/test-repo")
        );
    }

    #[test]
    fn repo_path_is_none_when_no_mission_is_selected() {
        let app = app_with(None);
        assert_eq!(app.repo_path_for_selected_mission(), None);
    }

    #[test]
    fn diff_file_selection_wraps_in_both_directions() {
        let mut app = app_with(None);
        app.diff_files = vec![
            DiffFile {
                path: "a.rs".into(),
                status: "M".into(),
                hunks: vec![],
                additions: 1,
                deletions: 0,
            },
            DiffFile {
                path: "b.rs".into(),
                status: "M".into(),
                hunks: vec![],
                additions: 1,
                deletions: 0,
            },
        ];

        app.select_prev_diff_file();
        assert_eq!(app.selected_diff_file_index, 1, "wraps backward from 0");
        app.select_next_diff_file();
        assert_eq!(
            app.selected_diff_file_index, 0,
            "wraps forward from the last index"
        );
    }

    #[test]
    fn switching_missions_clears_stale_diff_state() {
        let mut app = App::new(CoreClient::new("127.0.0.1", 5919, None));
        app.repos = vec![repo()];
        app.missions = vec![mission(None), mission(None)];
        app.diff_files = vec![DiffFile {
            path: "a.rs".into(),
            status: "M".into(),
            hunks: vec![],
            additions: 1,
            deletions: 0,
        }];
        app.selected_diff_file_index = 0;

        app.select_next_mission();

        assert!(
            app.diff_files.is_empty(),
            "a stale diff from the previous Mission must not linger"
        );
        assert_eq!(app.selected_diff_file_index, 0);
    }

    async fn start_mock_core(diff_response: serde_json::Value) -> (String, u16) {
        let app = axum::Router::new().route(
            "/api/rpc",
            axum::routing::post(move |body: axum::Json<serde_json::Value>| {
                let diff_response = diff_response.clone();
                async move {
                    axum::Json(serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": body["id"],
                        "result": diff_response,
                    }))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap();
        });
        (addr.ip().to_string(), addr.port())
    }

    #[tokio::test]
    async fn refresh_diff_populates_files_from_a_real_response() {
        let (host, port) = start_mock_core(serde_json::json!([
            {
                "path": "src/main.rs",
                "old_path": null,
                "status": "M",
                "additions": 3,
                "deletions": 1,
                "hunks": [
                    {
                        "id": "hunk-1",
                        "file_path": "src/main.rs",
                        "old_start": 1,
                        "old_lines": 1,
                        "new_start": 1,
                        "new_lines": 3,
                        "header": "@@ -1,1 +1,3 @@",
                        "content": "+added line\n-removed line"
                    }
                ]
            }
        ]))
        .await;

        let mut app = App::new(CoreClient::new(&host, port, None));
        app.repos = vec![repo()];
        app.missions = vec![mission(None)];
        app.selected_mission_index = 0;

        app.refresh_diff().await;

        assert_eq!(app.diff_files.len(), 1);
        assert_eq!(app.diff_files[0].path, "src/main.rs");
        assert_eq!(app.diff_files[0].hunks[0].header, "@@ -1,1 +1,3 @@");
        assert!(app.last_error.is_none());
    }

    #[tokio::test]
    async fn refresh_diff_clears_stale_files_when_no_mission_is_selected() {
        let mut app = App::new(CoreClient::new("127.0.0.1", 5919, None));
        app.diff_files = vec![DiffFile {
            path: "stale.rs".into(),
            status: "M".into(),
            hunks: vec![],
            additions: 1,
            deletions: 0,
        }];

        app.refresh_diff().await;

        assert!(app.diff_files.is_empty());
    }
}
