import { useEffect, useState } from "react";
import { useCid } from "@/hooks/useCid";
import { api } from "@/lib/api";
import { toast } from "@/lib/dialog";
import { cn } from "@/lib/utils";
import { FolderGit2, Plus, Settings, Box, Zap, FileText, Search, Loader2 } from "lucide-react";
import { ThemeToggle } from "./ThemeToggle";

export function LeftRail() {
  const { repos, selectedRepoId, missions, selectedMissionId, selectRepo, selectMission, loadRepos } = useCid();
  const [newRepoPath, setNewRepoPath] = useState("");
  const [showAddRepo, setShowAddRepo] = useState(false);
  // Context Engine (Part 7): off by default per repo, the same mental model
  // as enabling a VS Code extension — this toggle is the RPC surface's only
  // real UI entry point (review_prompt.md §4: it existed, tested, with a
  // frontend API wrapper already written, but no component ever called it).
  const [contextEngineEnabled, setContextEngineEnabled] = useState(false);
  const [contextEngineBusy, setContextEngineBusy] = useState(false);
  const selectedRepo = repos.find((r) => r.id === selectedRepoId);

  useEffect(() => {
    loadRepos();
  }, [loadRepos]);

  useEffect(() => {
    if (!selectedRepo) return;
    let cancelled = false;
    api.contextEngine
      .status(selectedRepo.path)
      .then((status) => {
        if (!cancelled) setContextEngineEnabled(!!status?.enabled);
      })
      .catch(() => {
        if (!cancelled) setContextEngineEnabled(false);
      });
    return () => {
      cancelled = true;
    };
    // Deliberately keyed on the path, not the `selectedRepo` object: it's
    // re-derived by `.find()` every render, so depending on it directly would
    // re-run this effect (and re-fetch status) whenever `repos` changes for
    // any reason, not just when the selected repo actually changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedRepo?.path]);

  const toggleContextEngine = async () => {
    if (!selectedRepo || contextEngineBusy) return;
    setContextEngineBusy(true);
    try {
      const status = await (contextEngineEnabled
        ? api.contextEngine.disable(selectedRepo.path)
        : api.contextEngine.enable(selectedRepo.path));
      setContextEngineEnabled(!!status?.enabled);
    } catch (e) {
      toast.error(`Failed to toggle Context Engine: ${e}`);
    } finally {
      setContextEngineBusy(false);
    }
  };

  const handleConnectRepo = async () => {
    if (!newRepoPath.trim()) return;
    try {
      const repo = await api.repo.connect(newRepoPath.trim());
      setNewRepoPath("");
      setShowAddRepo(false);
      await loadRepos();
      selectRepo(repo.id);
    } catch (e) {
      toast.error(`Failed to connect repo: ${e}`);
    }
  };

  return (
    <div className="w-[280px] h-full bg-card border-r flex flex-col">
      {/* Header */}
      <div className="p-4 border-b flex items-center gap-2">
        <div className="w-8 h-8 rounded bg-primary flex items-center justify-center font-bold text-primary-foreground">C</div>
        <div>
          <div className="font-semibold text-sm">CID</div>
          <div className="text-xs text-muted-foreground">Mission Control</div>
        </div>
      </div>

      {/* Repo Channels */}
      <div className="flex-1 overflow-y-auto">
        <div className="p-3">
          <div className="flex items-center justify-between mb-2">
            <span className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">Repo Channels</span>
            <button
              onClick={() => setShowAddRepo(!showAddRepo)}
              className="p-1 hover:bg-accent rounded"
              aria-label={showAddRepo ? "Cancel connecting a repo" : "Connect a repo"}
              aria-expanded={showAddRepo}
            >
              <Plus className="w-4 h-4" />
            </button>
          </div>

          {showAddRepo && (
            <div className="mb-3 p-2 bg-accent/50 rounded flex flex-col gap-2">
              <input
                className="w-full bg-background border rounded px-2 py-1 text-sm"
                placeholder="C:\path\to\repo or /home/user/repo"
                value={newRepoPath}
                onChange={(e) => setNewRepoPath(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && handleConnectRepo()}
                autoFocus
              />
              <div className="flex gap-2">
                <button onClick={handleConnectRepo} className="flex-1 bg-primary text-primary-foreground text-xs py-1 rounded">
                  Connect
                </button>
                <button onClick={() => setShowAddRepo(false)} className="flex-1 bg-secondary text-xs py-1 rounded">
                  Cancel
                </button>
              </div>
            </div>
          )}

          <div className="space-y-1">
            {repos.map((repo) => (
              <div key={repo.id}>
                <button
                  onClick={() => selectRepo(repo.id)}
                  className={cn(
                    "w-full text-left px-2 py-2 rounded flex items-center gap-2 text-sm hover:bg-accent",
                    selectedRepoId === repo.id && "bg-accent text-accent-foreground"
                  )}
                >
                  <FolderGit2 className="w-4 h-4 shrink-0" />
                  <span className="truncate">{repo.name}</span>
                </button>

                {/* Missions under selected repo */}
                {selectedRepoId === repo.id && (
                  <div className="ml-6 mt-1 space-y-1 border-l pl-2">
                    {missions.map((m) => (
                      <button
                        key={m.id}
                        onClick={() => selectMission(m.id)}
                        className={cn(
                          "w-full text-left px-2 py-1.5 rounded text-xs flex items-center gap-1.5 hover:bg-accent/50",
                          selectedMissionId === m.id && "bg-primary/20 text-primary"
                        )}
                      >
                        <span
                          className={cn(
                            "w-2 h-2 rounded-full",
                            m.status === "running"
                              ? "bg-yellow-500"
                              : m.status === "done" || m.status === "review"
                              ? "bg-green-500"
                              : m.status === "blocked_on_approval"
                              ? "bg-orange-500"
                              : "bg-muted-foreground"
                          )}
                        />
                        <span className="truncate">{m.title}</span>
                      </button>
                    ))}
                    {missions.length === 0 && <div className="text-[11px] text-muted-foreground px-2 py-1">No missions yet</div>}
                  </div>
                )}
              </div>
            ))}
            {repos.length === 0 && (
              <div className="text-xs text-muted-foreground p-2">
                No repos connected. Click + to add a local git repo.
              </div>
            )}
          </div>
        </div>

        {/* Pinned context */}
        {selectedRepoId && (
          <div className="p-3 border-t mt-3">
            <div className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-2">Context</div>
            <div className="space-y-1">
              <div className="flex items-center gap-2 text-xs p-1.5 rounded bg-accent/30">
                <FileText className="w-3 h-3" />
                <span>AGENTS.md</span>
                <span className="ml-auto text-[10px] bg-green-500/20 text-green-400 px-1 rounded">auto</span>
              </div>
              <div className="flex items-center gap-2 text-xs p-1.5 rounded hover:bg-accent/50 cursor-pointer">
                <Box className="w-3 h-3" />
                <span>Skills</span>
              </div>
              <div className="flex items-center gap-2 text-xs p-1.5 rounded hover:bg-accent/50 cursor-pointer">
                <Zap className="w-3 h-3" />
                <span>MCP Servers</span>
              </div>
              <button
                onClick={toggleContextEngine}
                disabled={contextEngineBusy}
                title="Tree-sitter structural indexing + search, off by default per repo (Part 7) — enabling it costs CPU/memory for large repos."
                className="w-full flex items-center gap-2 text-xs p-1.5 rounded hover:bg-accent/50 disabled:opacity-50"
              >
                <Search className="w-3 h-3" />
                <span>Context Engine</span>
                {contextEngineBusy ? (
                  <Loader2 className="w-3 h-3 ml-auto animate-spin" />
                ) : (
                  <span
                    className={cn(
                      "ml-auto text-[10px] px-1 rounded",
                      contextEngineEnabled ? "bg-green-500/20 text-green-400" : "bg-muted text-muted-foreground"
                    )}
                  >
                    {contextEngineEnabled ? "on" : "off"}
                  </span>
                )}
              </button>
            </div>
          </div>
        )}
      </div>

      {/* Footer */}
      <div className="p-3 border-t flex items-center justify-between">
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <div className="w-2 h-2 rounded-full bg-green-500 animate-pulse" />
          <span>Core</span>
        </div>
        <div className="flex items-center gap-1">
          <ThemeToggle />
          <button
            onClick={() => window.dispatchEvent(new CustomEvent("cid:open-settings"))}
            className="p-1 hover:bg-accent rounded"
            aria-label="Open settings"
          >
            <Settings className="w-4 h-4" />
          </button>
        </div>
      </div>
    </div>
  );
}
