import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/api";
import { toast } from "@/lib/dialog";
import { useCid } from "@/hooks/useCid";
import { ListOrdered, Play, Square, Check, RotateCcw, Plus, Trash2, RefreshCw } from "lucide-react";

type StepKind = "agent" | "command" | "manual_gate";

type StepDraft = {
  id: string;
  name: string;
  kind: StepKind;
  prompt: string;
  command: string;
  model_provider: string;
  model_id: string;
  on_failure: "stop" | "continue";
};

type Workflow = {
  id: string;
  name: string;
  description: string;
  steps: {
    id: string;
    name: string;
    kind: StepKind;
    prompt?: string | null;
    command?: string | null;
    model_provider?: string | null;
    model_id?: string | null;
    on_failure: "stop" | "continue";
  }[];
};

type StepRun = {
  step_id: string;
  status: string;
  output?: string | null;
  error?: string | null;
};

type Run = {
  id: string;
  workflow_id: string;
  session_id: string;
  status: string;
  current_index: number;
  steps: StepRun[];
};

type PendingTool = {
  id: string;
  session_id: string;
  name: string;
  arguments: unknown;
};

const STATUS_COLOR: Record<string, string> = {
  running: "bg-blue-500",
  waiting_approval: "bg-orange-500",
  completed: "bg-green-500",
  failed: "bg-red-500",
  cancelled: "bg-muted-foreground/40",
  pending: "bg-muted-foreground/40",
  skipped: "bg-muted-foreground/40",
};

function newStepDraft(): StepDraft {
  return {
    id: `new-${Math.random().toString(36).slice(2)}`,
    name: "",
    kind: "agent",
    prompt: "",
    command: "",
    model_provider: "",
    model_id: "",
    on_failure: "stop",
  };
}

