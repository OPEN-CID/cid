use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// JSON-RPC 2.0 envelope

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(default = "default_version")]
    pub version: Option<String>,
    pub id: Option<serde_json::Value>,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: serde_json::Value,
}

fn default_version() -> Option<String> {
    Some("2.0".to_string())
}

impl JsonRpcResponse {
    pub fn success(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }
    pub fn error(id: Option<serde_json::Value>, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message,
                data: None,
            }),
        }
    }
}

// ================= Domain Types =================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    Worktree,
    Shared,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyLevel {
    Manual,
    CoPilot,
    Autonomous, // reserved for Phase 1+
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MissionStatus {
    Created,
    Planning,
    Running,
    BlockedOnApproval,
    Review,
    Done,
    Failed,
    Closed,
}

// ============ Planner / Reviewer (Part 5) ============

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MissionPlanStatus {
    Draft,
    Approved,
    Rejected,
}

/// The editable plan document a Mission's Planner produces, and the human
/// approves, before the Implementer is allowed to run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionPlan {
    pub id: String,
    pub mission_id: String,
    pub content: String,
    pub status: MissionPlanStatus,
    pub approved_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewSeverity {
    Critical,
    Warning,
    Info,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewVerdict {
    Clean,
    CommentsOnly,
    ChangesRequested,
    NotRun,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewFinding {
    pub severity: ReviewSeverity,
    pub file: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionReview {
    pub id: String,
    pub mission_id: String,
    pub verdict: ReviewVerdict,
    pub findings: Vec<ReviewFinding>,
    pub raw_output: String,
    pub created_at: DateTime<Utc>,
}

/// A snapshot of a Mission's worktree at a point in time (review_prompt.md
/// §3.2) — built on the worktree every Mission already has, not a parallel
/// snapshot store. `sha` is the worktree's HEAD commit at checkpoint time,
/// after committing any then-uncommitted changes first, so a rewind is
/// always a clean `git reset --hard` back to a fully-captured state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionCheckpoint {
    pub id: String,
    pub mission_id: String,
    pub sha: String,
    pub label: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoChannel {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub path: String,
    pub remote_url: Option<String>,
    pub agents_md_content: Option<String>,
    pub created_at: DateTime<Utc>,
    /// review_prompt.md §1.2: whether a human has reviewed this repo's
    /// AGENTS.md. Until true, it is detected and shown to the user but never
    /// loaded into the model's system prompt.
    #[serde(default)]
    pub agents_md_approved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mission {
    pub id: String,
    pub repo_channel_id: String,
    pub title: String,
    pub task_description: String,
    pub session_mode: SessionMode,
    pub autonomy_level: AutonomyLevel,
    pub status: MissionStatus,
    pub worktree_path: Option<String>,
    pub branch_name: Option<String>,
    pub base_branch: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    pub status: ToolCallStatus,
    pub result: Option<serde_json::Value>,
    pub requires_approval: bool,
    pub approved: Option<bool>,
    /// review_prompt.md §1.2 point 3: set when this call's arguments were
    /// built in a turn where untrusted repo content (an approved AGENTS.md,
    /// or a prior file/diff/MCP read this Mission) was present in context —
    /// a provenance marker for the History panel, not a guarantee the
    /// content actually influenced this specific call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    PendingApproval,
    Approved,
    Denied,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub mission_id: String,
    pub role: MessageRole,
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub is_streaming: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitStatusFile {
    pub path: String,
    pub status: String,
    pub staged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitDiffHunk {
    pub id: String,
    pub file_path: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub header: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitDiffFile {
    pub path: String,
    pub old_path: Option<String>,
    pub status: String,
    pub hunks: Vec<GitDiffHunk>,
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyInstance {
    pub id: String,
    pub mission_id: String,
    pub cols: u16,
    pub rows: u16,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServer {
    pub id: String,
    pub name: String,
    pub transport_type: McpTransportType,
    pub transport_config: serde_json::Value,
    pub status: McpServerStatus,
    pub enabled_for_repos: Vec<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum McpTransportType {
    Stdio,
    Http,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum McpServerStatus {
    Connected,
    Disconnected,
    Error,
}

// The real MCP `tools/list` wire format uses camelCase (`inputSchema`) per
// the spec — found via review_prompt.md §2.1's stdio test, which is the
// first test in this codebase to deserialize a genuinely spec-shaped tool
// object rather than a hand-built fixture already using the wrong casing.
// Without this, `serde_json::from_value::<Vec<McpTool>>` fails silently on
// every real MCP server's response (missing required field `input_schema`),
// and the caller's `.ok()` swallows it into an empty tool list — a likely-
// live bug in the HTTP transport too, not just the stdio one being fixed
// alongside it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpTool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub content: String,
    pub scope: SkillScope,
    pub scope_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SkillScope {
    Workspace,
    Repo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub anthropic_api_key: Option<String>,
    pub anthropic_model: String,
    pub openai_api_key: Option<String>,
    pub openai_model: Option<String>,
    pub google_api_key: Option<String>,
    pub google_model: Option<String>,
    pub openai_compatible_endpoint: Option<String>,
    pub openai_compatible_api_key: Option<String>,
    pub openai_compatible_model: Option<String>,
    pub worktree_root: Option<String>,
    pub theme: String,
    // Phase 1: per-role model configs (stored as JSON)
    pub planner_provider: Option<String>,
    pub planner_model: Option<String>,
    pub implementer_provider: Option<String>,
    pub implementer_model: Option<String>,
    pub reviewer_provider: Option<String>,
    pub reviewer_model: Option<String>,
    pub github_token: Option<String>,
}

// Phase 1 Model Provider Types

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ModelProvider {
    Anthropic,
    OpenAI,
    Google,
    OpenAICompatible,
    Ollama,
    LmStudio,
    LlamaCpp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub provider: ModelProvider,
    pub context_length: Option<usize>,
    pub default: bool,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    Planner,
    Implementer,
    Reviewer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleModelConfig {
    pub role: AgentRole,
    pub provider: ModelProvider,
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProviderConfig {
    pub provider: ModelProvider,
    pub enabled: bool,
    pub api_key: Option<String>,
    pub endpoint: Option<String>,
    pub models: Vec<ModelInfo>,
}

// ============ Local Runtime Detection ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalRuntime {
    pub runtime_type: ModelProvider,
    pub name: String,
    pub endpoint: String,
    pub available: bool,
    pub models: Vec<ModelInfo>,
    pub version: Option<String>,
}

// ============ SKILL.md Full Support ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillBundle {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub scope: SkillScope,
    pub scope_id: Option<String>,
    pub path: String,
    pub skill_md_content: String,
    pub additional_files: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ============ ACP Host ============

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AcpEditorType {
    Zed,
    JetBrains,
    VsCode,
    Cursor,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpEditor {
    pub id: String,
    pub name: String,
    pub editor_type: AcpEditorType,
    pub executable_path: String,
    pub available: bool,
    pub version: Option<String>,
    pub supports_acp: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AcpHandoffStatus {
    Idle,
    HandedOff,
    InExternalEditor,
    Returned,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpHandoff {
    pub id: String,
    pub mission_id: String,
    pub editor_id: String,
    pub status: AcpHandoffStatus,
    pub worktree_path: String,
    pub created_at: DateTime<Utc>,
    pub returned_at: Option<DateTime<Utc>>,
}

// ============ Headless Mode ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadlessConfig {
    pub host: String,
    pub port: u16,
    pub allow_remote: bool,
    pub auth_token: Option<String>,
    pub enable_cors: bool,
}

// ============ Context Engine (Tree-sitter) ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEngineStatus {
    pub enabled: bool,
    pub repo_path: String,
    pub indexed_files: usize,
    pub total_files: usize,
    pub last_indexed_at: Option<DateTime<Utc>>,
    pub indexing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Class,
    Interface,
    Variable,
    Constant,
    Enum,
    Struct,
    Method,
    Property,
    Import,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeSymbol {
    pub id: String,
    pub name: String,
    pub kind: SymbolKind,
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub parent: Option<String>,
    pub imports: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileIndex {
    pub path: String,
    pub language: String,
    pub symbols: Vec<CodeSymbol>,
    pub imports: Vec<String>,
    pub last_modified: DateTime<Utc>,
    pub size: usize,
}

// ============ GitHub Bridge ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubConfig {
    pub repo_path: String,
    pub owner: String,
    pub repo: String,
    pub connected: bool,
    pub has_token: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubIssue {
    pub id: u64,
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub author: String,
    pub labels: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubPr {
    pub id: u64,
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub head_branch: String,
    pub base_branch: String,
    pub author: String,
    pub url: String,
    pub created_at: DateTime<Utc>,
}

// ============ Autonomous Mode ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomyAllowlist {
    pub id: String,
    pub scope: String,
    pub scope_id: String,
    pub allowed_commands: Vec<AllowedCommand>,
    pub allowed_paths: Vec<String>,
    pub denied_paths: Vec<String>,
    pub max_tool_calls: Option<usize>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllowedCommand {
    pub pattern: String,
    pub description: Option<String>,
    pub requires_approval: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomyCheckResult {
    pub allowed: bool,
    pub reason: String,
    pub requires_approval: bool,
    pub matched_pattern: Option<String>,
}

// =========== Request Params ===========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectRepoParams {
    pub path: String,
    pub workspace_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMissionParams {
    pub repo_channel_id: String,
    pub title: String,
    pub task: String,
    pub session_mode: Option<SessionMode>,
    pub autonomy_level: Option<AutonomyLevel>,
    /// Vibe-coding preset (Phase 5): a lightweight Mission Thread
    /// configuration for quick, low-stakes changes — a minimal plan is
    /// generated and auto-approved so the Mission starts executing
    /// immediately. Tool-call approval (Co-Pilot), diffs, and History are
    /// unaffected: this shortens the Planner ceremony, not code review.
    #[serde(default)]
    pub vibe: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageParams {
    pub mission_id: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitStatusParams {
    pub repo_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitDiffParams {
    pub repo_path: String,
    pub base: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommitParams {
    pub repo_path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeCreateParams {
    pub repo_path: String,
    pub branch: String,
    pub worktree_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyCreateParams {
    pub mission_id: String,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyWriteParams {
    pub pty_id: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyResizeParams {
    pub pty_id: String,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpAddServerParams {
    pub name: String,
    pub transport_type: McpTransportType,
    pub config: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpCallToolParams {
    pub server_id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReadParams {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileWriteParams {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolApprovalParams {
    pub mission_id: String,
    pub tool_call_id: String,
    pub approved: bool,
}

// ============ Phase 1 Request Params ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelListParams {
    pub provider: Option<ModelProvider>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleModelUpdateParams {
    pub role: AgentRole,
    pub provider: ModelProvider,
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalRuntimeDetectParams {
    pub force_refresh: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillBundleListParams {
    pub scope: Option<String>,
    pub repo_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpHandoffParams {
    pub mission_id: String,
    pub editor_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEngineToggleParams {
    pub repo_path: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEngineSearchParams {
    pub repo_path: String,
    pub query: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubConnectParams {
    pub repo_path: String,
    pub owner: String,
    pub repo: String,
    pub token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubIssueToMissionParams {
    pub repo_path: String,
    pub issue_number: u64,
    pub session_mode: Option<SessionMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomyAllowlistUpdateParams {
    pub scope_id: String,
    pub allowed_commands: Vec<AllowedCommand>,
    pub allowed_paths: Vec<String>,
    pub denied_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomyCheckParams {
    pub repo_path: String,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubPrCreateParams {
    pub repo_path: String,
    pub title: String,
    pub body: Option<String>,
    pub head_branch: String,
    pub base_branch: Option<String>,
}

// ============ Phase 2: Background Model Router ============

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundTaskType {
    Summarize,
    CommitMessage,
    LintSuggestion,
    PlanExecution,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundModelConfig {
    pub repo_channel_id: String,
    pub enabled: bool,
    pub preferred_runtime: Option<ModelProvider>,
    pub preferred_model: Option<String>,
    pub enabled_tasks: Vec<BackgroundTaskType>,
    pub max_concurrent_tasks: usize,
}

impl Default for BackgroundModelConfig {
    fn default() -> Self {
        Self {
            repo_channel_id: String::new(),
            enabled: false,
            preferred_runtime: None,
            preferred_model: None,
            enabled_tasks: vec![
                BackgroundTaskType::Summarize,
                BackgroundTaskType::CommitMessage,
            ],
            max_concurrent_tasks: 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundTaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundTask {
    pub id: String,
    pub task_type: BackgroundTaskType,
    pub repo_channel_id: String,
    pub mission_id: Option<String>,
    pub input: serde_json::Value,
    pub status: BackgroundTaskStatus,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    pub runtime: Option<ModelProvider>,
    pub model: Option<String>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundTaskSubmitParams {
    pub task_type: BackgroundTaskType,
    pub repo_channel_id: String,
    pub mission_id: Option<String>,
    pub input: serde_json::Value,
}

// ============ Phase 2: Subagent Orchestrator ============

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SubagentRole {
    ResearchWorker,
    ParallelImpl,
    CodeExplorer,
    TestRunner,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SubagentStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subagent {
    pub id: String,
    pub mission_id: String,
    pub role: SubagentRole,
    pub prompt: String,
    pub status: SubagentStatus,
    pub parent_agent_type: AgentRole,
    pub model_provider: Option<ModelProvider>,
    pub model_id: Option<String>,
    pub tool_permissions: Vec<String>,
    pub result: Option<SubagentResult>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentResult {
    pub summary: String,
    pub findings: serde_json::Value,
    pub files_changed: Vec<String>,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentSpawnParams {
    pub mission_id: String,
    pub role: SubagentRole,
    pub prompt: String,
    pub tool_permissions: Option<Vec<String>>,
    pub model_provider: Option<ModelProvider>,
    pub model_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentCancelParams {
    pub subagent_id: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentListParams {
    pub mission_id: String,
}

// ============ Phase 2: Slack Bridge ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackConfig {
    pub workspace_id: Option<String>,
    pub webhook_url: String,
    pub signing_secret: Option<String>,
    pub bot_token: Option<String>,
    pub enabled: bool,
    pub allowed_channels: Vec<String>,
    pub default_channel: Option<String>,
    pub trigger_prefix: Option<String>,
}

impl Default for SlackConfig {
    fn default() -> Self {
        Self {
            workspace_id: None,
            webhook_url: String::new(),
            signing_secret: None,
            bot_token: None,
            enabled: false,
            allowed_channels: vec![],
            default_channel: None,
            trigger_prefix: Some("/cid".to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackMessage {
    pub id: String,
    pub channel_id: String,
    pub channel_name: String,
    pub user_id: String,
    pub user_name: String,
    pub text: String,
    pub timestamp: String,
    pub thread_ts: Option<String>,
    pub reactions: Vec<SlackReaction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackReaction {
    pub name: String,
    pub count: u32,
    pub users: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackTrigger {
    pub id: String,
    pub message: SlackMessage,
    pub triggered_at: DateTime<Utc>,
    pub parsed_command: Option<String>,
    pub parsed_args: Option<String>,
    pub mission_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackTriggerParams {
    pub message: SlackMessage,
    pub workspace_id: Option<String>,
}

// ============ Phase 2: Teams Bridge ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamsConfig {
    pub workspace_id: Option<String>,
    pub webhook_url: String,
    pub enabled: bool,
    pub allowed_teams: Vec<String>,
    pub allowed_channels: Vec<String>,
    pub trigger_keywords: Vec<String>,
}

impl Default for TeamsConfig {
    fn default() -> Self {
        Self {
            workspace_id: None,
            webhook_url: String::new(),
            enabled: false,
            allowed_teams: vec![],
            allowed_channels: vec![],
            trigger_keywords: vec!["@cid".to_string()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamsMessage {
    pub id: String,
    pub team_id: String,
    pub team_name: String,
    pub channel_id: String,
    pub channel_name: String,
    pub user_id: String,
    pub user_name: String,
    pub text: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamsTriggerParams {
    pub message: TeamsMessage,
    pub workspace_id: Option<String>,
}

// ============ Phase 2: MCP Tasks ============

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum McpTaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTaskHandle {
    pub id: String,
    pub server_id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub status: McpTaskStatus,
    pub progress: Option<f64>,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTaskCreateParams {
    pub server_id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTaskPollParams {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTaskSubscribeParams {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTaskCancelParams {
    pub task_id: String,
}

// ============ Phase 2: Semantic Context Engine ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticIndexStatus {
    pub repo_path: String,
    pub enabled: bool,
    pub indexed_files: usize,
    pub total_file_chunks: usize,
    pub dependency_nodes: usize,
    pub dependency_edges: usize,
    pub last_indexed_at: Option<DateTime<Utc>>,
    pub indexing: bool,
    /// True once the real embedding model (`semantic_engine::embeddings`)
    /// has downloaded and loaded — false means embeddings are still the
    /// hash-based fallback, not a lesser version of the same real thing.
    #[serde(default)]
    pub embedding_model_ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyNode {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub line: usize,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyEdge {
    pub from: String,
    pub to: String,
    pub relation: String,
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitBlameInfo {
    pub file_path: String,
    pub line: usize,
    pub author: String,
    pub email: String,
    pub commit_hash: String,
    pub commit_date: DateTime<Utc>,
    pub commit_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticSearchResult {
    pub file_path: String,
    pub content: String,
    pub score: f64,
    pub line: Option<usize>,
    pub symbol_name: Option<String>,
    pub dependencies: Vec<String>,
    pub blame: Option<GitBlameInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticSearchParams {
    pub repo_path: String,
    pub query: String,
    pub limit: Option<usize>,
    pub include_dependencies: Option<bool>,
    pub include_blame: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticDependencyParams {
    pub repo_path: String,
    pub file_path: Option<String>,
    pub symbol_name: Option<String>,
    pub depth: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticGitBlameParams {
    pub repo_path: String,
    pub file_path: String,
    pub line: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticEnableParams {
    pub repo_path: String,
}

// ============ Phase 2: Sandboxing ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    pub worktree_path: String,
    pub allowed_read_paths: Vec<String>,
    pub allowed_write_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxTestResult {
    pub passed: bool,
    pub reason: String,
    pub attempted_path: String,
    pub blocked: bool,
}

// Helpers

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

pub fn now_utc() -> DateTime<Utc> {
    Utc::now()
}
