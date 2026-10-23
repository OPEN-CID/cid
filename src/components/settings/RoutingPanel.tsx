import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/api";
import { toast } from "@/lib/dialog";
import { Zap } from "lucide-react";

type Decision = {
  provider: string;
  model_id: string;
  mode: string;
  complexity: string;
  reason: string;
  is_local: boolean;
};

const MODES: { id: string; label: string; blurb: string }[] = [
  { id: "manual", label: "Manual", blurb: "Today's behavior: role pins, then global settings. The router never intervenes." },
  { id: "auto_cost", label: "Auto · cost", blurb: "Cheapest capable route per turn — a running local model first, else the cheapest priced default." },
  { id: "auto_balanced", label: "Auto · balanced", blurb: "Task complexity picks the tier: cheap models for small turns, stronger ones as work grows. Local first when running." },
  { id: "auto_performance", label: "Auto · performance", blurb: "Strongest priced default every turn. Never routes local." },
  { id: "auto_local", label: "Auto · local only", blurb: "Ollama on this machine or nothing — refuses with guidance rather than silently using cloud. Needs a local model downloaded and running (Local models tab)." },
];

// Install-wide automatic model routing. Explicit choices always win: a
// per-session or per-step override bypasses every auto mode, and Planner /
// Reviewer turns stay on explicit resolution (planning quality dominates and
// planning is infrequent — nothing meaningful to optimize there).
export function RoutingPanel() {
  const [mode, setMode] = useState("manual");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [probe, setProbe] = useState("");
  const [decision, setDecision] = useState<Decision | null>(null);
  const [previewing, setPreviewing] = useState(false);

  const load = useCallback(async () => {
    try {
      const res = await api.model.routingGet();
      setMode(res?.mode ?? "manual");
    } catch (e) {
      toast.error(`Failed to load routing mode: ${e}`);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const save = async (next: string) => {
    setSaving(true);
    try {
      const res = await api.model.routingSet(next);
      setMode(res?.mode ?? next);
      toast.success(`Routing: ${res?.mode ?? next}`);
    } catch (e) {
      toast.error(`Failed to set routing mode: ${e}`);
    } finally {
      setSaving(false);
    }
  };

  const preview = async () => {
    if (!probe.trim()) return;
    setPreviewing(true);
    try {
      setDecision(await api.model.routeRecommend({ task: probe.trim() }));
    } catch (e) {
      toast.error(`Routing preview failed: ${e}`);
    } finally {
      setPreviewing(false);
    }
  };

  if (loading) return <div className="p-4 text-xs text-muted-foreground">Loading routing…</div>;

  return (
    <div className="p-4 space-y-4 overflow-y-auto h-full text-xs">
      <div className="flex items-center gap-1 font-medium text-sm">
        <Zap className="w-3.5 h-3.5" /> Automatic model routing
      </div>
      <div className="text-[11px] text-muted-foreground space-y-1">
        <div>
          Auto routing picks <span className="text-foreground">which</span> of your configured models each turn runs
          on — it does not add or download models. Whatever you set up under Cloud providers or Local models is the
          pool it chooses from.
        </div>
        <div>
          Applies to Implementer and subagent turns. A session or step with an explicitly chosen model always keeps
          it — auto never overrules a human choice. Planner and Reviewer stay manual.
        </div>
      </div>

      <div className="space-y-1.5" role="radiogroup" aria-label="Routing mode">
        {MODES.map((m) => (
          <label
            key={m.id}
            className={`flex gap-2 p-2 border rounded cursor-pointer ${mode === m.id ? "ring-1 ring-primary bg-background" : "bg-background"}`}
          >
            <input
              type="radio"
              name="routing-mode"
              value={m.id}
              checked={mode === m.id}
              disabled={saving}
              onChange={() => save(m.id)}
              className="mt-0.5"
            />
            <span>
              <span className="font-medium">{m.label}</span>
              <span className="block text-[11px] text-muted-foreground">{m.blurb}</span>
            </span>
          </label>
        ))}
      </div>

      <div className="space-y-1.5">
        <div className="font-medium">Preview a routing decision</div>
        <div className="flex gap-1.5">
          <input
            className="flex-1 bg-background border rounded px-2 py-1 text-[11px]"
            placeholder="Describe a task, e.g. fix typo in readme"
            value={probe}
            onChange={(e) => setProbe(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") preview();
            }}
          />
          <button
            onClick={preview}
            disabled={!probe.trim() || previewing}
            className="px-2 py-1 rounded bg-secondary disabled:opacity-50"
          >
            {previewing ? "Routing…" : "Preview"}
          </button>
        </div>
        {decision && (
          <div className="p-2 border rounded bg-background space-y-0.5">
            <div className="font-medium">
              {decision.provider} {decision.model_id}
              {decision.is_local && <span className="ml-1 text-[10px] px-1 rounded bg-green-500/20">local · $0</span>}
            </div>
            <div className="text-[11px] text-muted-foreground">
              {decision.mode} · {decision.complexity} task
            </div>
            <div className="text-[11px]">{decision.reason}</div>
          </div>
        )}
      </div>
    </div>
  );
}