// Workflows v1: linear chains of agent / command / manual-gate steps run
// against the selected Session. A step's model override accepts any provider
// id — including a local runtime (Ollama/LM Studio/llama.cpp) — so "run this
// step on a local model" is a per-step model choice, not a separate feature.
export function WorkflowPanel() {
  const { sessions, repos, selectedSessionId } = useCid();
  const session = sessions.find((m) => m.id === selectedSessionId);
  const repo = repos.find((r) => r.id === session?.repo_channel_id);

  const [workflows, setWorkflows] = useState<Workflow[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [runs, setRuns] = useState<Run[]>([]);
  const [activeRun, setActiveRun] = useState<Run | null>(null);
  const [pendingTools, setPendingTools] = useState<PendingTool[]>([]);
  const [loading, setLoading] = useState(false);
  const [showCreate, setShowCreate] = useState(false);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [steps, setSteps] = useState<StepDraft[]>([newStepDraft()]);
  const [approveNote, setApproveNote] = useState("");

  const selected = workflows.find((w) => w.id === selectedId) ?? null;

  const load = useCallback(async () => {
    if (!repo) return;
    setLoading(true);
    try {
      const list = await api.workflow.list(repo.id);
      setWorkflows(list || []);
    } catch (e) {
      toast.error(`Failed to list workflows: ${e}`);
    } finally {
      setLoading(false);
    }
  }, [repo]);

  const loadRuns = useCallback(async (workflowId: string) => {
    try {
      const list = await api.workflow.runList(workflowId);
      setRuns(list || []);
    } catch (e) {
      toast.error(`Failed to list runs: ${e}`);
    }
  }, []);

  useEffect(() => {
    setSelectedId(null);
    setActiveRun(null);
    setRuns([]);
    load();
  }, [load]);

  useEffect(() => {
    if (selectedId) loadRuns(selectedId);
    else setRuns([]);
  }, [selectedId, loadRuns]);

  // While a run is live, re-read it — the runner persists every transition,
  // so polling converges on the same state the notifications carry. Pending
  // tool approvals are polled alongside: a command step parked on approval
  // is otherwise invisible outside the Chat thread, which is exactly the
  // "clicked Start and nothing happens" report this answers.
  useEffect(() => {
    if (!activeRun || (activeRun.status !== "running" && activeRun.status !== "waiting_approval")) {
      setPendingTools([]);
      return;
    }
    const poll = async () => {
      try {
        const run = await api.workflow.runGet(activeRun.id);
        setActiveRun(run);
        if (selectedId) loadRuns(selectedId);
      } catch {
        /* a deleted run simply stops refreshing */
      }
      try {
        const pending = await api.session.pendingTools(activeRun.session_id);
        setPendingTools(Array.isArray(pending) ? pending : []);
      } catch {
        /* approvals polling is best-effort next to the run state itself */
      }
    };
    poll();
    const t = setInterval(poll, 2000);
    return () => clearInterval(t);
  }, [activeRun, selectedId, loadRuns]);

  const submitCreate = async () => {
    if (!repo || !name.trim()) return;
    try {
      const payload = steps.map((s) => ({
        id: s.id.startsWith("new-") ? undefined : s.id,
        name: s.name.trim(),
        kind: s.kind,
        prompt: s.kind === "agent" ? s.prompt : undefined,
        command: s.kind === "command" ? s.command : undefined,
        model_provider: s.model_provider.trim() || undefined,
        model_id: s.model_id.trim() || undefined,
        on_failure: s.on_failure,
      }));
      const created = await api.workflow.create({
        repo_channel_id: repo.id,
        name: name.trim(),
        description: description.trim() || undefined,
        steps: payload,
      });
      setName("");
      setDescription("");
      setSteps([newStepDraft()]);
      setShowCreate(false);
      await load();
      setSelectedId(created.id);
    } catch (e) {
      toast.error(`Failed to create workflow: ${e}`);
    }
  };

  const startRun = async () => {
    if (!selected || !selectedSessionId) return;
    try {
      const run = await api.workflow.runStart(selected.id, selectedSessionId);
      setActiveRun(run);
      loadRuns(selected.id);
    } catch (e) {
      toast.error(`Failed to start run: ${e}`);
    }
  };

  const cancelRun = async () => {
    if (!activeRun) return;
    try {
      const run = await api.workflow.runCancel(activeRun.id);
      setActiveRun(run);
      if (selectedId) loadRuns(selectedId);
    } catch (e) {
      toast.error(`Failed to cancel run: ${e}`);
    }
  };

  const approveRun = async () => {
    if (!activeRun) return;
    try {
      const run = await api.workflow.runApprove(activeRun.id, approveNote.trim() || undefined);
      setApproveNote("");
      setActiveRun(run);
      if (selectedId) loadRuns(selectedId);
    } catch (e) {
      toast.error(`Failed to approve step: ${e}`);
    }
  };

  const retryRun = async () => {
    if (!activeRun) return;
    try {
      const run = await api.workflow.runRetry(activeRun.id);
      setActiveRun(run);
      if (selectedId) loadRuns(selectedId);
    } catch (e) {
      toast.error(`Failed to retry run: ${e}`);
    }
  };

  const decideTool = async (toolCallId: string, approved: boolean) => {
    if (!activeRun) return;
    try {
      await api.session.approveTool(activeRun.session_id, toolCallId, approved);
      const pending = await api.session.pendingTools(activeRun.session_id);
      setPendingTools(Array.isArray(pending) ? pending : []);
    } catch (e) {
      toast.error(`Failed to record approval: ${e}`);
    }
  };

  const refreshRun = async () => {
    if (!activeRun) return;
    try {
      setActiveRun(await api.workflow.runGet(activeRun.id));
    } catch (e) {
      toast.error(`Failed to refresh run: ${e}`);
    }
  };

  if (!selectedSessionId) {
    return <div className="p-4 text-xs text-muted-foreground">Select a session to run a workflow against it.</div>;
  }

  return (
    <div className="p-4 space-y-4 overflow-y-auto h-full text-xs">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-1 font-medium">
          <ListOrdered className="w-3.5 h-3.5" /> Workflows
        </div>
        <button
          onClick={() => setShowCreate((v) => !v)}
          className="px-2 py-0.5 rounded bg-primary text-primary-foreground flex items-center gap-1"
        >
          <Plus className="w-3 h-3" /> New
        </button>
      </div>
      <div className="text-[10px] text-muted-foreground">
        Ordered steps — agent turns, shell commands, human gates — run against this Session. Each agent step can name
        its own model, including a local runtime. Or describe the pipeline in chat (“create a workflow that runs the
        tests, then asks me before committing”) and edit what the agent drafts here.
      </div>

      {showCreate && (
        <div className="p-2 border rounded bg-background space-y-2">
          <input
            className="w-full bg-background border rounded px-2 py-1 text-[11px]"
            placeholder="Workflow name"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
          <input
            className="w-full bg-background border rounded px-2 py-1 text-[11px]"
            placeholder="Description (optional)"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
          />
          {steps.map((s, i) => (
            <div key={s.id} className="p-2 border rounded space-y-1.5">
              <div className="flex items-center gap-1.5">
                <span className="text-muted-foreground">#{i + 1}</span>
                <input
                  className="flex-1 bg-background border rounded px-2 py-1 text-[11px]"
                  placeholder="Step name"
                  value={s.name}
                  onChange={(e) => setSteps(steps.map((x) => (x.id === s.id ? { ...x, name: e.target.value } : x)))}
                />
                <select
                  aria-label={`Step ${i + 1} kind`}
                  className="bg-background border rounded px-1 py-1 text-[11px]"
                  value={s.kind}
                  onChange={(e) =>
                    setSteps(steps.map((x) => (x.id === s.id ? { ...x, kind: e.target.value as StepKind } : x)))
                  }
                >
                  <option value="agent">agent</option>
                  <option value="command">command</option>
                  <option value="manual_gate">manual gate</option>
                </select>
                <button
                  aria-label={`Remove step ${i + 1}`}
                  onClick={() => setSteps(steps.filter((x) => x.id !== s.id))}
                  className="px-1.5 py-1 rounded bg-secondary"
                >
                  <Trash2 className="w-3 h-3" />
                </button>
              </div>
              {s.kind === "agent" && (
                <>
                  <textarea
                    className="w-full bg-background border rounded px-2 py-1 text-[11px] font-mono"
                    placeholder="Prompt for this turn"
                    rows={2}
                    value={s.prompt}
                    onChange={(e) => setSteps(steps.map((x) => (x.id === s.id ? { ...x, prompt: e.target.value } : x)))}
                  />
                  <div className="flex gap-1.5">
                    <input
                      className="flex-1 bg-background border rounded px-2 py-1 text-[11px] font-mono"
                      placeholder="Model provider override (optional)"
                      value={s.model_provider}
                      onChange={(e) =>
                        setSteps(steps.map((x) => (x.id === s.id ? { ...x, model_provider: e.target.value } : x)))
                      }
                    />
                    <input
                      className="flex-1 bg-background border rounded px-2 py-1 text-[11px] font-mono"
                      placeholder="Model id override (optional)"
                      value={s.model_id}
                      onChange={(e) =>
                        setSteps(steps.map((x) => (x.id === s.id ? { ...x, model_id: e.target.value } : x)))
                      }
                    />
                  </div>
                </>
              )}
              {s.kind === "command" && (
                <input
                  className="w-full bg-background border rounded px-2 py-1 text-[11px] font-mono"
                  placeholder="Shell command (runs under the repo allow-list)"
                  value={s.command}
                  onChange={(e) => setSteps(steps.map((x) => (x.id === s.id ? { ...x, command: e.target.value } : x)))}
                />
              )}
              <select
                aria-label={`Step ${i + 1} failure policy`}
                className="bg-background border rounded px-1 py-1 text-[11px]"
                value={s.on_failure}
                onChange={(e) =>
                  setSteps(steps.map((x) => (x.id === s.id ? { ...x, on_failure: e.target.value as "stop" | "continue" } : x)))
                }
              >
                <option value="stop">stop on failure</option>
                <option value="continue">continue on failure</option>
              </select>
            </div>
          ))}
          <div className="flex gap-1.5">
            <button onClick={() => setSteps([...steps, newStepDraft()])} className="px-2 py-1 rounded bg-secondary">
              Add step
            </button>
            <button
              onClick={submitCreate}
              disabled={!name.trim()}
              className="px-3 py-1 rounded bg-primary text-primary-foreground disabled:opacity-50"
            >
              Save workflow
            </button>
          </div>
        </div>
      )}

      {loading && <div className="text-muted-foreground">Loading…</div>}
      {!loading && workflows.length === 0 && (
        <div className="text-muted-foreground">No workflows on this repo yet.</div>
      )}
      <div className="space-y-1">
        {workflows.map((w) => (
          <button
            key={w.id}
            onClick={() => {
              setSelectedId(w.id);
              setActiveRun(null);
            }}
            className={`w-full text-left p-2 border rounded bg-background ${selectedId === w.id ? "ring-1 ring-primary" : ""}`}
          >
            <div className="font-medium">{w.name}</div>
            <div className="text-[10px] text-muted-foreground">
              {w.steps.length} step{w.steps.length === 1 ? "" : "s"} ·{" "}
              {w.steps.map((s) => s.kind).join(" → ")}
            </div>
          </button>
        ))}
      </div>

      {selected && (
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <div className="font-medium">Runs · {selected.name}</div>
            <button
              onClick={startRun}
              className="px-2 py-0.5 rounded bg-primary text-primary-foreground flex items-center gap-1"
            >
              <Play className="w-3 h-3" /> Start on this session
            </button>
          </div>
          {runs.length > 0 && (
            <div className="space-y-1">
              {runs.slice(0, 5).map((r) => (
                <button
                  key={r.id}
                  onClick={() => setActiveRun(r)}
                  className="w-full text-left p-1.5 border rounded bg-background flex items-center gap-2"
                >
                  <span className={`w-2 h-2 rounded-full ${STATUS_COLOR[r.status] ?? "bg-muted-foreground/40"}`} />
                  <span className="font-mono text-[10px] truncate">{r.id.slice(0, 8)}</span>
                  <span className="text-[10px] text-muted-foreground ml-auto">{r.status.replace("_", " ")}</span>
                </button>
              ))}
            </div>
          )}

          {activeRun && (
            <div className="p-2 border rounded bg-background space-y-2">
              <div className="flex items-center gap-2">
                <span className={`w-2 h-2 rounded-full ${STATUS_COLOR[activeRun.status] ?? ""}`} />
                <span className="font-medium">Run {activeRun.status.replace("_", " ")}</span>
                <button onClick={refreshRun} aria-label="Refresh run" className="ml-auto px-1.5 py-0.5 rounded bg-secondary">
                  <RefreshCw className="w-3 h-3" />
                </button>
                {(activeRun.status === "running" || activeRun.status === "waiting_approval") && (
                  <button
                    onClick={cancelRun}
                    className="px-2 py-0.5 rounded bg-secondary flex items-center gap-1"
                  >
                    <Square className="w-3 h-3" /> Cancel
                  </button>
                )}
                {(activeRun.status === "failed" || activeRun.status === "cancelled") && (
                  <button
                    onClick={retryRun}
                    className="px-2 py-0.5 rounded bg-secondary flex items-center gap-1"
                  >
                    <RotateCcw className="w-3 h-3" /> Retry
                  </button>
                )}
              </div>
              {pendingTools.length > 0 && (
                <div className="p-2 border border-orange-500/50 rounded space-y-1.5">
                  <div className="font-medium text-orange-400">
                    Waiting for your approval ({pendingTools.length})
                  </div>
                  {pendingTools.map((t) => (
                    <div key={t.id} className="flex items-center gap-1.5">
                      <span className="font-mono truncate flex-1" title={JSON.stringify(t.arguments)}>
                        {t.name}
                      </span>
                      <button
                        onClick={() => decideTool(t.id, true)}
                        className="px-2 py-0.5 rounded bg-primary text-primary-foreground"
                      >
                        Approve
                      </button>
                      <button
                        onClick={() => decideTool(t.id, false)}
                        className="px-2 py-0.5 rounded bg-secondary"
                      >
                        Deny
                      </button>
                    </div>
                  ))}
                </div>
              )}
              {activeRun.status === "waiting_approval" && (
                <div className="flex gap-1.5">
                  <input
                    className="flex-1 bg-background border rounded px-2 py-1 text-[11px]"
                    placeholder="Approval note (optional)"
                    value={approveNote}
                    onChange={(e) => setApproveNote(e.target.value)}
                  />
                  <button
                    onClick={approveRun}
                    className="px-2 py-1 rounded bg-primary text-primary-foreground flex items-center gap-1"
                  >
                    <Check className="w-3 h-3" /> Approve
                  </button>
                </div>
              )}
              <div className="space-y-1">
                {(selected.steps ?? []).map((s, i) => {
                  const sr = activeRun.steps[i];
                  return (
                    <div key={s.id} className="p-1.5 border rounded">
                      <div className="flex items-center gap-1.5">
                        <span className={`w-2 h-2 rounded-full ${STATUS_COLOR[sr?.status ?? "pending"] ?? ""}`} />
                        <span>
                          #{i + 1} {s.name}
                        </span>
                        <span className="text-[10px] text-muted-foreground ml-auto">
                          {s.kind.replace("_", " ")} · {sr?.status.replace("_", " ") ?? "pending"}
                        </span>
                      </div>
                      {sr?.output && (
                        <div className="mt-1 font-mono text-[10px] text-muted-foreground break-all">{sr.output}</div>
                      )}
                      {sr?.error && <div className="mt-1 text-[10px] text-red-500">{sr.error}</div>}
                    </div>
                  );
                })}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
