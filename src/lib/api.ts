/**
 * CID JSON-RPC 2.0 Client
 * Supports both WebSocket and HTTP fallback, plus Tauri IPC adapter.
 * Phase 0: connects to cid-core on ws://127.0.0.1:5919/ws
 */

// The JSON-RPC surface spans ~170 methods, each with its own response shape,
// none of it modeled in TypeScript (051-Editor-Excellence-Roadmap.md Wave
// 2.1 / 5.1 — modeling every RPC's real return type is future work, not a
// lint-driven side effect of this fix). `RpcValue` centralizes that one
// honest `any` in a single, named, justified place rather than letting the
// keyword recur at every call site — the "invisible global downgrade" F4
// warns against is `--max-warnings` silently rising, not a single documented
// alias.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type RpcValue = any;

export type JsonRpcRequest = {
  jsonrpc: "2.0";
  id: string;
  method: string;
  params: unknown;
};

export type JsonRpcResponse = {
  jsonrpc: "2.0";
  id: string;
  result?: RpcValue;
  error?: { code: number; message: string; data?: RpcValue };
};

export type DirEntry = { name: string; path: string; is_git_repo: boolean };

export type ListDirsResult = { path: string; parent: string | null; entries: DirEntry[] };

/// Mirrors `cid_core::api::types::ModelInfo` on the wire. The field is
/// `available` (a key/endpoint is configured for that provider), not
/// `enabled` — an earlier version of this type invented the latter, which
/// TypeScript could not catch because `call()` returns an asserted type
/// rather than a validated one, so every model silently read as unavailable
/// and every picker rendered every option disabled.
export type ModelInfo = {
  id: string;
  name: string;
  provider: string;
  context_length: number;
  default: boolean;
  available: boolean;
};

export type JsonRpcNotification = {
  jsonrpc: "2.0";
  method: string;
  // Incoming — existing consumers (ChatThread, TerminalPane, MobileApp) already
  // narrow by property access per notification method, the same "genuinely
  // varies, not modeled" shape as RpcValue above.
  params: RpcValue;
};

type NotificationHandler = (notif: JsonRpcNotification) => void;

class CidApiClient {
  private ws: WebSocket | null = null;
  private httpBase: string;
  private wsUrl: string;
  private requestId = 0;
  private pending = new Map<string, { resolve: (v: RpcValue) => void; reject: (e: unknown) => void }>();
  private notificationHandlers = new Set<NotificationHandler>();
  private reconnectAttempts = 0;
  private isTauri = false;

  constructor() {
    const port = import.meta.env.VITE_CID_CORE_PORT || "5919";
    const host = import.meta.env.VITE_CID_CORE_HOST || "127.0.0.1";
    this.wsUrl = `ws://${host}:${port}/ws`;
    this.httpBase = `http://${host}:${port}`;
    this.isTauri = !!(window as unknown as { __TAURI__?: unknown }).__TAURI__;
  }

  async connect(): Promise<void> {
    if (this.isTauri) {
      // The desktop shell starts Core in-process (src-tauri/src/lib.rs) and
      // talks to it over the same WebSocket the browser client uses, rather
      // than through Tauri `invoke` — one transport, one code path to test.
      console.log("[CID] Running inside Tauri, using WS to core");
    }

    return new Promise((resolve, reject) => {
      try {
        this.ws = new WebSocket(this.wsUrl);

        this.ws.onopen = () => {
          console.log(`[CID] Connected to core at ${this.wsUrl}`);
          this.reconnectAttempts = 0;
          resolve();
        };

        this.ws.onmessage = (event) => {
          try {
            const msg = JSON.parse(event.data);
            if (msg.id !== undefined && msg.id !== null) {
              // Response
              const pending = this.pending.get(String(msg.id));
              if (pending) {
                if (msg.error) {
                  pending.reject(new Error(msg.error.message));
                } else {
                  pending.resolve(msg.result);
                }
                this.pending.delete(String(msg.id));
              } else {
                // Could be a broadcasted response (our simple server broadcasts responses)
                // Try to handle as notification if no pending
                if (msg.result !== undefined || msg.error) {
                  // ignore or treat as notification
                }
              }
            } else if (msg.method) {
              // Notification
              const notif = msg as JsonRpcNotification;
              for (const handler of this.notificationHandlers) {
                try {
                  handler(notif);
                } catch (e) {
                  console.error("[CID] Notification handler error", e);
                }
              }
            }
          } catch (e) {
            console.error("[CID] Failed to parse WS message", e, event.data);
          }
        };

        this.ws.onerror = (e) => {
          console.error("[CID] WS error", e);
          // Don't reject if already connected, will retry
          if (this.reconnectAttempts === 0) {
            reject(e);
          }
        };

        this.ws.onclose = () => {
          console.log("[CID] WS closed, will reconnect");
          this.ws = null;
          // Try reconnect with backoff
          if (this.reconnectAttempts < 10) {
            setTimeout(() => {
              this.reconnectAttempts++;
              this.connect().catch(() => {});
            }, Math.min(1000 * Math.pow(2, this.reconnectAttempts), 10000));
          }
        };
      } catch (e) {
        reject(e);
      }
    });
  }

