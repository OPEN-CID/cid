import { useCallback, useEffect, useState } from "react";
import { api } from "../../lib/api";
import { useCid } from "../../hooks/useCid";
import { Plus, Save, Trash2, RotateCcw } from "lucide-react";
import { RoleProfilesPanel } from "./RoleProfilesPanel";

type AllowedCommand = { pattern: string; description?: string | null; requires_approval: boolean };
type AutonomyAllowlist = {
  scope_id: string;
  allowed_commands: AllowedCommand[];
  allowed_paths: string[];
  denied_paths: string[];
  max_tool_calls: number | null;
};

/**
 * Autonomous-mode command controls (Part 14): which command patterns an
 * Autonomous Session may run without a per-step approval, and which always
 * stop for a human — e.g. `git commit` auto-approved, `git push`/PR-opening
 * commands always asked for. Scoped per repo, matching how the backend
 * allow-list is scoped (`autonomy.allowlist.*`, repo_channel id as scope_id).
 */
export function AutonomyPanel() {
  const { repos, selectedRepoId } = useCid();
  const [list, setList] = useState<AutonomyAllowlist | null>(null);
  const [loading, setLoading] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [newPattern, setNewPattern] = useState("");
  const [newDescription, setNewDescription] = useState("");
  const [newRequiresApproval, setNewRequiresApproval] = useState(true);

  const load = useCallback(async () => {
    if (!selectedRepoId) return;
    setLoading(true);
    try {
      const existing = await api.autonomy.allowlistGet(selectedRepoId);
      setList(
        existing && existing.exists !== false
          ? existing
          : await api.autonomy.allowlistDefault(selectedRepoId)
      );
    } catch (e) {
      setStatus(String(e));
    } finally {
      setLoading(false);
    }
  }, [selectedRepoId]);

  useEffect(() => {
    load();
  }, [load]);

  const save = async (next: AutonomyAllowlist) => {
    if (!selectedRepoId) return;
    setStatus(null);
    try {
      const saved = await api.autonomy.allowlistSet({
        scope_id: selectedRepoId,
        allowed_commands: next.allowed_commands,
        allowed_paths: next.allowed_paths,
        denied_paths: next.denied_paths,
      });
      setList(saved);
      setStatus("Saved");
    } catch (e) {
      setStatus(String(e));
    }
  };

  const resetToDefault = async () => {
    if (!selectedRepoId) return;
    const def = await api.autonomy.allowlistDefault(selectedRepoId);
    setList(def);
    setStatus("Reset to default");
  };

  const toggleApproval = (pattern: string) => {
    if (!list) return;
    const next = {
      ...list,
      allowed_commands: list.allowed_commands.map((c) =>
        c.pattern === pattern ? { ...c, requires_approval: !c.requires_approval } : c
      ),
    };
    setList(next);
    save(next);
  };

  const removeCommand = (pattern: string) => {
    if (!list) return;
    const next = { ...list, allowed_commands: list.allowed_commands.filter((c) => c.pattern !== pattern) };
    setList(next);
    save(next);
  };

  const addCommand = () => {
    if (!list || !newPattern.trim()) return;
    const next = {
      ...list,
      allowed_commands: [
        ...list.allowed_commands,
        { pattern: newPattern.trim(), description: newDescription.trim() || null, requires_approval: newRequiresApproval },
      ],
    };
    setList(next);
    save(next);
    setNewPattern("");
    setNewDescription("");
    setNewRequiresApproval(true);
  };

  const repoPath = repos.find((r) => r.id === selectedRepoId)?.path;

  if (!selectedRepoId) {
    return <div className="p-4 text-xs text-muted-foreground">Select a repo channel to manage its Autonomous-mode command controls.</div>;
  }

  return (
    <div className="p-4 space-y-4 overflow-y-auto h-full text-xs">
      <div className="flex items-center justify-between">
        <div>
          <div className="font-medium">Autonomous-mode command controls</div>
          <div className="text-[10px] text-muted-foreground truncate">{repoPath}</div>
        </div>
        <button onClick={resetToDefault} className="flex items-center gap-1 px-2 py-0.5 rounded bg-secondary">
          <RotateCcw className="w-3 h-3" /> Reset to default
        </button>
      </div>

      <div className="text-[10px] text-muted-foreground">
        In Autonomous mode, a command matching a pattern below with &quot;auto-run&quot; runs without
        stopping; a pattern set to &quot;ask first&quot; (or no match at all) always waits for your
        approval. Manual and Co-Pilot sessions are unaffected — every tool call there is already
        approved individually regardless of this list.
      </div>

      {status && <div className="text-muted-foreground">{status}</div>}

      {loading && <div className="text-muted-foreground">Loading…</div>}

      {list && (
        <>
          <div className="space-y-1.5">
            {list.allowed_commands.map((c) => (
              <div key={c.pattern} className="flex items-center gap-2 p-2 border rounded bg-background">
                <code className="text-[10px] flex-1 truncate">{c.pattern}</code>
                {c.description && <span className="text-[10px] text-muted-foreground hidden md:inline">{c.description}</span>}
                <button
                  onClick={() => toggleApproval(c.pattern)}
                  className={`text-[10px] px-2 py-0.5 rounded ${c.requires_approval ? "bg-yellow-500/20 text-yellow-600" : "bg-green-500/20 text-green-600"}`}
                >
                  {c.requires_approval ? "ask first" : "auto-run"}
                </button>
                <button
                  onClick={() => removeCommand(c.pattern)}
                  className="text-muted-foreground hover:text-red-500"
                  aria-label={`Remove pattern ${c.pattern}`}
                >
                  <Trash2 className="w-3 h-3" />
                </button>
              </div>
            ))}
            {list.allowed_commands.length === 0 && (
              <div className="text-muted-foreground">
                No patterns configured — every command will require approval, even in Autonomous mode.
              </div>
            )}
          </div>

          <div className="p-2 border rounded bg-background space-y-1.5">
            <div className="font-medium">Add a custom command pattern</div>
            <input
              className="w-full bg-background border rounded px-2 py-1 text-[11px] font-mono"
              placeholder="regex pattern, e.g. ^git commit "
              value={newPattern}
              onChange={(e) => setNewPattern(e.target.value)}
            />
            <input
              className="w-full bg-background border rounded px-2 py-1 text-[11px]"
              placeholder="description (optional)"
              value={newDescription}
              onChange={(e) => setNewDescription(e.target.value)}
            />
            <label className="flex items-center gap-1.5">
              <input type="checkbox" checked={newRequiresApproval} onChange={(e) => setNewRequiresApproval(e.target.checked)} />
              Always ask before running (uncheck to auto-run without approval)
            </label>
            <button
              onClick={addCommand}
              disabled={!newPattern.trim()}
              className="flex items-center gap-1 px-2 py-1 rounded bg-primary text-primary-foreground disabled:opacity-50"
            >
              <Plus className="w-3 h-3" /> Add pattern
            </button>
          </div>

          <div>
            <div className="font-medium mb-1">Denied paths</div>
            <textarea
              className="w-full bg-background border rounded px-2 py-1 text-[11px] font-mono min-h-[60px]"
              value={list.denied_paths.join("\n")}
              onChange={(e) => setList({ ...list, denied_paths: e.target.value.split("\n") })}
              onBlur={() => list && save(list)}
            />
          </div>

          <button onClick={() => save(list)} className="flex items-center gap-1 px-3 py-1.5 rounded bg-primary text-primary-foreground">
            <Save className="w-3 h-3" /> Save
          </button>
        </>
      )}

      <RoleProfilesPanel />
    </div>
  );
}
