import { useCallback, useEffect, useState } from "react";
import { api } from "../../lib/api";
import { useCid } from "../../hooks/useCid";
import { ExternalLink, RotateCcw, RefreshCw } from "lucide-react";

type AcpEditor = {
  id: string;
  name: string;
  editor_type: string;
  available: boolean;
  executable_path: string;
  version?: string | null;
  supports_acp: boolean;
};

type AcpHandoff = {
  id: string;
  session_id: string;
  editor_id: string;
  status: string;
  worktree_path: string;
  created_at: string;
  returned_at?: string | null;
};

const ACTIVE_STATUSES = ["handed_off", "in_external_editor"];

export function AcpPanel() {
  const { selectedSessionId } = useCid();
  const [editors, setEditors] = useState<AcpEditor[]>([]);
  const [handoffs, setHandoffs] = useState<AcpHandoff[]>([]);
  const [loading, setLoading] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [e, h] = await Promise.all([
        api.acp.editors(),
        api.acp.handoffs(selectedSessionId ?? undefined),
      ]);
      setEditors(e ?? []);
      setHandoffs(h ?? []);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [selectedSessionId]);

  useEffect(() => {
    load();
  }, [load]);

  const handoff = async (editorId: string) => {
    if (!selectedSessionId) return;
    setBusyId(editorId);
    setError(null);
    try {
      await api.acp.handoff(selectedSessionId, editorId);
      await load();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusyId(null);
    }
  };

  const takeBack = async (handoffId: string) => {
    setBusyId(handoffId);
    setError(null);
    try {
      await api.acp.takeBack(handoffId);
      await load();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusyId(null);
    }
  };

  const active = handoffs.filter((h) => ACTIVE_STATUSES.includes(h.status));

  return (
    <div className="h-full overflow-y-auto p-3 text-sm">
      <div className="flex items-center justify-between mb-3">
        <div>
          <div className="font-medium">External Editors</div>
          <div className="text-[11px] text-muted-foreground">
            Agent Client Protocol handoff — pop this Session out to a full IDE and take it back
          </div>
        </div>
        <button
          onClick={load}
          className="text-xs flex items-center gap-1 px-2 py-1 rounded bg-secondary"
          disabled={loading}
        >
          <RefreshCw className={`w-3 h-3 ${loading ? "animate-spin" : ""}`} /> Rescan
        </button>
      </div>

      {error && (
        <div className="mb-3 p-2 rounded border border-red-500/40 bg-red-500/10 text-[11px] text-red-300">
          {error}
        </div>
      )}

      {!selectedSessionId && (
        <div className="mb-3 p-2 rounded border bg-background text-[11px] text-muted-foreground">
          Select a Session to hand its worktree off to an editor.
        </div>
      )}

      {active.length > 0 && (
        <div className="mb-4">
          <div className="text-xs font-medium mb-1.5">Active handoffs</div>
          <div className="space-y-1.5">
            {active.map((h) => (
              <div key={h.id} className="p-2 border rounded bg-background">
                <div className="flex items-center gap-2">
                  <span className="text-xs font-medium">{h.editor_id}</span>
                  <span className="text-[10px] px-1.5 py-0.5 rounded bg-blue-500/20 text-blue-300">
                    {h.status}
                  </span>
                  <button
                    onClick={() => takeBack(h.id)}
                    disabled={busyId === h.id}
                    className="ml-auto text-[11px] flex items-center gap-1 px-2 py-0.5 rounded bg-secondary disabled:opacity-50"
                  >
                    <RotateCcw className="w-3 h-3" />
                    {busyId === h.id ? "…" : "Take back"}
                  </button>
                </div>
                <div className="text-[10px] text-muted-foreground mt-1 truncate">{h.worktree_path}</div>
              </div>
            ))}
          </div>
        </div>
      )}

      <div className="text-xs font-medium mb-1.5">Detected editors</div>
      <div className="space-y-1.5">
        {editors.map((e) => (
          <div key={e.id} className="p-2 border rounded bg-background">
            <div className="flex items-center gap-2">
              <span className={`w-2 h-2 rounded-full ${e.available ? "bg-green-500" : "bg-muted-foreground/40"}`} />
              <span className="text-xs font-medium">{e.name}</span>
              {e.supports_acp ? (
                <span className="text-[10px] px-1.5 py-0.5 rounded bg-green-500/15 text-green-300">ACP</span>
              ) : (
                <span className="text-[10px] px-1.5 py-0.5 rounded bg-secondary text-muted-foreground">
                  folder open
                </span>
              )}
              <button
                onClick={() => handoff(e.id)}
                disabled={!e.available || !selectedSessionId || busyId === e.id}
                className="ml-auto text-[11px] flex items-center gap-1 px-2 py-0.5 rounded bg-primary text-primary-foreground disabled:opacity-40"
              >
                <ExternalLink className="w-3 h-3" />
                {busyId === e.id ? "…" : "Hand off"}
              </button>
            </div>
            <div className="text-[10px] text-muted-foreground mt-1 truncate">
              {e.available ? e.executable_path : "not installed"}
              {e.version ? ` · ${e.version}` : ""}
            </div>
          </div>
        ))}
        {editors.length === 0 && !loading && (
          <div className="text-[11px] text-muted-foreground">No editor definitions returned.</div>
        )}
      </div>

      <div className="mt-4 text-[10px] text-muted-foreground border-t pt-2">
        Taking a session back marks it as returned in CID; it does not close the external editor.
      </div>
    </div>
  );
}