  onNotification(handler: NotificationHandler): () => void {
    this.notificationHandlers.add(handler);
    return () => this.notificationHandlers.delete(handler);
  }

  async call(method: string, params: unknown = {}): Promise<RpcValue> {
    // Try WS first if connected, fallback to HTTP
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      return this.callViaWs(method, params);
    } else {
      return this.callViaHttp(method, params);
    }
  }

  private callViaWs(method: string, params: unknown): Promise<RpcValue> {
    return new Promise((resolve, reject) => {
      const id = String(++this.requestId);
      const req: JsonRpcRequest = {
        jsonrpc: "2.0",
        id,
        method,
        params,
      };
      this.pending.set(id, { resolve, reject });
      try {
        this.ws!.send(JSON.stringify(req));
      } catch (e) {
        this.pending.delete(id);
        reject(e);
      }
      // Timeout
      setTimeout(() => {
        if (this.pending.has(id)) {
          this.pending.delete(id);
          reject(new Error(`Timeout calling ${method}`));
        }
      }, 15000);
    });
  }

  private async callViaHttp(method: string, params: unknown): Promise<RpcValue> {
    const id = String(++this.requestId);
    const req = {
      jsonrpc: "2.0",
      id,
      method,
      params,
    };
    const resp = await fetch(`${this.httpBase}/api/rpc`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(req),
    });
    if (!resp.ok) {
      throw new Error(`HTTP RPC failed: ${resp.status}`);
    }
    const json = (await resp.json()) as JsonRpcResponse;
    if (json.error) {
      throw new Error(json.error.message);
    }
    return json.result;
  }

  // Convenience wrappers
  workspace = {
    list: () => this.call("workspace.list"),
  };

  repo = {
    list: () => this.call("repo.list"),
    connect: (path: string) => this.call("repo.connect", { path }),
    get: (id: string) => this.call("repo.get", { id }),
    agentsMd: (path: string) => this.call("repo.agents_md", { path }),
    agentsMdApprove: (id: string) => this.call("repo.agents_md.approve", { id }),
    disconnect: (id: string) => this.call("repo.disconnect", { id }),
  };

  mission = {
    list: (repoChannelId?: string) => this.call("mission.list", { repo_channel_id: repoChannelId }),
    create: (params: {
      repo_channel_id: string;
      title: string;
      task?: string;
      session_mode?: string;
      autonomy_level?: string;
      vibe?: boolean;
      model_provider?: string | null;
      model_id?: string | null;
    }) => this.call("mission.create", params),
    get: (id: string) => this.call("mission.get", { id }),
    close: (id: string) => this.call("mission.close", { id }),
    sendMessage: (mission_id: string, content: string) => this.call("mission.send_message", { mission_id, content }),
    approveTool: (mission_id: string, tool_call_id: string, approved: boolean) =>
      this.call("mission.approve_tool", { mission_id, tool_call_id, approved }),
    contextUsage: (mission_id: string) => this.call("mission.context.usage", { mission_id }),
    contextCompact: (mission_id: string) => this.call("mission.context.compact", { mission_id }),
    checkpointList: (mission_id: string) => this.call("mission.checkpoint.list", { mission_id }),
    checkpointRewind: (mission_id: string, checkpoint_id: string, confirm: boolean) =>
      this.call("mission.checkpoint.rewind", { mission_id, checkpoint_id, confirm }),
  };

  message = {
    list: (missionId: string) => this.call("message.list", { mission_id: missionId }),
  };

  git = {
    status: (repoPath: string) => this.call("git.status", { repo_path: repoPath }),
    diff: (repoPath: string, base?: string) => this.call("git.diff", { repo_path: repoPath, base }),
    commit: (repoPath: string, message: string) => this.call("git.commit", { repo_path: repoPath, message }),
    log: (repoPath: string) => this.call("git.log", { repo_path: repoPath }),
    worktree: {
      list: (repoPath: string) => this.call("git.worktree.list", { repo_path: repoPath }),
      create: (repoPath: string, branch: string, worktreePath: string) =>
        this.call("git.worktree.create", { repo_path: repoPath, branch, worktree_path: worktreePath }),
      remove: (repoPath: string, worktreePath: string) =>
        this.call("git.worktree.remove", { repo_path: repoPath, worktree_path: worktreePath }),
    },
  };

  pty = {
    create: (mission_id: string, cols?: number, rows?: number) => this.call("pty.create", { mission_id, cols, rows }),
    write: (pty_id: string, data: string) => this.call("pty.write", { pty_id, data }),
    resize: (pty_id: string, cols: number, rows: number) => this.call("pty.resize", { pty_id, cols, rows }),
    kill: (pty_id: string) => this.call("pty.kill", { pty_id }),
    list: (mission_id: string) => this.call("pty.list", { mission_id }),
  };

  mcp = {
    list: () => this.call("mcp.server.list"),
    add: (name: string, transport_type: string, config: unknown) => this.call("mcp.server.add", { name, transport_type, config }),
    remove: (id: string) => this.call("mcp.server.remove", { id }),
    tools: (serverId: string) => this.call("mcp.tools.list", { server_id: serverId }),
    callTool: (serverId: string, toolName: string, args: unknown) => this.call("mcp.tool.call", { server_id: serverId, tool_name: toolName, arguments: args }),
  };

  confidence = {
    score: (mission_id: string, target_file: string, new_content?: string) =>
      this.call("confidence.score", { mission_id, target_file, new_content }),
    history: (mission_id: string) => this.call("confidence.history", { mission_id }),
  };

  missionReview = {
    run: (mission_id: string) => this.call("mission.review.run", { mission_id }),
    get: (mission_id: string) => this.call("mission.review.get", { mission_id }),
    list: (mission_id: string) => this.call("mission.review.list", { mission_id }),
  };

  file = {
    read: (path: string) => this.call("file.read", { path }),
    write: (path: string, content: string) => this.call("file.write", { path, content }),
    list: (path: string) => this.call("file.list", { path }),
  };

  fs = {
    listDirs: (path?: string | null): Promise<ListDirsResult> => this.call("fs.list_dirs", { path: path ?? null }),
  };

  code = {
    analyzeFile: (file_path: string) => this.call("code.analyze_file", { file_path }),
    analyzeDirectory: (dir_path: string) => this.call("code.analyze_directory", { dir_path }),
    searchSymbols: (dir_path: string, query: string) => this.call("code.search_symbols", { dir_path, query }),
    getImports: (file_path: string) => this.call("code.get_imports", { file_path }),
  };

  skills = {
    list: (scope?: string) => this.call("skills.list", { scope }),
    save: (skill: unknown) => this.call("skills.save", skill),
  };

  repoHealth = {
    scan: (path: string) => this.call("repo_health.scan", { path }),
  };

  observability = {
    crashesList: () => this.call("observability.crashes.list"),
  };

  settings = {
    get: () => this.call("settings.get"),
    update: (settings: unknown) => this.call("settings.update", settings),
  };

  model = {
    list: (): Promise<ModelInfo[]> => this.call("model.list"),
    chat: (params: unknown) => this.call("model.chat", params),
  };

  // ---- Phase 1 ----

  localRuntime = {
    list: () => this.call("local.runtime.list"),
    detect: (forceRefresh = false) => this.call("local.runtime.detect", { force_refresh: forceRefresh }),
  };

  acp = {
    editors: () => this.call("acp.editors.list"),
    handoff: (mission_id: string, editor_id: string) => this.call("acp.handoff", { mission_id, editor_id }),
    takeBack: (handoff_id: string) => this.call("acp.take_back", { handoff_id }),
    handoffs: (mission_id?: string) => this.call("acp.handoffs.list", { mission_id }),
    get: (handoff_id: string) => this.call("acp.handoff.get", { handoff_id }),
    remove: (handoff_id: string) => this.call("acp.handoff.remove", { handoff_id }),
  };

  skillBundles = {
    list: (repo_path: string, scope: "workspace" | "repo" = "repo") =>
      this.call("skills.bundles.list", { repo_path, scope }),
    write: (path: string, content: string) => this.call("skills.bundle.write", { path, content }),
    resolve: (repo_path: string, mission_context?: string) =>
      this.call("skills.resolve", { repo_path, mission_context }),
  };

  github = {
    connect: (params: unknown) => this.call("github.connect", params),
    config: (repo_path: string) => this.call("github.config.get", { repo_path }),
    issues: (repo_path: string) => this.call("github.issues.list", { repo_path }),
    issue: (repo_path: string, number: number) => this.call("github.issue.get", { repo_path, number }),
    issueToMission: (params: unknown) => this.call("github.issue.to_mission", params),
    prs: (repo_path: string) => this.call("github.pr.list", { repo_path }),
    createPr: (params: unknown) => this.call("github.pr.create", params),
    prStatus: (repo_path: string, number: number) => this.call("github.pr.status", { repo_path, number }),
  };

  autonomy = {
    allowlistGet: (scope_id: string) => this.call("autonomy.allowlist.get", { scope_id }),
    allowlistSet: (params: unknown) => this.call("autonomy.allowlist.set", params),
    allowlistRemove: (scope_id: string) => this.call("autonomy.allowlist.remove", { scope_id }),
    allowlistList: () => this.call("autonomy.allowlist.list"),
    allowlistDefault: (scope_id: string) => this.call("autonomy.allowlist.default", { scope_id }),
    checkCommand: (repo_path: string, command: string) =>
      this.call("autonomy.command.check", { repo_path, command }),
    checkBudget: (scope_id: string) => this.call("autonomy.budget.check", { scope_id }),
  };

  roleProfile = {
    listForRepo: (workspace_id: string, repo_channel_id: string) =>
      this.call("role_profile.list", { workspace_id, repo_channel_id }),
    get: (id: string) => this.call("role_profile.get", { id }),
    create: (input: unknown) => this.call("role_profile.create", input),
    update: (id: string, profile: unknown) => this.call("role_profile.update", { id, profile }),
    delete: (id: string) => this.call("role_profile.delete", { id }),
    checkPermission: (profile_id: string, tool_name: string) =>
      this.call("role_profile.check_permission", { profile_id, tool_name }),
  };

  contextEngine = {
    status: (repo_path: string) => this.call("context_engine.status", { repo_path }),
    enable: (repo_path: string) => this.call("context_engine.enable", { repo_path }),
    disable: (repo_path: string) => this.call("context_engine.disable", { repo_path }),
    search: (repo_path: string, query: string) => this.call("context_engine.search", { repo_path, query }),
    related: (repo_path: string, file_path: string) => this.call("context_engine.related", { repo_path, file_path }),
    fileIndex: (repo_path: string, file_path: string) =>
      this.call("context_engine.file_index", { repo_path, file_path }),
    recent: (repo_path: string) => this.call("context_engine.recent", { repo_path }),
  };

  // ---- Phase 2 ----

  backgroundModel = {
    status: () => this.call("background_model.status", {}),
    configure: (params: unknown) => this.call("background_model.configure", params),
    submitTask: (params: unknown) => this.call("background_model.submit_task", params),
    listTasks: () => this.call("background_model.list_tasks", {}),
  };

  subagent = {
    spawn: (params: unknown) => this.call("subagent.spawn", params),
    list: (mission_id: string) => this.call("subagent.list", { mission_id }),
    get: (id: string) => this.call("subagent.get", { id }),
    cancel: (id: string) => this.call("subagent.cancel", { id }),
  };

  semanticEngine = {
    status: (repo_path: string) => this.call("semantic_engine.status", { repo_path }),
    enable: (repo_path: string) => this.call("semantic_engine.enable", { repo_path }),
    disable: (repo_path: string) => this.call("semantic_engine.disable", { repo_path }),
    search: (repo_path: string, query: string) => this.call("semantic_engine.search", { repo_path, query }),
    dependencyGraph: (repo_path: string) => this.call("semantic_engine.dependency_graph", { repo_path }),
    gitBlame: (repo_path: string, file_path: string) =>
      this.call("semantic_engine.git_blame", { repo_path, file_path }),
    loadBlame: (repo_path: string, file_path: string, blames: unknown) =>
      this.call("semantic_engine.load_blame", { repo_path, file_path, blames }),
    indexFile: (repo_path: string, file_path: string, content: string) =>
      this.call("semantic_engine.index_file", { repo_path, file_path, content }),
    testImpactForSymbol: (repo_path: string, symbol: string) =>
      this.call("semantic_engine.test_impact.for_symbol", { repo_path, symbol }),
    testImpactForSymbols: (repo_path: string, symbols: string[]) =>
      this.call("semantic_engine.test_impact.for_symbols", { repo_path, symbols }),
    testImpactEntries: (repo_path: string) => this.call("semantic_engine.test_impact.entries", { repo_path }),
    docsForSymbol: (repo_path: string, symbol: string) =>
      this.call("semantic_engine.docs.for_symbol", { repo_path, symbol }),
    docsStale: (repo_path: string) => this.call("semantic_engine.docs.stale", { repo_path }),
  };

  mcpTasks = {
    create: (params: unknown) => this.call("mcp.task.create", params),
    poll: (task_id: string) => this.call("mcp.task.poll", { task_id }),
    cancel: (task_id: string) => this.call("mcp.task.cancel", { task_id }),
    list: () => this.call("mcp.task.list", {}),
  };

  sandbox = {
    test: (worktree_path: string) => this.call("sandbox.test", { worktree_path }),
    status: () => this.call("sandbox.status", {}),
    networkAllowlistGet: () => this.call("sandbox.network_allowlist.get", {}),
    networkAllowlistSet: (allowed_hosts: string[]) =>
      this.call("sandbox.network_allowlist.set", { allowed_hosts }),
  };

  // ---- Phase 3 ----

  auth = {
    status: () => this.call("auth.status", {}),
    register: (params: { username: string; password: string; role?: string; session_token?: string }) =>
      this.call("auth.register", params),
    login: (username: string, password: string) => this.call("auth.login", { username, password }),
    logout: (session_token: string) => this.call("auth.logout", { session_token }),
    session: (session_token: string) => this.call("auth.session", { session_token }),
    users: (session_token: string) => this.call("auth.users.list", { session_token }),
    setRole: (session_token: string, user_id: string, role: string) =>
      this.call("auth.user.set_role", { session_token, user_id, role }),
    setActive: (session_token: string, user_id: string, active: boolean) =>
      this.call("auth.user.set_active", { session_token, user_id, active }),
    changePassword: (session_token: string, new_password: string, user_id?: string) =>
      this.call("auth.user.change_password", { session_token, new_password, user_id }),
  };

  slack = {
    configure: (config: { workspace_id: string; webhook_url: string; signing_secret?: string; bot_token?: string; enabled: boolean; allowed_channels: string[]; default_channel?: string; trigger_prefix?: string }) =>
      this.call("slack.configure", config),
    configGet: (workspace_id: string) => this.call("slack.config.get", { workspace_id }),
  };

  teams = {
    configure: (config: { workspace_id: string; webhook_url: string; enabled: boolean; allowed_teams: string[]; allowed_channels: string[]; trigger_keywords: string[] }) =>
      this.call("teams.configure", config),
    configGet: (workspace_id: string) => this.call("teams.config.get", { workspace_id }),
  };

  decisions = {
    list: (repo_path: string) => this.call("decisions.list", { repo_path }),
    forMission: (mission_id: string) => this.call("decisions.for_mission", { mission_id }),
  };

  deployment = {
    record: (params: { mission_id: string; environment: string; commit_or_tag: string; ci_run_url?: string; note?: string }) =>
      this.call("deployment.record", params),
    list: (mission_id: string) => this.call("deployment.list", { mission_id }),
  };

  governance = {
    getPolicy: (workspace_id?: string) => this.call("governance.policy.get", { workspace_id }),
    setPolicy: (session_token: string, policy: unknown) =>
      this.call("governance.policy.set", { session_token, policy }),
    checkAutonomous: (session_token: string, repo_path: string, workspace_id?: string) =>
      this.call("governance.check.autonomous", { session_token, repo_path, workspace_id }),
    checkPlanApproval: (session_token: string, workspace_id?: string) =>
      this.call("governance.check.plan_approval", { session_token, workspace_id }),
    checkMerge: (session_token: string, workspace_id?: string) =>
      this.call("governance.check.merge", { session_token, workspace_id }),
    checkSpend: (mission_id: string, usd: number, workspace_id?: string) =>
      this.call("governance.spend.check", { mission_id, usd, workspace_id }),
    recordSpend: (mission_id: string, usd: number, note?: string, workspace_id?: string) =>
      this.call("governance.spend.record", { mission_id, usd, note, workspace_id }),
    spendSummary: (mission_id?: string, workspace_id?: string) =>
      this.call("governance.spend.summary", { mission_id, workspace_id }),
  };

  forge = {
    connect: (params: {
      repo_path: string;
      kind: "gitlab" | "bitbucket";
      project: string;
      base_url?: string;
      token?: string;
    }) => this.call("forge.connect", params),
    config: (repo_path: string) => this.call("forge.config.get", { repo_path }),
    disconnect: (repo_path: string) => this.call("forge.disconnect", { repo_path }),
    issues: (repo_path: string, state = "opened") => this.call("forge.issues.list", { repo_path, state }),
    issue: (repo_path: string, number: number) => this.call("forge.issue.get", { repo_path, number }),
    issueToMission: (repo_path: string, issue_number: number, session_mode?: string) =>
      this.call("forge.issue.to_mission", { repo_path, issue_number, session_mode }),
    createChangeRequest: (params: {
      repo_path: string;
      title: string;
      source_branch: string;
      target_branch?: string;
      body?: string;
    }) => this.call("forge.change_request.create", params),
    changeRequests: (repo_path: string) => this.call("forge.change_request.list", { repo_path }),
    changeRequestStatus: (repo_path: string, number: number) =>
      this.call("forge.change_request.status", { repo_path, number }),
  };

  tracker = {
    status: () => this.call("tracker.status", {}),
    setToken: (tracker: "jira" | "linear", token: string) =>
      this.call("tracker.token.set", { tracker, token }),
    issue: (params: { tracker: string; issue_key: string; site_url?: string; email?: string }) =>
      this.call("tracker.issue.get", params),
    link: (params: {
      mission_id: string;
      tracker: string;
      issue_key: string;
      site_url?: string;
      email?: string;
    }) => this.call("tracker.link", params),
    links: (mission_id: string) => this.call("tracker.links.list", { mission_id }),
    unlink: (link_id: string) => this.call("tracker.unlink", { link_id }),
    issueToMission: (params: {
      repo_path: string;
      tracker: string;
      issue_key: string;
      site_url?: string;
      email?: string;
      session_mode?: string;
    }) => this.call("tracker.issue.to_mission", params),
    comment: (params: {
      tracker: string;
      issue_key: string;
      body: string;
      site_url?: string;
      email?: string;
    }) => this.call("tracker.comment", params),
  };
}

export const api = new CidApiClient();
