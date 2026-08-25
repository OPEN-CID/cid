import { useCallback, useEffect, useState } from "react";
import { api } from "../../lib/api";
import { Check, X, Pencil, RefreshCw, Loader2 } from "lucide-react";

type PlanStatus = "draft" | "approved" | "rejected";

type SessionPlan = {
  id: string;
  session_id: string;
  content: string;
  status: PlanStatus;
  approved_by?: string | null;
  updated_at: string;
};

const STATUS_STYLE: Record<PlanStatus, string> = {
  draft: "bg-yellow-500/15 text-yellow-300 border-yellow-500/40",
  approved: "bg-green-500/15 text-green-300 border-green-500/40",
  rejected: "bg-red-500/15 text-red-300 border-red-500/40",
};

/**
 * The plan-approval card from Flow 1 step 3 — rendered inline in the Session
 * thread rather than in a modal, because the plan is part of the conversation.
 */
export function PlanCard({ sessionId }: { sessionId: string }) {
  const [plan, setPlan] = useState<SessionPlan | null>(null);
  const [blockedReason, setBlockedReason] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const res = await api.call("session.plan.get", { session_id: sessionId });
      setPlan(res?.plan ?? null);
      setBlockedReason(res?.implementer_blocked_reason ?? null);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, [sessionId]);

  useEffect(() => {
    load();
    const unsub = api.onNotification((n) => {
      if (n.method === "session.plan.changed" && n.params?.session_id === sessionId) load();
      if (n.method === "session.blocked" && n.params?.session_id === sessionId) load();
    });
    return () => unsub();
  }, [load, sessionId]);

  const act = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await fn();
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const generate = () =>
    act(() => api.call("session.plan.generate", { session_id: sessionId, force: !!plan }));
  const approve = () => act(() => api.call("session.plan.approve", { session_id: sessionId }));
  const reject = () => act(() => api.call("session.plan.reject", { session_id: sessionId }));
  const save = () =>
    act(async () => {
      await api.call("session.plan.update", { session_id: sessionId, content: draft });
      setEditing(false);
    });

  if (!plan) {
    return (
      <div className="border rounded-lg p-3 bg-card">
        <div className="flex items-center gap-2">
          <span className="text-sm font-medium">No plan yet</span>
          <button
            onClick={generate}
            disabled={busy}
            className="ml-auto text-xs flex items-center gap-1 px-2 py-1 rounded bg-primary text-primary-foreground disabled:opacity-50"
          >
            {busy ? <Loader2 className="w-3 h-3 animate-spin" /> : <RefreshCw className="w-3 h-3" />}
            Run Planner
          </button>
        </div>
        {blockedReason && <div className="text-[11px] text-muted-foreground mt-1.5">{blockedReason}</div>}
        {error && <div className="text-[11px] text-red-300 mt-1.5">{error}</div>}
      </div>
    );
  }

  return (
    <div className={`border rounded-lg p-3 ${STATUS_STYLE[plan.status]}`}>
      <div className="flex items-center gap-2 mb-2">
        <span className="text-sm font-semibold">Plan</span>
        <span className="text-[10px] uppercase tracking-wide px-1.5 py-0.5 rounded bg-background/40">
          {plan.status}
        </span>
        {plan.approved_by && (
          <span className="text-[10px] text-muted-foreground">approved by {plan.approved_by}</span>
        )}
        <div className="ml-auto flex gap-1.5">
          {!editing && (
            <button
              onClick={() => {
                setDraft(plan.content);
                setEditing(true);
              }}
              className="text-[11px] flex items-center gap-1 px-2 py-0.5 rounded bg-background/60"
            >
              <Pencil className="w-3 h-3" /> Edit
            </button>
          )}
          <button
            onClick={generate}
            disabled={busy}
            className="text-[11px] flex items-center gap-1 px-2 py-0.5 rounded bg-background/60 disabled:opacity-50"
          >
            <RefreshCw className="w-3 h-3" /> Re-plan
          </button>
        </div>
      </div>

      {editing ? (
        <>
          <textarea
            className="w-full bg-background border rounded p-2 text-xs font-mono min-h-[200px]"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
          />
          <div className="flex gap-2 mt-2">
            <button
              onClick={save}
              disabled={busy || !draft.trim()}
              className="text-xs px-3 py-1 rounded bg-primary text-primary-foreground disabled:opacity-50"
            >
              Save
            </button>
            <button onClick={() => setEditing(false)} className="text-xs px-3 py-1 rounded bg-secondary">
              Cancel
            </button>
          </div>
          <div className="text-[10px] text-muted-foreground mt-1.5">
            Saving returns the plan to draft — an approval applied to the previous text.
          </div>
        </>
      ) : (
        <pre className="whitespace-pre-wrap text-xs font-mono bg-background/40 rounded p-2 max-h-80 overflow-y-auto">
          {plan.content}
        </pre>
      )}

      {!editing && plan.status !== "approved" && (
        <div className="flex gap-2 mt-2">
          <button
            onClick={approve}
            disabled={busy}
            className="flex items-center gap-1 bg-green-600 hover:bg-green-700 text-white text-xs px-3 py-1 rounded disabled:opacity-50"
          >
            <Check className="w-3 h-3" /> Approve plan
          </button>
          <button
            onClick={reject}
            disabled={busy}
            className="flex items-center gap-1 bg-red-600 hover:bg-red-700 text-white text-xs px-3 py-1 rounded disabled:opacity-50"
          >
            <X className="w-3 h-3" /> Reject
          </button>
        </div>
      )}

      {blockedReason && (
        <div className="text-[11px] mt-2 border-t border-current/20 pt-1.5">
          Implementer is blocked: {blockedReason}
        </div>
      )}
      {error && <div className="text-[11px] text-red-300 mt-1.5">{error}</div>}
    </div>
  );
}
