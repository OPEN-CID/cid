import { useCallback, useEffect, useState } from "react";
import { useCid } from "@/hooks/useCid";
import { useSessionRepoPath } from "@/hooks/useSessionRepoPath";
import { api } from "@/lib/api";
import { ConfidenceCard } from "../confidence/ConfidenceCard";

type DiffFile = {
  path: string;
  status: string;
  hunks: { id: string; content: string; header: string; old_start: number; old_lines: number; new_start: number; new_lines: number }[];
  additions: number;
  deletions: number;
};

export function DiffViewer() {
  const { selectedSessionId } = useCid();
  const [diff, setDiff] = useState<DiffFile[]>([]);
  const [loading, setLoading] = useState(false);
  const [viewMode, setViewMode] = useState<"unified" | "split">("unified");
  const [actionLog, setActionLog] = useState<Record<string, string>>({});

  // Shared with the Editor so the two cannot drift apart again — they did, and
  // saved edits silently failed to appear here. See the hook.
  const repoPath = useSessionRepoPath();

  const loadDiff = useCallback(async () => {
    if (!repoPath) return;
    setLoading(true);
    try {
      const d = await api.git.diff(repoPath);
      setDiff(d);
    } catch (e) {
      console.error(e);
    } finally {
      setLoading(false);
    }
  }, [repoPath]);

  useEffect(() => {
    loadDiff();
  }, [loadDiff]);

  useEffect(() => {
    // Re-subscribing whenever `loadDiff` changes (i.e. `repoPath` changes)
    // matters, not just satisfies the lint rule — without it this closure
    // kept calling a stale `loadDiff` bound to the previous repoPath if the
    // repo changed without selectedSessionId also changing.
    const unsub = api.onNotification((notif) => {
      if (notif.method === "git.diff.update") {
        loadDiff();
      }
    });
    return () => unsub();
  }, [selectedSessionId, loadDiff]);

  const handleHunkAction = async (filePath: string, hunk: DiffFile["hunks"][number], action: "accept" | "reject") => {
    if (!repoPath) return;
    const hunkId = hunk.id;
    try {
      setActionLog((prev) => ({ ...prev, [hunkId]: `${action}ing...` }));
      // The hunk's own header + content travel with the request so the
      // backend can reverse-apply exactly this hunk (git.hunk.apply's
      // hunk_id alone can't identify a hunk server-side — see the handler's
      // own comment in router.rs — a fresh id is minted on every git.diff
      // call, so an id from an earlier response means nothing later).
      await api.call("git.hunk.apply", {
        repo_path: repoPath,
        file_path: filePath,
        hunk_id: hunkId,
        action,
        header: hunk.header,
        content: hunk.content,
      });
      setActionLog((prev) => ({ ...prev, [hunkId]: action === "accept" ? "✓ Accepted" : "✗ Rejected" }));
      setTimeout(() => {
        setActionLog((prev) => {
          const copy = { ...prev };
          delete copy[hunkId];
          return copy;
        });
        loadDiff();
      }, 1000);
    } catch (e) {
      console.error(e);
      setActionLog((prev) => ({ ...prev, [hunkId]: `Error: ${e}` }));
    }
  };

  const handleFileAction = async (filePath: string, action: "accept" | "reject") => {
    if (!repoPath) return;
    try {
      if (action === "reject") {
        // File-level reject: git checkout HEAD -- file
        await api.call("git.hunk.apply", { repo_path: repoPath, file_path: filePath, hunk_id: "all", action: "reject" });
      }
      setTimeout(loadDiff, 500);
    } catch (e) {
      console.error(e);
    }
  };

  if (!selectedSessionId) {
    return <div className="h-full flex items-center justify-center text-sm text-muted-foreground">Select a session to view diff</div>;
  }

  return (
    <div className="h-full flex flex-col">
      <div className="h-10 border-b flex items-center px-3 gap-2">
        <span className="text-sm font-medium">Diff</span>
        <span className="text-xs text-muted-foreground">{diff.length} files changed</span>
        <div className="ml-auto flex gap-1">
          <button
            onClick={() => setViewMode("unified")}
            className={`text-xs px-2 py-1 rounded ${viewMode === "unified" ? "bg-accent" : "hover:bg-accent"}`}
          >
            Unified
          </button>
          <button
            onClick={() => setViewMode("split")}
            className={`text-xs px-2 py-1 rounded ${viewMode === "split" ? "bg-accent" : "hover:bg-accent"}`}
          >
            Split
          </button>
          <button onClick={loadDiff} className="text-xs px-2 py-1 rounded bg-primary text-primary-foreground ml-2">
            Refresh
          </button>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto">
        {loading ? (
          <div className="p-4 text-sm text-muted-foreground">Loading diff...</div>
        ) : diff.length === 0 ? (
          <div className="p-4 text-sm text-muted-foreground">No changes detected — clean working tree. Make edits in Session thread or via terminal.</div>
        ) : (
          <div className="divide-y">
            {diff.map((file) => (
              <div key={file.path} className="p-3">
                <div className="flex items-center gap-2 text-sm mb-2">
                  <span className="font-mono text-xs px-1.5 py-0.5 rounded bg-accent">{file.status}</span>
                  <span className="font-medium truncate">{file.path}</span>
                  <span className="text-xs text-green-400 ml-2">+{file.additions}</span>
                  <span className="text-xs text-red-400">-{file.deletions}</span>
                  <div className="ml-auto flex gap-1">
                    <button
                      onClick={() => handleFileAction(file.path, "accept")}
                      className="text-[11px] px-1.5 py-0.5 rounded bg-green-600/20 text-green-400 hover:bg-green-600/30"
                    >
                      Accept file
                    </button>
                    <button
                      onClick={() => handleFileAction(file.path, "reject")}
                      className="text-[11px] px-1.5 py-0.5 rounded bg-red-600/20 text-red-400 hover:bg-red-600/30"
                    >
                      Reject file
                    </button>
                  </div>
                </div>
                {selectedSessionId && <ConfidenceCard sessionId={selectedSessionId} filePath={file.path} />}
                <div className="bg-background rounded border overflow-hidden mt-2">
                  {file.hunks.map((hunk) => (
                    <div key={hunk.id} className="border-b last:border-0">
                      <div className="bg-accent/50 px-3 py-1 text-[11px] font-mono text-muted-foreground flex items-center gap-2">
                        <span>{hunk.header || `@@ -${hunk.old_start},${hunk.old_lines} +${hunk.new_start},${hunk.new_lines} @@`}</span>
                        <span className="ml-auto flex items-center gap-1">
                          {actionLog[hunk.id] && <span className="text-[10px] text-yellow-400">{actionLog[hunk.id]}</span>}
                          <button
                            onClick={() => handleHunkAction(file.path, hunk, "accept")}
                            className="text-[10px] px-1 py-0.5 rounded bg-green-600/20 text-green-400 hover:bg-green-600/30"
                          >
                            Accept hunk
                          </button>
                          <button
                            onClick={() => handleHunkAction(file.path, hunk, "reject")}
                            className="text-[10px] px-1 py-0.5 rounded bg-red-600/20 text-red-400 hover:bg-red-600/30"
                          >
                            Reject hunk
                          </button>
                        </span>
                      </div>
                      <pre className="text-xs font-mono p-2 overflow-x-auto whitespace-pre-wrap">
                        {hunk.content.split("\n").map((line, i) => (
                          <div
                            key={i}
                            className={
                              line.startsWith("+") ? "bg-green-500/10 text-green-300" : line.startsWith("-") ? "bg-red-500/10 text-red-300" : "text-muted-foreground"
                            }
                          >
                            {line}
                          </div>
                        ))}
                      </pre>
                    </div>
                  ))}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="p-2 border-t text-[11px] text-muted-foreground">
        Per-hunk Accept/Reject: Accept keeps changes (auto-commit per logical change via git.commit). Reject hunk reverse-applies just that hunk via <code>git apply -R</code>, leaving the rest of the file untouched; Reject file discards the whole file via <code>git checkout HEAD -- file</code>. Auto-commit pattern: Aider-style atomic commits.
      </div>
    </div>
  );
}
