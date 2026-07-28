use crate::api::types::*;
use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection};
use std::{path::PathBuf, sync::Mutex};

pub struct Persistence {
    conn: Mutex<Connection>,
}

/// Ordered schema migrations, applied by `run_migrations` after the base
/// `CREATE TABLE IF NOT EXISTS` batch. Append-only — a migration's position
/// in this array is its version number (1-indexed), tracked in
/// `PRAGMA user_version`. Never edit or reorder an existing entry once
/// released; add a new one.
///
/// Several of these columns are *also* present in the base `CREATE TABLE
/// settings` above, because that "ideal fresh schema" was updated after these
/// migrations were written for pre-existing databases. On a brand-new DB the
/// column already exists by the time its migration runs — `run_migrations`
/// tolerates exactly that one, specific, expected shape (SQLite's "duplicate
/// column name" error) and fails loudly on anything else, so a real error
/// (lock contention, a full disk, a genuine schema conflict) is never
/// silently swallowed the way a blanket `let _ =` would (050-Gold-Standard-
/// Review.md F3).
const MIGRATIONS: &[&str] = &[
    "ALTER TABLE settings ADD COLUMN openai_api_key TEXT",
    "ALTER TABLE settings ADD COLUMN openai_model TEXT",
    "ALTER TABLE settings ADD COLUMN google_api_key TEXT",
    "ALTER TABLE settings ADD COLUMN google_model TEXT",
    "ALTER TABLE settings ADD COLUMN openai_compatible_endpoint TEXT",
    "ALTER TABLE settings ADD COLUMN openai_compatible_api_key TEXT",
    "ALTER TABLE settings ADD COLUMN openai_compatible_model TEXT",
    "ALTER TABLE settings ADD COLUMN planner_provider TEXT",
    "ALTER TABLE settings ADD COLUMN planner_model TEXT",
    "ALTER TABLE settings ADD COLUMN implementer_provider TEXT",
    "ALTER TABLE settings ADD COLUMN implementer_model TEXT",
    "ALTER TABLE settings ADD COLUMN reviewer_provider TEXT",
    "ALTER TABLE settings ADD COLUMN reviewer_model TEXT",
    "ALTER TABLE settings ADD COLUMN github_token TEXT",
    "ALTER TABLE settings ADD COLUMN worktree_root TEXT",
    "ALTER TABLE settings ADD COLUMN theme TEXT DEFAULT 'dark'",
    // review_prompt.md §1.2: AGENTS.md is untrusted repo content until a
    // human explicitly reviews it — see connect_repo/get_repo_channel and
    // SkillsManager::build_system_context, which refuses to load it into
    // the system prompt while this is 0.
    "ALTER TABLE repo_channels ADD COLUMN agents_md_approved INTEGER NOT NULL DEFAULT 0",
];

/// True for SQLite's error when an `ALTER TABLE ... ADD COLUMN` targets a
/// column that already exists — the one error `run_migrations` tolerates,
/// because it means a migration's effect is already present via the base
/// schema (see `MIGRATIONS`'s doc comment), not that anything went wrong.
fn is_duplicate_column_error(err: &rusqlite::Error) -> bool {
    err.to_string().contains("duplicate column name")
}

/// Apply every migration in `MIGRATIONS` newer than the DB's current
/// `PRAGMA user_version`, inside a single transaction, then advance
/// `user_version` to `MIGRATIONS.len()`. Refuses to open a database stamped
/// with a version newer than this binary knows about, rather than running
/// queries against a schema shape it can't account for.
fn run_migrations(conn: &mut Connection) -> Result<()> {
    let current_version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let target_version = MIGRATIONS.len() as i64;

    if current_version > target_version {
        anyhow::bail!(
            "database schema version ({current_version}) is newer than this build of CID \
             supports (version {target_version}). Refusing to start against a database \
             written by a newer version — use a matching or newer build."
        );
    }

    if current_version == target_version {
        return Ok(());
    }

    let tx = conn.transaction()?;
    for (idx, sql) in MIGRATIONS.iter().enumerate() {
        let version = (idx + 1) as i64;
        if version <= current_version {
            continue;
        }
        if let Err(e) = tx.execute(sql, []) {
            if is_duplicate_column_error(&e) {
                continue;
            }
            return Err(e).with_context(|| format!("migration {version} failed: {sql}"));
        }
    }
    tx.pragma_update(None, "user_version", target_version)?;
    tx.commit()?;

    Ok(())
}

