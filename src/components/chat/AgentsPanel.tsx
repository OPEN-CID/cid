import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/api";
import { toast } from "@/lib/dialog";
import { useCid } from "@/hooks/useCid";
import { Bot, FlaskConical, Square, RefreshCw, Play } from "lucide-react";

type Subagent = {
  id: string;
  session_id: string;
  role: string;
  prompt: string;
  status: string;
  model_provider?: string | null;
  model_id?: string | null;
  result?: { summary?: string; files_changed?: string[]; success?: boolean } | null;
};

type BackgroundTask = {
  id: string;
  kind?: string;
  status?: string;
  detail?: string;
};

const STATUS_COLOR: Record<string, string> = {
  running: "bg-blue-500",
  pending: "bg-yellow-500",
  completed: "bg-green-500",
  failed: "bg-red-500",
  cancelled: "bg-muted-foreground/40",
};

// Live view over the agents this app can run: session subagents (spawned by
// the Implementer for parallel scoped work) and background model tasks. Both
// RPC surfaces existed with zero UI — including two client calls (`get` and
// `cancel`) that sent `{id}` where the server requires `subagent_id` and
// therefore always failed.
export function AgentsPanel() {
  const { sessions, repos, selectedSessionId, selectSession, loadSessions } = useCid();
  const session = sessions.find((m) => m.id === selectedSessionId);
  const repo = repos.find((r) => r.id === session?.repo_channel_id);

  const [subagents, setSubagents] = useState<Subagent[]>([]);
  const [tasks, setTasks] = useState<BackgroundTask[]>([]);
  const [loading, setLoading] = useState(false);
  const [sandboxing, setSandboxing] = useState(false);

  const load = useCallback(async () => {
    if (!selectedSessionId) return;
    setLoading(true);
    try {
      const [subs, bg] = await Promise.all([
        api.subagent.list(selectedSessionId).catch(() => []),
        api.backgroundModel.listTasks().catch(() => []),
      ]);
      setSubagents(Array.isArray(subs) ? subs : []);
      setTasks(Array.isArray(bg) ? bg : []);
    } finally {
      setLoading(false);
    }
  }, [selectedSessionId]);

  useEffect(() => {
    setSubagents([]);
    setTasks([]);
    load();
  }, [load]);

  const cancelSubagent = async (id: string) => {
    try {
      await api.subagent.cancel(id);
      await load();
    } catch (e) {
      toast.error(`Failed to cancel subagent: ${e}`);
    }
  };

  // One click, one isolated sandbox: a fresh Manual worktree session — full
  // filesystem isolation via the worktree, human-driven so nothing executes
  // on its own — pinned to a local model when one is actually installed, so
  // the sandbox costs nothing and leaks nothing.
  const newSandbox = async () => {
    if (!repo) return;
    setSandboxing(true);
    try {
      const created = (await api.session.create({
        repo_channel_id: repo.id,
        title: `Sandbox ${new Date().toLocaleTimeString()}`,
        task: "Isolated sandbox for hands-on work",
        isolation_mode: "worktree",
        autonomy_level: "manual",
      })) as { id?: string } | null;
      if (!created?.id) throw new Error("session creation returned no id");
      try {
        const status = await api.localRuntime.status();
        const installed: string[] = Array.isArray(status?.installed_models)
          ? status.installed_models
          : [];
        if (status?.running && installed.length > 0) {
          await api.session.setModel(created.id, "ollama", installed[0]);
        }
      } catch {
        // Local pinning is best-effort: the sandbox is fully usable on the
        // default model when no local runtime is up.
      }
      await loadSessions(repo.id);
      selectSession(created.id);
      toast.success("Sandbox ready — isolated worktree, manual control");
    } catch (e) {
      toast.error(`Failed to open sandbox: ${e}`);
    } finally {
      setSandboxing(false);
    }
  };

  if (!selectedSessionId || !repo) {
    return <div className="p-4 text-xs text-muted-foreground">Select a session to see its agents.</div>;
  }

  const active = subagents.filter((s) => s.status === "pending" || s.status === "running");

  return (
    <div className="p-4 space-y-4 overflow-y-auto h-full text-xs">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-1 font-medium text-sm">
          <Bot className="w-3.5 h-3.5" /> Agents · {repo.name}
        </div>
        <div className="flex gap-1.5">
          <button
            onClick={load}
            aria-label="Refresh agents"
            className="px-1.5 py-0.5 rounded bg-secondary flex items-center gap-1"
          >
            <RefreshCw className="w-3 h-3" /> Refresh
          </button>
          <button
            onClick={newSandbox}
            disabled={sandboxing}
            className="px-2 py-0.5 rounded bg-primary text-primary-foreground flex items-center gap-1 disabled:opacity-50"
          >
            <FlaskConical className="w-3 h-3" /> {sandboxing ? "Opening…" : "New isolated sandbox"}
          </button>
        </div>
      </div>
      <div className="text-[11px] text-muted-foreground">
        A sandbox is a fresh Manual session in its own worktree — isolated from every other Session — pinned to a
        local model when one is running, otherwise on the default model.
      </div>

      <div className="space-y-1.5">
        <div className="font-medium">
          Subagents {active.length > 0 && <span className="text-muted-foreground">({active.length} active)</span>}
        </div>
        {loading && <div className="text-muted-foreground">Loading…</div>}
        {!loading && subagents.length === 0 && (
          <div className="text-muted-foreground">No subagents on this session. The Implementer spawns them for scoped parallel work.</div>
        )}
        {subagents.map((s) => (
          <div key={s.id} className="p-2 border rounded bg-background space-y-1">
            <div className="flex items-center gap-1.5">
              <span className={`w-2 h-2 rounded-full ${STATUS_COLOR[s.status] ?? "bg-muted-foreground/40"}`} />
              <span className="font-medium">{s.role}</span>
              {(s.model_provider || s.model_id) && (
                <span className="text-[10px] text-muted-foreground font-mono">
                  {s.model_provider} {s.model_id}
                </span>
              )}
              <span className="text-[10px] text-muted-foreground ml-auto">{s.status.replace("_", " ")}</span>
              {(s.status === "pending" || s.status === "running") && (
                <button
                  onClick={() => cancelSubagent(s.id)}
                  className="px-1.5 py-0.5 rounded bg-secondary flex items-center gap-1"
                >
                  <Square className="w-2.5 h-2.5" /> Cancel
                </button>
              )}
            </div>
            <div className="text-[11px] text-muted-foreground truncate" title={s.prompt}>
              {s.prompt}
            </div>
            {s.result && (
              <div className="text-[11px]">
                {s.result.summary ?? (s.result.success ? "Completed" : "Finished")}
                {Array.isArray(s.result.files_changed) && s.result.files_changed.length > 0 && (
                  <span className="text-muted-foreground"> · {s.result.files_changed.length} file(s)</span>
                )}
              </div>
            )}
          </div>
        ))}
      </div>

      <div className="space-y-1.5">
        <div className="font-medium">Background tasks</div>
        {tasks.length === 0 && <div className="text-muted-foreground">No background tasks.</div>}
        {tasks.map((t) => (
          <div key={t.id} className="p-2 border rounded bg-background flex items-center gap-1.5">
            <Play className="w-3 h-3 text-muted-foreground" />
            <span className="font-mono text-[11px]">{t.kind ?? "task"}</span>
            <span className="text-[10px] text-muted-foreground ml-auto">{t.status ?? "unknown"}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
