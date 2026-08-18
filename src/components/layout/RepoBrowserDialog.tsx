import { useEffect, useState } from "react";
import { api, type ListDirsResult } from "@/lib/api";
import { toast } from "@/lib/dialog";
import { useFocusTrap } from "@/lib/useFocusTrap";
import { Folder, FolderGit2, CornerUpLeft, Loader2, Check } from "lucide-react";

/**
 * Walks the filesystem via `fs.list_dirs` so a user can find a repo to
 * connect without knowing (or typing) its exact path. Directories only —
 * the RPC never returns files, so there's nothing here to filter out.
 */
export function RepoBrowserDialog({
  onClose,
  onConnected,
}: {
  onClose: () => void;
  onConnected: (repo: { id: string; name: string }) => void;
}) {
  const modalRef = useFocusTrap<HTMLDivElement>(true, onClose);
  const [path, setPath] = useState<string | null>(null);
  const [listing, setListing] = useState<ListDirsResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [connecting, setConnecting] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    api.fs
      .listDirs(path)
      .then((res) => {
        if (!cancelled) setListing(res);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [path]);

  const currentPath = listing?.path ?? path;

  const handleConnect = async () => {
    if (!currentPath) return;
    setConnecting(true);
    try {
      const repo = await api.repo.connect(currentPath);
      onConnected(repo);
    } catch (e) {
      toast.error(`Failed to connect repo: ${e}`);
    } finally {
      setConnecting(false);
    }
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-[110]">
      <div
        ref={modalRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="repo-browser-title"
        tabIndex={-1}
        className="bg-card border rounded-lg p-4 w-[480px] max-w-[90vw] max-h-[80vh] flex flex-col"
      >
        <h2 id="repo-browser-title" className="font-semibold mb-2 text-sm">
          Browse for a repository
        </h2>
        <div className="text-xs text-muted-foreground mb-2 truncate font-mono">
          {currentPath ?? "Filesystem roots"}
        </div>

        <div className="flex-1 min-h-[240px] overflow-y-auto border rounded bg-background">
          {loading ? (
            <div className="h-full flex items-center justify-center py-8">
              <Loader2 className="w-4 h-4 animate-spin text-muted-foreground" />
            </div>
          ) : error ? (
            <div className="p-3 text-xs text-destructive">{error}</div>
          ) : (
            <div className="divide-y">
              {listing?.parent != null && (
                <button
                  onClick={() => setPath(listing.parent)}
                  className="w-full flex items-center gap-2 px-3 py-2 text-sm text-left hover:bg-accent"
                >
                  <CornerUpLeft className="w-3.5 h-3.5 shrink-0" />
                  <span>..</span>
                </button>
              )}
              {listing?.entries.map((entry) => (
                <button
                  key={entry.path}
                  onClick={() => setPath(entry.path)}
                  className="w-full flex items-center gap-2 px-3 py-2 text-sm text-left hover:bg-accent"
                >
                  {entry.is_git_repo ? (
                    <FolderGit2 className="w-3.5 h-3.5 shrink-0" />
                  ) : (
                    <Folder className="w-3.5 h-3.5 shrink-0" />
                  )}
                  <span className="truncate flex-1">{entry.name}</span>
                  {entry.is_git_repo && (
                    <span
                      className="ml-auto flex items-center gap-0.5 text-[10px] bg-green-500/20 text-green-400 px-1 rounded shrink-0"
                      title="Git repository"
                    >
                      <Check className="w-3 h-3" /> repo
                    </span>
                  )}
                </button>
              ))}
              {listing && listing.entries.length === 0 && (
                <div className="p-3 text-xs text-muted-foreground">No subdirectories</div>
              )}
            </div>
          )}
        </div>

        <div className="flex justify-end gap-2 mt-3">
          <button onClick={onClose} className="px-4 py-2 text-sm bg-secondary rounded">
            Cancel
          </button>
          <button
            onClick={handleConnect}
            disabled={!currentPath || connecting || loading}
            className="px-4 py-2 text-sm bg-primary text-primary-foreground rounded disabled:opacity-50"
          >
            {connecting ? "Connecting..." : "Connect"}
          </button>
        </div>
      </div>
    </div>
  );
}