impl Persistence {
    pub fn new(db_path: Option<PathBuf>) -> Result<Self> {
        let path = db_path.unwrap_or_else(|| {
            let mut p = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
            p.push("cid");
            std::fs::create_dir_all(&p).ok();
            p.push("cid.db");
            p
        });

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&path).context(format!("Failed to open DB at {:?}", path))?;
        // WAL instead of the default rollback journal: a writer that's killed
        // abruptly (crash, forced termination, OS shutdown) leaves the WAL file
        // as the source of truth on next open rather than risking a partially
        // committed rollback journal. Found via real reproduction: a rollback-
        // journal-mode DB repeatedly force-killed during iterative local restarts
        // reached a state where a synchronously-seeded row (the default
        // workspace) was visible to some reads but not yet durable for an FK
        // check on the very next write, on process restart.
        conn.pragma_update(None, "journal_mode", "WAL")
            .context("failed to enable WAL journal mode")?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .context("failed to set synchronous=NORMAL")?;
        let persistence = Self {
            conn: Mutex::new(conn),
        };
        persistence.init_schema()?;
        Ok(persistence)
    }

    pub fn new_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let persistence = Self {
            conn: Mutex::new(conn),
        };
        persistence.init_schema()?;
        Ok(persistence)
    }

    fn init_schema(&self) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS workspaces (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                root_path TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS repo_channels (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                name TEXT NOT NULL,
                path TEXT NOT NULL UNIQUE,
                remote_url TEXT,
                agents_md_content TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY (workspace_id) REFERENCES workspaces(id)
            );

            CREATE TABLE IF NOT EXISTS missions (
                id TEXT PRIMARY KEY,
                repo_channel_id TEXT NOT NULL,
                title TEXT NOT NULL,
                task_description TEXT NOT NULL,
                session_mode TEXT NOT NULL,
                autonomy_level TEXT NOT NULL,
                status TEXT NOT NULL,
                worktree_path TEXT,
                branch_name TEXT,
                base_branch TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (repo_channel_id) REFERENCES repo_channels(id)
            );

            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                mission_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                tool_calls TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL,
                is_streaming INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY (mission_id) REFERENCES missions(id)
            );

            CREATE TABLE IF NOT EXISTS skills (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                content TEXT NOT NULL,
                scope TEXT NOT NULL,
                scope_id TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS mcp_servers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                transport_type TEXT NOT NULL,
                transport_config TEXT NOT NULL,
                status TEXT NOT NULL,
                enabled_for_repos TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                anthropic_api_key TEXT,
                anthropic_model TEXT NOT NULL DEFAULT 'claude-3-5-sonnet-20241022',
                openai_api_key TEXT,
                openai_model TEXT,
                google_api_key TEXT,
                google_model TEXT,
                openai_compatible_endpoint TEXT,
                openai_compatible_api_key TEXT,
                openai_compatible_model TEXT,
                worktree_root TEXT,
                theme TEXT NOT NULL DEFAULT 'dark',
                planner_provider TEXT,
                planner_model TEXT,
                implementer_provider TEXT,
                implementer_model TEXT,
                reviewer_provider TEXT,
                reviewer_model TEXT,
                github_token TEXT
            );

            INSERT OR IGNORE INTO settings (id, anthropic_model, theme) VALUES (1, 'claude-3-5-sonnet-20241022', 'dark');
            INSERT OR IGNORE INTO workspaces (id, name, root_path, created_at) VALUES ('default', 'Default Workspace', '', datetime('now'));

            CREATE TABLE IF NOT EXISTS github_configs (
                repo_path TEXT PRIMARY KEY,
                owner TEXT NOT NULL,
                repo TEXT NOT NULL,
                connected INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS mission_plans (
                mission_id TEXT PRIMARY KEY,
                id TEXT NOT NULL,
                content TEXT NOT NULL,
                status TEXT NOT NULL,
                approved_by TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS confidence_scores (
                id TEXT PRIMARY KEY,
                mission_id TEXT NOT NULL,
                patch_id TEXT NOT NULL,
                target_file TEXT NOT NULL,
                overall REAL NOT NULL,
                card_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_confidence_mission ON confidence_scores(mission_id);

            CREATE TABLE IF NOT EXISTS mission_reviews (
                id TEXT PRIMARY KEY,
                mission_id TEXT NOT NULL,
                verdict TEXT NOT NULL,
                findings TEXT NOT NULL,
                raw_output TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS mission_checkpoints (
                id TEXT PRIMARY KEY,
                mission_id TEXT NOT NULL,
                sha TEXT NOT NULL,
                label TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_checkpoints_mission ON mission_checkpoints(mission_id);

            CREATE TABLE IF NOT EXISTS forge_configs (
                repo_path TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                project TEXT NOT NULL,
                base_url TEXT,
                connected INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS tracker_links (
                id TEXT PRIMARY KEY,
                mission_id TEXT NOT NULL,
                tracker TEXT NOT NULL,
                issue_key TEXT NOT NULL,
                url TEXT NOT NULL,
                title TEXT,
                created_at TEXT NOT NULL,
                UNIQUE(mission_id, tracker, issue_key)
            );

            CREATE INDEX IF NOT EXISTS idx_tracker_links_mission ON tracker_links(mission_id);

            CREATE TABLE IF NOT EXISTS deployment_records (
                id TEXT PRIMARY KEY,
                mission_id TEXT NOT NULL,
                environment TEXT NOT NULL,
                commit_or_tag TEXT NOT NULL,
                ci_run_url TEXT,
                note TEXT,
                source TEXT NOT NULL,
                deployed_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_deployments_mission ON deployment_records(mission_id);

            CREATE TABLE IF NOT EXISTS role_profiles (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT NOT NULL,
                scope TEXT NOT NULL,
                scope_id TEXT NOT NULL,
                system_prompt TEXT NOT NULL,
                model_provider TEXT,
                model_id TEXT,
                allowed_tools TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_role_profiles_scope ON role_profiles(scope, scope_id);

            CREATE TABLE IF NOT EXISTS users (
                id TEXT PRIMARY KEY,
                username TEXT NOT NULL UNIQUE,
                password_hash TEXT NOT NULL,
                role TEXT NOT NULL,
                active INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS sessions (
                token TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);
            CREATE INDEX IF NOT EXISTS idx_reviews_mission ON mission_reviews(mission_id);
            CREATE INDEX IF NOT EXISTS idx_missions_repo ON missions(repo_channel_id);
            CREATE INDEX IF NOT EXISTS idx_messages_mission ON messages(mission_id);
            CREATE INDEX IF NOT EXISTS idx_messages_created ON messages(created_at);
            "#,
        )?;

        run_migrations(&mut conn)?;

        Ok(())
    }

    // Workspace
    pub fn list_workspaces(&self) -> Result<Vec<Workspace>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, name, root_path, created_at FROM workspaces")?;
        let rows = stmt.query_map([], |row| {
            Ok(Workspace {
                id: row.get(0)?,
                name: row.get(1)?,
                root_path: row.get(2)?,
                created_at: row
                    .get::<_, String>(3)?
                    .parse()
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_workspace(&self, id: &str) -> Result<Workspace> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT id, name, root_path, created_at FROM workspaces WHERE id = ?")?;
        let ws = stmt.query_row(params![id], |row| {
            Ok(Workspace {
                id: row.get(0)?,
                name: row.get(1)?,
                root_path: row.get(2)?,
                created_at: row
                    .get::<_, String>(3)?
                    .parse()
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?;
        Ok(ws)
    }

    // Repo Channels
    /// Connects (or reconnects) a repo by filesystem path. `path` is
    /// `UNIQUE`, so a reconnect must update the existing row in place rather
    /// than mint a fresh id — `INSERT OR REPLACE` would delete-then-reinsert
    /// on a `path` conflict, and that delete violates the `missions.
    /// repo_channel_id` foreign key for any Mission already created against
    /// this repo, breaking every existing Mission the moment the user
    /// reconnects the same repo (e.g. after restarting Core).
    pub fn connect_repo(&self, path: &str, workspace_id: Option<&str>) -> Result<RepoChannel> {
        let ws_id = workspace_id.unwrap_or("default");
        let name = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("repo")
            .to_string();
        let new_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        // Try to get remote url via git2
        let remote_url = crate::git::get_remote_url(path).ok();

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO repo_channels (id, workspace_id, name, path, remote_url, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(path) DO UPDATE SET
                workspace_id = excluded.workspace_id,
                name = excluded.name,
                remote_url = excluded.remote_url",
            params![new_id, ws_id, name, path, remote_url, now],
        )?;
        let id: String = conn.query_row(
            "SELECT id FROM repo_channels WHERE path = ?1",
            params![path],
            |row| row.get(0),
        )?;

        drop(conn);
        self.get_repo_channel(&id)
    }

    pub fn list_repo_channels(&self) -> Result<Vec<RepoChannel>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, workspace_id, name, path, remote_url, agents_md_content, created_at, agents_md_approved FROM repo_channels ORDER BY created_at DESC")?;
        let rows = stmt.query_map([], |row| {
            Ok(RepoChannel {
                id: row.get(0)?,
                workspace_id: row.get(1)?,
                name: row.get(2)?,
                path: row.get(3)?,
                remote_url: row.get(4)?,
                agents_md_content: row.get(5)?,
                created_at: row
                    .get::<_, String>(6)?
                    .parse()
                    .unwrap_or_else(|_| Utc::now()),
                agents_md_approved: row.get::<_, i32>(7)? != 0,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// review_prompt.md §1.2: a repo's AGENTS.md is untrusted repo content on
    /// first connect — a human must explicitly approve it before
    /// `SkillsManager::build_system_context` will load it into the system
    /// prompt. Known limitation, documented in SECURITY.md rather than
    /// hidden: approval does not re-arm if AGENTS.md changes later (e.g. a
    /// subsequent `git pull`), so a repo trusted once stays trusted even if
    /// its AGENTS.md is edited afterward.
    pub fn approve_agents_md(&self, repo_channel_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE repo_channels SET agents_md_approved = 1 WHERE id = ?1",
            params![repo_channel_id],
        )?;
        Ok(())
    }

    pub fn get_repo_channel(&self, id: &str) -> Result<RepoChannel> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, workspace_id, name, path, remote_url, agents_md_content, created_at, agents_md_approved FROM repo_channels WHERE id = ?")?;
        let repo = stmt.query_row(params![id], |row| {
            Ok(RepoChannel {
                id: row.get(0)?,
                workspace_id: row.get(1)?,
                name: row.get(2)?,
                path: row.get(3)?,
                remote_url: row.get(4)?,
                agents_md_content: row.get(5)?,
                created_at: row
                    .get::<_, String>(6)?
                    .parse()
                    .unwrap_or_else(|_| Utc::now()),
                agents_md_approved: row.get::<_, i32>(7)? != 0,
            })
        })?;
        Ok(repo)
    }

    pub fn disconnect_repo(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM repo_channels WHERE id = ?", params![id])?;
        Ok(())
    }

    // Missions
    pub fn create_mission(
        &self,
        repo_channel_id: &str,
        title: &str,
        task: &str,
        session_mode: SessionMode,
        autonomy_level: AutonomyLevel,
    ) -> Result<Mission> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let session_str = serde_json::to_string(&session_mode)?
            .trim_matches('"')
            .to_string();
        let autonomy_str = serde_json::to_string(&autonomy_level)?
            .trim_matches('"')
            .to_string();

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO missions (id, repo_channel_id, title, task_description, session_mode, autonomy_level, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'created', ?7, ?8)",
            params![id, repo_channel_id, title, task, session_str, autonomy_str, now_str, now_str],
        )?;
        drop(conn);
        self.get_mission(&id)
    }

    pub fn list_missions(&self, repo_channel_id: Option<&str>) -> Result<Vec<Mission>> {
        let conn = self.conn.lock().unwrap();
        let (sql, param): (String, Option<String>) = if let Some(rc_id) = repo_channel_id {
            ("SELECT id, repo_channel_id, title, task_description, session_mode, autonomy_level, status, worktree_path, branch_name, base_branch, created_at, updated_at FROM missions WHERE repo_channel_id = ? ORDER BY created_at DESC".to_string(), Some(rc_id.to_string()))
        } else {
            ("SELECT id, repo_channel_id, title, task_description, session_mode, autonomy_level, status, worktree_path, branch_name, base_branch, created_at, updated_at FROM missions ORDER BY created_at DESC".to_string(), None)
        };

        let mut stmt = conn.prepare(&sql)?;
        let rows: Vec<Mission> = if let Some(rc) = param {
            let mapped = stmt.query_map(params![rc], parse_mission_row)?;
            mapped.filter_map(|r| r.ok().and_then(|opt| opt)).collect()
        } else {
            let mapped = stmt.query_map([], parse_mission_row)?;
            mapped.filter_map(|r| r.ok().and_then(|opt| opt)).collect()
        };
        Ok(rows)
    }

    pub fn get_mission(&self, id: &str) -> Result<Mission> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, repo_channel_id, title, task_description, session_mode, autonomy_level, status, worktree_path, branch_name, base_branch, created_at, updated_at FROM missions WHERE id = ?")?;
        let mission = stmt
            .query_row(params![id], |row| {
                parse_mission_row(row).map(|opt| opt.expect("mission parse"))
            })
            .with_context(|| format!("No mission with id {id}"))?;
        Ok(mission)
    }

    pub fn update_mission_worktree(
        &self,
        id: &str,
        worktree_path: Option<String>,
        branch_name: Option<String>,
    ) -> Result<Mission> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE missions SET worktree_path = ?1, branch_name = ?2, updated_at = ?3 WHERE id = ?4",
            params![worktree_path, branch_name, now, id],
        )?;
        drop(conn);
        self.get_mission(id)
    }

    pub fn update_mission_status(&self, id: &str, status: MissionStatus) -> Result<Mission> {
        // Convert to snake_case via serde
        let status_str = serde_json::to_string(&status)?
            .trim_matches('"')
            .to_string();
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE missions SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status_str, now, id],
        )?;
        drop(conn);
        self.get_mission(id)
    }

    // Messages
    pub fn create_message(
        &self,
        mission_id: &str,
        role: MessageRole,
        content: &str,
        tool_calls: Vec<ToolCall>,
    ) -> Result<ChatMessage> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let role_str = serde_json::to_string(&role)?.trim_matches('"').to_string();
        let tool_calls_json = serde_json::to_string(&tool_calls)?;

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO messages (id, mission_id, role, content, tool_calls, created_at, is_streaming) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
            params![id, mission_id, role_str, content, tool_calls_json, now_str],
        )?;
        drop(conn);
        Ok(ChatMessage {
            id,
            mission_id: mission_id.to_string(),
            role,
            content: content.to_string(),
            tool_calls,
            created_at: now,
            is_streaming: false,
        })
    }

    pub fn list_messages(&self, mission_id: &str) -> Result<Vec<ChatMessage>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, mission_id, role, content, tool_calls, created_at, is_streaming FROM messages WHERE mission_id = ? ORDER BY created_at ASC")?;
        let rows = stmt.query_map(params![mission_id], |row| {
            let role_str: String = row.get(2)?;
            let role: MessageRole =
                serde_json::from_str(&format!("\"{}\"", role_str)).unwrap_or(MessageRole::User);
            let tool_calls_str: String = row.get(4)?;
            let tool_calls: Vec<ToolCall> =
                serde_json::from_str(&tool_calls_str).unwrap_or_default();
            let created_at_str: String = row.get(5)?;
            let created_at = created_at_str.parse().unwrap_or_else(|_| Utc::now());
            Ok(ChatMessage {
                id: row.get(0)?,
                mission_id: row.get(1)?,
                role,
                content: row.get(3)?,
                tool_calls,
                created_at,
                is_streaming: row.get::<_, i32>(6)? != 0,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // Skills
    pub fn list_skills(&self, scope: Option<&str>) -> Result<Vec<Skill>> {
        let conn = self.conn.lock().unwrap();
        let sql = if let Some(s) = scope {
            format!("SELECT id, name, content, scope, scope_id, created_at, updated_at FROM skills WHERE scope = '{}' ORDER BY updated_at DESC", s)
        } else {
            "SELECT id, name, content, scope, scope_id, created_at, updated_at FROM skills ORDER BY updated_at DESC".to_string()
        };
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            let scope_str: String = row.get(3)?;
            let scope_enum: SkillScope = serde_json::from_str(&format!("\"{}\"", scope_str))
                .unwrap_or(SkillScope::Workspace);
            Ok(Skill {
                id: row.get(0)?,
                name: row.get(1)?,
                content: row.get(2)?,
                scope: scope_enum,
                scope_id: row.get(4)?,
                created_at: row
                    .get::<_, String>(5)?
                    .parse()
                    .unwrap_or_else(|_| Utc::now()),
                updated_at: row
                    .get::<_, String>(6)?
                    .parse()
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn save_skill(&self, skill: &Skill) -> Result<Skill> {
        let conn = self.conn.lock().unwrap();
        let scope_str = serde_json::to_string(&skill.scope)?
            .trim_matches('"')
            .to_string();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT OR REPLACE INTO skills (id, name, content, scope, scope_id, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![skill.id, skill.name, skill.content, scope_str, skill.scope_id, skill.created_at.to_rfc3339(), now],
        )?;
        drop(conn);
        Ok(skill.clone())
    }

    // MCP servers persistence
    pub fn save_mcp_server(&self, server: &McpServer) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let transport_str = serde_json::to_string(&server.transport_type)?
            .trim_matches('"')
            .to_string();
        let status_str = serde_json::to_string(&server.status)?
            .trim_matches('"')
            .to_string();
        let config_str = serde_json::to_string(&server.transport_config)?;
        let repos_str = serde_json::to_string(&server.enabled_for_repos)?;
        conn.execute(
            "INSERT OR REPLACE INTO mcp_servers (id, name, transport_type, transport_config, status, enabled_for_repos, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![server.id, server.name, transport_str, config_str, status_str, repos_str, server.created_at.to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn list_mcp_servers(&self) -> Result<Vec<McpServer>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, name, transport_type, transport_config, status, enabled_for_repos, created_at FROM mcp_servers")?;
        let rows = stmt.query_map([], |row| {
            let transport_str: String = row.get(2)?;
            let transport_type: McpTransportType =
                serde_json::from_str(&format!("\"{}\"", transport_str))
                    .unwrap_or(McpTransportType::Stdio);
            let status_str: String = row.get(4)?;
            let status: McpServerStatus = serde_json::from_str(&format!("\"{}\"", status_str))
                .unwrap_or(McpServerStatus::Disconnected);
            let config_str: String = row.get(3)?;
            let config: serde_json::Value =
                serde_json::from_str(&config_str).unwrap_or(serde_json::json!({}));
            let repos_str: String = row.get(5)?;
            let repos: Vec<String> = serde_json::from_str(&repos_str).unwrap_or_default();
            Ok(McpServer {
                id: row.get(0)?,
                name: row.get(1)?,
                transport_type,
                transport_config: config,
                status,
                enabled_for_repos: repos,
                created_at: row
                    .get::<_, String>(6)?
                    .parse()
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn delete_mcp_server(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM mcp_servers WHERE id = ?", params![id])?;
        Ok(())
    }

    // Settings - Phase 1 multi-provider with migration tolerance
    pub fn get_settings(&self) -> Result<Settings> {
        let conn = self.conn.lock().unwrap();

        // Try full column set first; if fails due to missing columns (old DB without migration), fallback
        let full_sql = "SELECT anthropic_api_key, anthropic_model, openai_api_key, openai_model, google_api_key, google_model, openai_compatible_endpoint, openai_compatible_api_key, openai_compatible_model, worktree_root, theme, planner_provider, planner_model, implementer_provider, implementer_model, reviewer_provider, reviewer_model, github_token FROM settings WHERE id = 1";

        let attempt = conn.prepare(full_sql);
        let settings = if let Ok(mut stmt) = attempt {
            stmt.query_row([], |row| {
                Ok(Settings {
                    anthropic_api_key: row.get(0)?,
                    anthropic_model: row.get(1)?,
                    openai_api_key: row.get(2)?,
                    openai_model: row.get(3)?,
                    google_api_key: row.get(4)?,
                    google_model: row.get(5)?,
                    openai_compatible_endpoint: row.get(6)?,
                    openai_compatible_api_key: row.get(7)?,
                    openai_compatible_model: row.get(8)?,
                    worktree_root: row.get(9)?,
                    theme: row.get(10)?,
                    planner_provider: row.get(11)?,
                    planner_model: row.get(12)?,
                    implementer_provider: row.get(13)?,
                    implementer_model: row.get(14)?,
                    reviewer_provider: row.get(15)?,
                    reviewer_model: row.get(16)?,
                    github_token: row.get(17)?,
                })
            })
        } else {
            // Fallback: old schema (4 columns)
            let mut stmt = conn.prepare("SELECT anthropic_api_key, anthropic_model, worktree_root, theme FROM settings WHERE id = 1")?;
            stmt.query_row([], |row| {
                Ok(Settings {
                    anthropic_api_key: row.get(0)?,
                    anthropic_model: row.get(1)?,
                    openai_api_key: None,
                    openai_model: None,
                    google_api_key: None,
                    google_model: None,
                    openai_compatible_endpoint: None,
                    openai_compatible_api_key: None,
                    openai_compatible_model: None,
                    worktree_root: row.get(2)?,
                    theme: row.get(3)?,
                    planner_provider: None,
                    planner_model: None,
                    implementer_provider: None,
                    implementer_model: None,
                    reviewer_provider: None,
                    reviewer_model: None,
                    github_token: None,
                })
            })
        };

        settings.map_err(|e| anyhow::anyhow!("get_settings failed: {}", e))
    }

    pub fn update_settings(&self, settings: &Settings) -> Result<Settings> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE settings SET anthropic_api_key = ?1, anthropic_model = ?2, openai_api_key = ?3, openai_model = ?4, google_api_key = ?5, google_model = ?6, openai_compatible_endpoint = ?7, openai_compatible_api_key = ?8, openai_compatible_model = ?9, worktree_root = ?10, theme = ?11, planner_provider = ?12, planner_model = ?13, implementer_provider = ?14, implementer_model = ?15, reviewer_provider = ?16, reviewer_model = ?17, github_token = ?18 WHERE id = 1",
            params![
                settings.anthropic_api_key,
                settings.anthropic_model,
                settings.openai_api_key,
                settings.openai_model,
                settings.google_api_key,
                settings.google_model,
                settings.openai_compatible_endpoint,
                settings.openai_compatible_api_key,
                settings.openai_compatible_model,
                settings.worktree_root,
                settings.theme,
                settings.planner_provider,
                settings.planner_model,
                settings.implementer_provider,
                settings.implementer_model,
                settings.reviewer_provider,
                settings.reviewer_model,
                settings.github_token,
            ],
        )?;
        drop(conn);
        self.get_settings()
    }

    // GitHub configs
    pub fn save_github_config(&self, config: &GitHubConfig) -> Result<GitHubConfig> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT OR REPLACE INTO github_configs (repo_path, owner, repo, connected, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![config.repo_path, config.owner, config.repo, if config.connected { 1 } else { 0 }, now, now],
        )?;
        drop(conn);
        let cfg = self
            .get_github_config(&config.repo_path)?
            .ok_or_else(|| anyhow::anyhow!("Failed to retrieve saved GitHub config"))?;
        Ok(cfg)
    }

    pub fn get_github_config(&self, repo_path: &str) -> Result<Option<GitHubConfig>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT repo_path, owner, repo, connected FROM github_configs WHERE repo_path = ?1",
        )?;
        let result = stmt.query_row(params![repo_path], |row| {
            let repo_path: String = row.get(0)?;
            let owner: String = row.get(1)?;
            let repo: String = row.get(2)?;
            let connected: i32 = row.get(3)?;
            Ok(GitHubConfig {
                repo_path,
                owner,
                repo,
                connected: connected != 0,
                has_token: false, // determined by caller via keyring check
            })
        });
        match result {
            Ok(cfg) => Ok(Some(cfg)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn get_github_config_with_token_check<F>(
        &self,
        repo_path: &str,
        has_token_fn: F,
    ) -> Result<Option<GitHubConfig>>
    where
        F: Fn(&str, &str) -> bool,
    {
        if let Some(mut cfg) = self.get_github_config(repo_path)? {
            cfg.has_token = has_token_fn(&cfg.owner, &cfg.repo);
            Ok(Some(cfg))
        } else {
            Ok(None)
        }
    }

    pub fn list_github_configs(&self) -> Result<Vec<GitHubConfig>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT repo_path, owner, repo, connected FROM github_configs")?;
        let rows = stmt.query_map([], |row| {
            Ok(GitHubConfig {
                repo_path: row.get(0)?,
                owner: row.get(1)?,
                repo: row.get(2)?,
                connected: row.get::<_, i32>(3)? != 0,
                has_token: false,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn delete_github_config(&self, repo_path: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM github_configs WHERE repo_path = ?1",
            params![repo_path],
        )?;
        Ok(())
    }

    pub fn get_repo_channel_by_path(&self, path: &str) -> Result<RepoChannel> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, workspace_id, name, path, remote_url, agents_md_content, created_at, agents_md_approved FROM repo_channels WHERE path = ?1")?;
        let repo = stmt.query_row(params![path], |row| {
            Ok(RepoChannel {
                id: row.get(0)?,
                workspace_id: row.get(1)?,
                name: row.get(2)?,
                path: row.get(3)?,
                remote_url: row.get(4)?,
                agents_md_content: row.get(5)?,
                created_at: row
                    .get::<_, String>(6)?
                    .parse()
                    .unwrap_or_else(|_| chrono::Utc::now()),
                agents_md_approved: row.get::<_, i32>(7)? != 0,
            })
        })?;
        Ok(repo)
    }

    pub fn update_message_content(&self, message_id: &str, content: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE messages SET content = ?1 WHERE id = ?2",
            params![content, message_id],
        )?;
        Ok(())
    }

    /// Attaches the tool calls a provider requested during this message's turn,
    /// once the turn's stream has finished and they're known — the row is
    /// created empty (for delta streaming) before any tool call exists.
    pub fn update_message_tool_calls(
        &self,
        message_id: &str,
        tool_calls: &[ToolCall],
    ) -> Result<()> {
        let tool_calls_json = serde_json::to_string(tool_calls)?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE messages SET tool_calls = ?1 WHERE id = ?2",
            params![tool_calls_json, message_id],
        )?;
        Ok(())
    }

    // ---- Mission plans (Planner) ----

    /// Write the Mission's plan text. Any status other than Draft is reset,
    /// because an approval applied to the previous text, not this one.
    pub fn upsert_mission_plan(&self, mission_id: &str, content: &str) -> Result<MissionPlan> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now();
        let existing_id: Option<String> = conn
            .query_row(
                "SELECT id FROM mission_plans WHERE mission_id = ?1",
                params![mission_id],
                |r| r.get(0),
            )
            .ok();
        let id = existing_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let status = enum_str(&MissionPlanStatus::Draft);

        conn.execute(
            "INSERT INTO mission_plans (mission_id, id, content, status, approved_by, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?5)
             ON CONFLICT(mission_id) DO UPDATE SET
                content = excluded.content,
                status = excluded.status,
                approved_by = NULL,
                updated_at = excluded.updated_at",
            params![mission_id, id, content, status, now.to_rfc3339()],
        )?;
        drop(conn);

        self.get_mission_plan(mission_id)?
            .ok_or_else(|| anyhow::anyhow!("plan write did not persist"))
    }

    pub fn get_mission_plan(&self, mission_id: &str) -> Result<Option<MissionPlan>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, mission_id, content, status, approved_by, created_at, updated_at
             FROM mission_plans WHERE mission_id = ?1",
        )?;
        let mut rows = stmt.query_map(params![mission_id], |row| {
            Ok(MissionPlan {
                id: row.get(0)?,
                mission_id: row.get(1)?,
                content: row.get(2)?,
                status: parse_enum(&row.get::<_, String>(3)?, MissionPlanStatus::Draft),
                approved_by: row.get(4)?,
                created_at: parse_ts(&row.get::<_, String>(5)?),
                updated_at: parse_ts(&row.get::<_, String>(6)?),
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn set_mission_plan_status(
        &self,
        mission_id: &str,
        status: MissionPlanStatus,
        approved_by: Option<&str>,
    ) -> Result<MissionPlan> {
        {
            let conn = self.conn.lock().unwrap();
            let changed = conn.execute(
                "UPDATE mission_plans SET status = ?1, approved_by = ?2, updated_at = ?3 WHERE mission_id = ?4",
                params![enum_str(&status), approved_by, Utc::now().to_rfc3339(), mission_id],
            )?;
            if changed == 0 {
                anyhow::bail!("Mission {} has no plan", mission_id);
            }
        }
        self.get_mission_plan(mission_id)?
            .ok_or_else(|| anyhow::anyhow!("plan disappeared after status update"))
    }

    // ---- Mission reviews (Reviewer) ----

    pub fn save_mission_review(&self, review: &MissionReview) -> Result<MissionReview> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO mission_reviews (id, mission_id, verdict, findings, raw_output, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                review.id,
                review.mission_id,
                enum_str(&review.verdict),
                serde_json::to_string(&review.findings)?,
                review.raw_output,
                review.created_at.to_rfc3339(),
            ],
        )?;
        Ok(review.clone())
    }

    pub fn get_latest_mission_review(&self, mission_id: &str) -> Result<Option<MissionReview>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, mission_id, verdict, findings, raw_output, created_at
             FROM mission_reviews WHERE mission_id = ?1 ORDER BY created_at DESC LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![mission_id], |row| {
            let findings_json: String = row.get(3)?;
            Ok(MissionReview {
                id: row.get(0)?,
                mission_id: row.get(1)?,
                verdict: parse_enum(&row.get::<_, String>(2)?, ReviewVerdict::NotRun),
                findings: serde_json::from_str(&findings_json).unwrap_or_default(),
                raw_output: row.get(4)?,
                created_at: parse_ts(&row.get::<_, String>(5)?),
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn list_mission_reviews(&self, mission_id: &str) -> Result<Vec<MissionReview>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, mission_id, verdict, findings, raw_output, created_at
             FROM mission_reviews WHERE mission_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![mission_id], |row| {
            let findings_json: String = row.get(3)?;
            Ok(MissionReview {
                id: row.get(0)?,
                mission_id: row.get(1)?,
                verdict: parse_enum(&row.get::<_, String>(2)?, ReviewVerdict::NotRun),
                findings: serde_json::from_str(&findings_json).unwrap_or_default(),
                raw_output: row.get(4)?,
                created_at: parse_ts(&row.get::<_, String>(5)?),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ---- Mission checkpoints (review_prompt.md §3.2) ----

    pub fn create_checkpoint(
        &self,
        mission_id: &str,
        sha: &str,
        label: &str,
    ) -> Result<MissionCheckpoint> {
        let checkpoint = MissionCheckpoint {
            id: new_id(),
            mission_id: mission_id.to_string(),
            sha: sha.to_string(),
            label: label.to_string(),
            created_at: Utc::now(),
        };
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO mission_checkpoints (id, mission_id, sha, label, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                checkpoint.id,
                checkpoint.mission_id,
                checkpoint.sha,
                checkpoint.label,
                checkpoint.created_at.to_rfc3339(),
            ],
        )?;
        Ok(checkpoint)
    }

    pub fn list_checkpoints(&self, mission_id: &str) -> Result<Vec<MissionCheckpoint>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, mission_id, sha, label, created_at
             FROM mission_checkpoints WHERE mission_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![mission_id], |row| {
            Ok(MissionCheckpoint {
                id: row.get(0)?,
                mission_id: row.get(1)?,
                sha: row.get(2)?,
                label: row.get(3)?,
                created_at: parse_ts(&row.get::<_, String>(4)?),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_checkpoint(&self, id: &str) -> Result<MissionCheckpoint> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, mission_id, sha, label, created_at
             FROM mission_checkpoints WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(MissionCheckpoint {
                id: row.get(0)?,
                mission_id: row.get(1)?,
                sha: row.get(2)?,
                label: row.get(3)?,
                created_at: parse_ts(&row.get::<_, String>(4)?),
            })
        })?;
        rows.next()
            .transpose()?
            .ok_or_else(|| anyhow::anyhow!("Checkpoint {} not found", id))
    }
}

// ---- Forge remotes and tracker links (Phase 3, Part 16) ----

impl Persistence {
    pub fn save_forge_config(&self, config: &crate::forges::ForgeConfig) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO forge_configs (repo_path, kind, project, base_url, connected, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT(repo_path) DO UPDATE SET
                kind = excluded.kind,
                project = excluded.project,
                base_url = excluded.base_url,
                connected = excluded.connected,
                updated_at = excluded.updated_at",
            params![
                config.repo_path,
                config.kind.as_str(),
                config.project,
                config.base_url,
                if config.connected { 1 } else { 0 },
                now
            ],
        )?;
        Ok(())
    }

    pub fn get_forge_config(&self, repo_path: &str) -> Result<Option<crate::forges::ForgeConfig>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT repo_path, kind, project, base_url, connected FROM forge_configs WHERE repo_path = ?1",
        )?;
        let mut rows = stmt.query_map(params![repo_path], |row| {
            let kind_str: String = row.get(1)?;
            let connected: i64 = row.get(4)?;
            Ok(crate::forges::ForgeConfig {
                repo_path: row.get(0)?,
                kind: crate::forges::ForgeKind::parse(&kind_str)
                    .unwrap_or(crate::forges::ForgeKind::GitLab),
                project: row.get(2)?,
                base_url: row.get(3)?,
                connected: connected != 0,
                has_token: false,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn list_forge_configs(&self) -> Result<Vec<crate::forges::ForgeConfig>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT repo_path, kind, project, base_url, connected FROM forge_configs")?;
        let rows = stmt.query_map([], |row| {
            let kind_str: String = row.get(1)?;
            let connected: i64 = row.get(4)?;
            Ok(crate::forges::ForgeConfig {
                repo_path: row.get(0)?,
                kind: crate::forges::ForgeKind::parse(&kind_str)
                    .unwrap_or(crate::forges::ForgeKind::GitLab),
                project: row.get(2)?,
                base_url: row.get(3)?,
                connected: connected != 0,
                has_token: false,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn delete_forge_config(&self, repo_path: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM forge_configs WHERE repo_path = ?1",
            params![repo_path],
        )?;
        Ok(())
    }

    pub fn save_tracker_link(&self, link: &crate::trackers::TrackerLink) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO tracker_links (id, mission_id, tracker, issue_key, url, title, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(mission_id, tracker, issue_key) DO UPDATE SET
                url = excluded.url,
                title = excluded.title",
            params![
                link.id,
                link.mission_id,
                link.tracker.as_str(),
                link.issue_key,
                link.url,
                link.title,
                link.created_at.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn list_tracker_links(
        &self,
        mission_id: &str,
    ) -> Result<Vec<crate::trackers::TrackerLink>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, mission_id, tracker, issue_key, url, title, created_at
             FROM tracker_links WHERE mission_id = ?1 ORDER BY created_at",
        )?;
        let rows = stmt.query_map(params![mission_id], |row| {
            let tracker_str: String = row.get(2)?;
            Ok(crate::trackers::TrackerLink {
                id: row.get(0)?,
                mission_id: row.get(1)?,
                tracker: crate::trackers::Tracker::parse(&tracker_str)
                    .unwrap_or(crate::trackers::Tracker::Jira),
                issue_key: row.get(3)?,
                url: row.get(4)?,
                title: row.get(5)?,
                created_at: parse_ts(&row.get::<_, String>(6)?),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn delete_tracker_link(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM tracker_links WHERE id = ?1", params![id])?;
        Ok(())
    }
}

// ---- Deployment records (Phase 4) ----

impl Persistence {
    pub fn save_deployment_record(
        &self,
        record: &crate::decisions::DeploymentRecord,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO deployment_records (id, mission_id, environment, commit_or_tag, ci_run_url, note, source, deployed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                record.id,
                record.mission_id,
                record.environment,
                record.commit_or_tag,
                record.ci_run_url,
                record.note,
                enum_str(&record.source),
                record.deployed_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_deployment_records(
        &self,
        mission_id: &str,
    ) -> Result<Vec<crate::decisions::DeploymentRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, mission_id, environment, commit_or_tag, ci_run_url, note, source, deployed_at
             FROM deployment_records WHERE mission_id = ?1 ORDER BY deployed_at DESC",
        )?;
        let rows = stmt.query_map(params![mission_id], |row| {
            let source_str: String = row.get(6)?;
            Ok(crate::decisions::DeploymentRecord {
                id: row.get(0)?,
                mission_id: row.get(1)?,
                environment: row.get(2)?,
                commit_or_tag: row.get(3)?,
                ci_run_url: row.get(4)?,
                note: row.get(5)?,
                source: parse_enum(&source_str, crate::decisions::DeploymentSource::Manual),
                deployed_at: parse_ts(&row.get::<_, String>(7)?),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

// ---- Role profiles (Phase 4) ----

impl Persistence {
    pub fn create_role_profile(
        &self,
        input: crate::role_profiles::RoleProfileInput,
    ) -> Result<crate::role_profiles::RoleProfile> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO role_profiles (id, name, description, scope, scope_id, system_prompt, model_provider, model_id, allowed_tools, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
            params![
                id,
                input.name,
                input.description,
                enum_str(&input.scope),
                input.scope_id,
                input.system_prompt,
                input.model_provider,
                input.model_id,
                serde_json::to_string(&input.allowed_tools)?,
                now.to_rfc3339(),
            ],
        )?;
        drop(conn);
        self.get_role_profile(&id)
    }

    pub fn update_role_profile(
        &self,
        id: &str,
        input: crate::role_profiles::RoleProfileInput,
    ) -> Result<crate::role_profiles::RoleProfile> {
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute(
            "UPDATE role_profiles SET name=?1, description=?2, scope=?3, scope_id=?4, system_prompt=?5,
                model_provider=?6, model_id=?7, allowed_tools=?8, updated_at=?9 WHERE id=?10",
            params![
                input.name,
                input.description,
                enum_str(&input.scope),
                input.scope_id,
                input.system_prompt,
                input.model_provider,
                input.model_id,
                serde_json::to_string(&input.allowed_tools)?,
                Utc::now().to_rfc3339(),
                id,
            ],
        )?;
        if changed == 0 {
            anyhow::bail!("No role profile with id {id}");
        }
        drop(conn);
        self.get_role_profile(id)
    }

    pub fn delete_role_profile(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute("DELETE FROM role_profiles WHERE id = ?1", params![id])?;
        if changed == 0 {
            anyhow::bail!("No role profile with id {id}");
        }
        Ok(())
    }

    pub fn get_role_profile(&self, id: &str) -> Result<crate::role_profiles::RoleProfile> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, name, description, scope, scope_id, system_prompt, model_provider, model_id, allowed_tools, created_at, updated_at
             FROM role_profiles WHERE id = ?1",
            params![id],
            row_to_role_profile,
        )
        .context(format!("No role profile with id {id}"))
    }

    pub fn list_role_profiles(
        &self,
        scope: crate::role_profiles::ProfileScope,
        scope_id: &str,
    ) -> Result<Vec<crate::role_profiles::RoleProfile>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, description, scope, scope_id, system_prompt, model_provider, model_id, allowed_tools, created_at, updated_at
             FROM role_profiles WHERE scope = ?1 AND scope_id = ?2 ORDER BY created_at",
        )?;
        let rows = stmt.query_map(params![enum_str(&scope), scope_id], row_to_role_profile)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

fn row_to_role_profile(row: &rusqlite::Row) -> rusqlite::Result<crate::role_profiles::RoleProfile> {
    let scope_str: String = row.get(3)?;
    let allowed_tools_json: String = row.get(8)?;
    Ok(crate::role_profiles::RoleProfile {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        scope: parse_enum(&scope_str, crate::role_profiles::ProfileScope::Repo),
        scope_id: row.get(4)?,
        system_prompt: row.get(5)?,
        model_provider: row.get(6)?,
        model_id: row.get(7)?,
        allowed_tools: serde_json::from_str(&allowed_tools_json).unwrap_or_default(),
        created_at: parse_ts(&row.get::<_, String>(9)?),
        updated_at: parse_ts(&row.get::<_, String>(10)?),
    })
}

// ---- Confidence scores (Phase 4) ----

impl Persistence {
    pub fn save_confidence_score(
        &self,
        mission_id: &str,
        target_file: &str,
        card: &crate::confidence::ConfidenceScore,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO confidence_scores (id, mission_id, patch_id, target_file, overall, card_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                uuid::Uuid::new_v4().to_string(),
                mission_id,
                card.patch_id,
                target_file,
                card.overall,
                serde_json::to_string(card)?,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_confidence_scores(
        &self,
        mission_id: &str,
    ) -> Result<Vec<crate::confidence::ConfidenceScore>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT card_json FROM confidence_scores WHERE mission_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![mission_id], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            let json = row?;
            out.push(serde_json::from_str(&json)?);
        }
        Ok(out)
    }
}

// ---- Users and sessions (Phase 3, ADR 0013) ----

impl Persistence {
    pub fn count_users(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    pub fn create_user(
        &self,
        username: &str,
        password_hash: &str,
        role: crate::auth::Role,
    ) -> Result<crate::auth::User> {
        let conn = self.conn.lock().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        conn.execute(
            "INSERT INTO users (id, username, password_hash, role, active, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5)",
            params![id, username, password_hash, role.as_str(), now.to_rfc3339()],
        )?;
        Ok(crate::auth::User {
            id,
            username: username.to_string(),
            role,
            active: true,
            created_at: now,
        })
    }

    /// Look up a user with the stored hash. Returns `(user, password_hash)` so
    /// the hash never has to live on the `User` struct that gets serialized out.
    pub fn find_user_by_username(
        &self,
        username: &str,
    ) -> Result<Option<(crate::auth::User, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, username, password_hash, role, active, created_at
             FROM users WHERE username = ?1",
        )?;
        let mut rows = stmt.query_map(params![username], |row| {
            let role_str: String = row.get(3)?;
            let active: i64 = row.get(4)?;
            Ok((
                crate::auth::User {
                    id: row.get(0)?,
                    username: row.get(1)?,
                    role: crate::auth::Role::parse(&role_str).unwrap_or(crate::auth::Role::Viewer),
                    active: active != 0,
                    created_at: parse_ts(&row.get::<_, String>(5)?),
                },
                row.get::<_, String>(2)?,
            ))
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn get_user(&self, id: &str) -> Result<crate::auth::User> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, username, role, active, created_at FROM users WHERE id = ?1",
            params![id],
            |row| {
                let role_str: String = row.get(2)?;
                let active: i64 = row.get(3)?;
                Ok(crate::auth::User {
                    id: row.get(0)?,
                    username: row.get(1)?,
                    role: crate::auth::Role::parse(&role_str).unwrap_or(crate::auth::Role::Viewer),
                    active: active != 0,
                    created_at: parse_ts(&row.get::<_, String>(4)?),
                })
            },
        )
        .context(format!("No user with id {id}"))
    }

    pub fn list_users(&self) -> Result<Vec<crate::auth::User>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, username, role, active, created_at FROM users ORDER BY created_at",
        )?;
        let rows = stmt.query_map([], |row| {
            let role_str: String = row.get(2)?;
            let active: i64 = row.get(3)?;
            Ok(crate::auth::User {
                id: row.get(0)?,
                username: row.get(1)?,
                role: crate::auth::Role::parse(&role_str).unwrap_or(crate::auth::Role::Viewer),
                active: active != 0,
                created_at: parse_ts(&row.get::<_, String>(4)?),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn set_user_role(&self, id: &str, role: crate::auth::Role) -> Result<crate::auth::User> {
        {
            let conn = self.conn.lock().unwrap();
            let changed = conn.execute(
                "UPDATE users SET role = ?1, updated_at = ?2 WHERE id = ?3",
                params![role.as_str(), Utc::now().to_rfc3339(), id],
            )?;
            if changed == 0 {
                anyhow::bail!("No user with id {id}");
            }
        }
        self.get_user(id)
    }

    pub fn set_user_active(&self, id: &str, active: bool) -> Result<crate::auth::User> {
        {
            let conn = self.conn.lock().unwrap();
            let changed = conn.execute(
                "UPDATE users SET active = ?1, updated_at = ?2 WHERE id = ?3",
                params![if active { 1 } else { 0 }, Utc::now().to_rfc3339(), id],
            )?;
            if changed == 0 {
                anyhow::bail!("No user with id {id}");
            }
        }
        self.get_user(id)
    }

    pub fn set_user_password(&self, id: &str, password_hash: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute(
            "UPDATE users SET password_hash = ?1, updated_at = ?2 WHERE id = ?3",
            params![password_hash, Utc::now().to_rfc3339(), id],
        )?;
        if changed == 0 {
            anyhow::bail!("No user with id {id}");
        }
        Ok(())
    }

    pub fn create_session(
        &self,
        token: &str,
        user_id: &str,
        expires_at: chrono::DateTime<Utc>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO sessions (token, user_id, expires_at, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![
                token,
                user_id,
                expires_at.to_rfc3339(),
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn find_session(&self, token: &str) -> Result<Option<crate::auth::Session>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT s.token, s.user_id, s.expires_at, u.username, u.role, u.active
             FROM sessions s JOIN users u ON u.id = s.user_id
             WHERE s.token = ?1",
        )?;
        let mut rows = stmt.query_map(params![token], |row| {
            let role_str: String = row.get(4)?;
            let active: i64 = row.get(5)?;
            Ok((
                crate::auth::Session {
                    token: row.get(0)?,
                    user_id: row.get(1)?,
                    expires_at: parse_ts(&row.get::<_, String>(2)?),
                    username: row.get(3)?,
                    role: crate::auth::Role::parse(&role_str).unwrap_or(crate::auth::Role::Viewer),
                },
                active != 0,
            ))
        })?;
        // A deactivated user's session is treated as absent rather than valid.
        Ok(rows
            .next()
            .transpose()?
            .and_then(|(session, active)| active.then_some(session)))
    }

    pub fn delete_session(&self, token: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM sessions WHERE token = ?1", params![token])?;
        Ok(())
    }

    pub fn delete_sessions_for_user(&self, user_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM sessions WHERE user_id = ?1", params![user_id])?;
        Ok(())
    }

    /// Housekeeping for expired rows; safe to call at any time.
    pub fn purge_expired_sessions(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "DELETE FROM sessions WHERE expires_at <= ?1",
            params![Utc::now().to_rfc3339()],
        )?;
        Ok(n)
    }
}

/// Serialize a snake_case serde enum to the bare string stored in SQLite.
fn enum_str<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default()
}

fn parse_enum<T: serde::de::DeserializeOwned>(s: &str, fallback: T) -> T {
    serde_json::from_str::<T>(&format!("\"{}\"", s)).unwrap_or(fallback)
}

fn parse_ts(s: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn parse_mission_row(row: &rusqlite::Row) -> rusqlite::Result<Option<Mission>> {
    let id: String = row.get(0)?;
    let repo_channel_id: String = row.get(1)?;
    let title: String = row.get(2)?;
    let task_description: String = row.get(3)?;
    let session_mode_str: String = row.get(4)?;
    let autonomy_level_str: String = row.get(5)?;
    let status_str: String = row.get(6)?;
    let worktree_path: Option<String> = row.get(7)?;
    let branch_name: Option<String> = row.get(8)?;
    let base_branch: Option<String> = row.get(9)?;
    let created_at_str: String = row.get(10)?;
    let updated_at_str: String = row.get(11)?;

    let session_mode = serde_json::from_str::<SessionMode>(&format!("\"{}\"", session_mode_str))
        .unwrap_or(SessionMode::Worktree);
    let autonomy_level =
        serde_json::from_str::<AutonomyLevel>(&format!("\"{}\"", autonomy_level_str))
            .unwrap_or(AutonomyLevel::CoPilot);
    let status = serde_json::from_str::<MissionStatus>(&format!("\"{}\"", status_str))
        .unwrap_or(MissionStatus::Created);

    Ok(Some(Mission {
        id,
        repo_channel_id,
        title,
        task_description,
        session_mode,
        autonomy_level,
        status,
        worktree_path,
        branch_name,
        base_branch,
        created_at: created_at_str.parse().unwrap_or_else(|_| Utc::now()),
        updated_at: updated_at_str.parse().unwrap_or_else(|_| Utc::now()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // 050-Gold-Standard-Review.md F3 / Wave 1.3: migrations were a flat
    // `let _ = conn.execute(...)` that swallowed every error, real or benign,
    // and tracked no version. These reproduce the specific failure that fix
    // targets: a real error must abort startup, not be indistinguishable from
    // "column already exists".

    #[test]
    fn a_fresh_database_is_stamped_at_the_latest_migration_version() {
        let p = Persistence::new_in_memory().unwrap();
        let conn = p.conn.lock().unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
    }

    #[test]
    fn an_old_pre_migration_database_is_upgraded_and_stamped() {
        // Mirrors the schema shape a database from before this migration
        // system existed would have: the base tables, but without the
        // columns MIGRATIONS adds, and no user_version stamp (defaults to 0).
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE settings (id INTEGER PRIMARY KEY CHECK (id = 1), anthropic_api_key TEXT);
            CREATE TABLE repo_channels (id TEXT PRIMARY KEY, path TEXT NOT NULL UNIQUE);
            "#,
        )
        .unwrap();

        run_migrations(&mut conn).unwrap();

        let has_column: bool = conn
            .prepare(
                "SELECT COUNT(*) FROM pragma_table_info('settings') WHERE name = 'openai_api_key'",
            )
            .unwrap()
            .query_row([], |row| row.get::<_, i64>(0))
            .unwrap()
            > 0;
        assert!(
            has_column,
            "expected openai_api_key to exist after migrating an old DB"
        );

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
    }

    #[test]
    fn a_database_from_a_newer_binary_is_refused_rather_than_silently_run() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", MIGRATIONS.len() as i64 + 1)
            .unwrap();

        let err = run_migrations(&mut conn).unwrap_err();
        assert!(err.to_string().contains("newer"), "{err}");
    }

    #[test]
    fn a_genuine_migration_failure_aborts_instead_of_being_swallowed() {
        // No tables at all: every `ALTER TABLE settings ...` fails with
        // "no such table", which is not the tolerated duplicate-column case,
        // so this must return Err rather than silently continuing the way the
        // old blanket `let _ =` did.
        let mut conn = Connection::open_in_memory().unwrap();

        let result = run_migrations(&mut conn);
        assert!(result.is_err(), "expected a real schema error to propagate");

        // The failed transaction must not have partially applied — version
        // stays at its pre-migration value.
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 0);
    }

    #[test]
    fn file_backed_databases_use_wal_journal_mode() {
        // WAL survives an abruptly-killed writer far better than the default
        // rollback journal — found via a real reproduction where a repeatedly
        // force-killed dev instance reached a state where a synchronously
        // seeded row was inconsistently visible across connections.
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let _p = Persistence::new(Some(db_path.clone())).unwrap();

        let check_conn = Connection::open(&db_path).unwrap();
        let mode: String = check_conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
    }

    #[test]
    fn test_persistence_in_memory() {
        let p = Persistence::new_in_memory().unwrap();
        let ws = p.list_workspaces().unwrap();
        assert!(!ws.is_empty());
        let repo = p.connect_repo("/tmp/test-repo", None).unwrap();
        assert_eq!(repo.path, "/tmp/test-repo");
        let list = p.list_repo_channels().unwrap();
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn test_mission_crud() {
        let p = Persistence::new_in_memory().unwrap();
        let repo = p.connect_repo("/tmp/test-repo-2", None).unwrap();
        let mission = p
            .create_mission(
                &repo.id,
                "Test Mission",
                "Do something",
                SessionMode::Worktree,
                AutonomyLevel::CoPilot,
            )
            .unwrap();
        assert_eq!(mission.title, "Test Mission");
        let missions = p.list_missions(Some(&repo.id)).unwrap();
        assert_eq!(missions.len(), 1);
    }

    #[test]
    fn reconnecting_an_already_connected_repo_keeps_existing_missions_intact() {
        // Regression for a real bug found via live E2E testing: connect_repo
        // used to always mint a fresh id and `INSERT OR REPLACE`, which — since
        // `path` is UNIQUE — deleted the old row on a second connect of the
        // same path and broke the `missions.repo_channel_id` foreign key for
        // every Mission already created against it.
        let p = Persistence::new_in_memory().unwrap();
        let first = p.connect_repo("/tmp/reconnect-repo", None).unwrap();
        let mission = p
            .create_mission(
                &first.id,
                "Pre-existing Mission",
                "Do something",
                SessionMode::Worktree,
                AutonomyLevel::CoPilot,
            )
            .unwrap();

        let second = p.connect_repo("/tmp/reconnect-repo", None).unwrap();
        assert_eq!(
            first.id, second.id,
            "reconnecting the same path must keep the same repo_channel id"
        );

        let missions = p.list_missions(Some(&second.id)).unwrap();
        assert_eq!(
            missions.len(),
            1,
            "the Mission created before the reconnect must still be reachable"
        );
        assert_eq!(missions[0].id, mission.id);

        let all_repos = p.list_repo_channels().unwrap();
        assert_eq!(
            all_repos.len(),
            1,
            "reconnecting must not leave a duplicate repo_channels row"
        );
    }
}
