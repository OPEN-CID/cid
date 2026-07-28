import { useCallback, useEffect, useState } from "react";
import { api } from "../../lib/api";
import { History, RotateCcw, Loader2, AlertTriangle } from "lucide-react";

type MissionCheckpoint = {
  id: string;
  mission_id: string;
  sha: string;
  label: string;
  created_at: string;
};

/**
 * review_prompt.md §3.2: checkpoints are auto-recorded on the Mission's
 * worktree before every turn (ModelManager::auto_checkpoint), but had no UI
 * — a bad turn could only be undone by hand-editing git. Only renders for
 * worktree-mode Missions, since shared-clone Missions have nothing to
 * checkpoint against.
 */
export function CheckpointCard({ missionId, refreshOn }: { missionId: string; refreshOn?: number }) {
  const [hasWorktree, setHasWorktree] = useState(false);
  const [checkpoints, setCheckpoints] = useState<MissionCheckpoint[]>([]);
  const [confirmingId, setConfirmingId] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const mission = await api.mission.get(missionId);
      setHasWorktree(!!mission?.worktree_path);
      const list = await api.mission.checkpointList(missionId);
      setCheckpoints(list ?? []);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, [missionId]);

  useEffect(() => {
    load();
  }, [load, refreshOn]);

  const rewind = async (checkpointId: string) => {
    setBusyId(checkpointId);
    setError(null);
    try {
      await api.mission.checkpointRewind(missionId, checkpointId, true);
      setConfirmingId(null);
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyId(null);
    }
  };

  if (!hasWorktree || checkpoints.length === 0) return null;

  return (
    <div className="border rounded-lg p-3 bg-card">
      <div className="flex items-center gap-2 mb-1.5">
        <History className="w-3.5 h-3.5" />
        <span className="text-sm font-semibold">Checkpoints</span>
        <span className="text-[10px] text-muted-foreground ml-auto">
          {checkpoints.length} saved
        </span>
      </div>

      {error && <div className="text-[11px] text-red-300 mb-1.5">{error}</div>}

      <div className="space-y-1.5">
        {[...checkpoints].reverse().map((cp) => (
          <div
            key={cp.id}
            className="flex items-center gap-2 text-xs bg-background/40 rounded p-1.5"
          >
            <div className="min-w-0 flex-1">
              <div className="truncate">{cp.label}</div>
              <div className="text-[10px] text-muted-foreground font-mono">
                {cp.sha.slice(0, 7)} · {new Date(cp.created_at).toLocaleTimeString()}
              </div>
            </div>

            {confirmingId === cp.id ? (
              <div className="flex items-center gap-1.5 shrink-0">
                <span className="text-[10px] text-yellow-300 flex items-center gap-1">
                  <AlertTriangle className="w-3 h-3" /> Discards later changes
                </span>
                <button
                  onClick={() => rewind(cp.id)}
                  disabled={busyId === cp.id}
                  className="text-[10px] px-1.5 py-0.5 rounded bg-red-500/80 text-white disabled:opacity-50"
                >
                  {busyId === cp.id ? <Loader2 className="w-3 h-3 animate-spin" /> : "Confirm"}
                </button>
                <button
                  onClick={() => setConfirmingId(null)}
                  className="text-[10px] px-1.5 py-0.5 rounded bg-muted"
                >
                  Cancel
                </button>
              </div>
            ) : (
              <button
                onClick={() => setConfirmingId(cp.id)}
                className="shrink-0 text-[10px] flex items-center gap-1 px-1.5 py-0.5 rounded bg-muted hover:bg-muted/70"
                title="Rewind the Mission's worktree to this checkpoint"
              >
                <RotateCcw className="w-3 h-3" /> Rewind
              </button>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
