import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/api";
import { toast } from "@/lib/dialog";
import { useCid } from "@/hooks/useCid";
import { GitPullRequest, Rocket, ExternalLink, RefreshCw } from "lucide-react";

type PullRequest = {
  id?: number | string;
  number: number;
  title: string;
  state?: string | null;
  author?: string | null;
  url?: string | null;
  head_branch?: string | null;
  base_branch?: string | null;
  source_branch?: string | null;
  target_branch?: string | null;
};

type PrStatus = {
  state?: string | null;
  mergeable?: boolean | null;
  checks?: { name?: string; status?: string; conclusion?: string | null }[] | null;
};

type Release = {
  id: string;
  session_id: string;
  environment: string;
  commit_or_tag: string;
  source: string;
  deployed_at: string;
};

// Repo-level review dashboard: open pull/change requests across the
// connected forges plus the release log. Read-only composition over
// RPCs that already existed (github.pr.list, forge.change_request.list,
// deployment.list) with zero UI until now. Unconnected forges report a
// hint, never an error toast storm — connecting lives in settings.
export function ReviewsPanel() {
  const { sessions, repos, selectedSessionId } = useCid();
  const session = sessions.find((m) => m.id === selectedSessionId);
  const repo = repos.find((r) => r.id === session?.repo_channel_id);

  const [prs, setPrs] = useState<PullRequest[] | null>(null);
  const [changeRequests, setChangeRequests] = useState<PullRequest[] | null>(null);
  const [releases, setReleases] = useState<Release[]>([]);
  const [statusByPr, setStatusByPr] = useState<Record<string, PrStatus>>({});
  const [loading, setLoading] = useState(false);

  const load = useCallback(async () => {
    if (!repo) return;
    setLoading(true);
    try {
      const [gh, fg] = await Promise.all([
        api.github.prs(repo.path).catch(() => null),
        api.forge.changeRequests(repo.path).catch(() => null),
      ]);
      setPrs(Array.isArray(gh) ? gh : null);
      setChangeRequests(Array.isArray(fg) ? fg : null);
    } finally {
      setLoading(false);
    }
    try {
      const repoSessions = sessions.filter((s) => s.repo_channel_id === repo.id);
      const lists = await Promise.all(
        repoSessions.map((s) => api.deployment.list(s.id).catch(() => []))
      );
      const all: Release[] = lists
        .flat()
        .filter(Boolean)
        .sort((a, b) => (a.deployed_at < b.deployed_at ? 1 : -1));
      setReleases(all);
    } catch (e) {
      toast.error(`Failed to load releases: ${e}`);
    }
  }, [repo, sessions]);

  useEffect(() => {
    setPrs(null);
    setChangeRequests(null);
    setReleases([]);
    setStatusByPr({});
    load();
  }, [load]);

  const loadPrStatus = async (pr: PullRequest) => {
    if (!repo) return;
    const key = `${pr.number}`;
    if (statusByPr[key]) return;
    try {
      const st = await api.github.prStatus(repo.path, pr.number);
      setStatusByPr((prev) => ({ ...prev, [key]: st ?? {} }));
    } catch (e) {
      toast.error(`Failed to load PR status: ${e}`);
    }
  };

  if (!selectedSessionId || !repo) {
    return <div className="p-4 text-xs text-muted-foreground">Select a session to review the pull requests and releases for its repo.</div>;
  }

  const renderPr = (pr: PullRequest, source: string) => {
    const key = `${source}-${pr.number}`;
    const st = statusByPr[`${pr.number}`];
    const branches = pr.head_branch || pr.source_branch;
    const target = pr.base_branch || pr.target_branch;
    return (
      <div key={key} className="p-2 border rounded bg-background space-y-1">
        <div className="flex items-center gap-1.5">
          <span className="font-medium">#{pr.number}</span>
          <span className="truncate flex-1">{pr.title}</span>
          <span className="text-[10px] px-1 rounded bg-accent">{source}</span>
        </div>
        <div className="flex items-center gap-2 text-[10px] text-muted-foreground">
          <span>{pr.state ?? "open"}</span>
          {pr.author && <span>by {pr.author}</span>}
          {branches && (
            <span className="font-mono">
              {branches} → {target}
            </span>
          )}
          {pr.url && (
            <a href={pr.url} target="_blank" rel="noreferrer" className="text-primary flex items-center gap-0.5">
              <ExternalLink className="w-2.5 h-2.5" /> Open
            </a>
          )}
          {!st && source === "github" && (
            <button onClick={() => loadPrStatus(pr)} className="px-1.5 py-0.5 rounded bg-secondary">
              Check status
            </button>
          )}
        </div>
        {st && (
          <div className="text-[10px] text-muted-foreground">
            {st.mergeable !== undefined && st.mergeable !== null && (
              <span>{st.mergeable ? "Mergeable" : "Not mergeable"} · </span>
            )}
            {Array.isArray(st.checks) && st.checks.length > 0
              ? st.checks.map((c, i) => (
                  <span key={i} className="mr-2">
                    {c.name}: {c.conclusion ?? c.status}
                  </span>
                ))
              : "No check details reported."}
          </div>
        )}
      </div>
    );
  };

  return (
    <div className="p-4 space-y-4 overflow-y-auto h-full text-xs">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-1 font-medium text-sm">
          <GitPullRequest className="w-3.5 h-3.5" /> Reviews · {repo.name}
        </div>
        <button
          onClick={load}
          aria-label="Refresh reviews"
          className="px-1.5 py-0.5 rounded bg-secondary flex items-center gap-1"
        >
          <RefreshCw className="w-3 h-3" /> Refresh
        </button>
      </div>

      <div className="space-y-1.5">
        <div className="font-medium">Pull requests</div>
        {loading && <div className="text-muted-foreground">Loading…</div>}
        {!loading && prs === null && changeRequests === null && (
          <div className="text-muted-foreground">
            No forge connected for this repo. Connect GitHub or a GitLab/Bitbucket project in settings to list
            pull requests here.
          </div>

        )}
        {(prs ?? []).map((pr) => renderPr(pr, "github"))}
        {(changeRequests ?? []).map((pr) => renderPr(pr, "forge"))}
        {!loading && (prs ?? []).length === 0 && (changeRequests ?? []).length === 0 && prs !== null && (
          <div className="text-muted-foreground">No open pull requests.</div>
        )}
      </div>

      <div className="space-y-1.5">
        <div className="flex items-center gap-1 font-medium">
          <Rocket className="w-3.5 h-3.5" /> Releases
        </div>
        <div className="text-[10px] text-muted-foreground">
          Recorded deployments across the sessions of this repo — what went where, from the decision log.
        </div>
        {releases.length === 0 && <div className="text-muted-foreground">No releases recorded yet.</div>}
        {releases.map((r) => (
          <div key={r.id} className="p-2 border rounded bg-background">
            <div className="flex items-center gap-1.5">
              <span className="font-medium">{r.environment}</span>
              <span className="font-mono text-[10px]">{r.commit_or_tag}</span>
              <span className="text-[10px] text-muted-foreground ml-auto">
                {new Date(r.deployed_at).toLocaleString()}
              </span>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
