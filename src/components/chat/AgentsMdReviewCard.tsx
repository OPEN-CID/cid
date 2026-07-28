import { useEffect, useMemo, useState } from "react";
import { useCid } from "@/hooks/useCid";
import { api } from "@/lib/api";
import { ShieldAlert, Loader2 } from "lucide-react";

/**
 * review_prompt.md §1.2 point 2: `AGENTS.md` is repo-authored content CID
 * does not control. `handle_repo_connect` detects it but never approves it —
 * this card is the one-time human review step. Until approved,
 * `ModelManager::process_message_with_role` excludes AGENTS.md from every
 * Mission's system prompt on this repo.
 *
 * Content is fetched on demand via `repo.agents_md` rather than read off the
 * `repos` store — `repo.list`/`repo.get` never populate `agents_md_content`
 * (only the one-shot `repo.connect` response does; `SkillsPanel` already
 * fetches it the same on-demand way for the same reason).
 */
export function AgentsMdReviewCard({ missionId }: { missionId: string | null }) {
  const { missions, repos, loadRepos } = useCid();
  const [expanded, setExpanded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [content, setContent] = useState<string | null>(null);

  const repo = useMemo(() => {
    const mission = missions.find((m) => m.id === missionId);
    if (!mission) return null;
    return repos.find((r) => r.id === mission.repo_channel_id) ?? null;
  }, [missions, repos, missionId]);

  useEffect(() => {
    if (!repo || repo.agents_md_approved) {
      setContent(null);
      return;
    }
    let cancelled = false;
    api.repo
      .agentsMd(repo.path)
      .then((r: { content?: string | null }) => {
        if (!cancelled) setContent(r?.content ?? null);
      })
      .catch(() => {
        if (!cancelled) setContent(null);
      });
    return () => {
      cancelled = true;
    };
  }, [repo]);

  if (!repo || repo.agents_md_approved || !content) return null;

  const approve = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.repo.agentsMdApprove(repo.id);
      await loadRepos();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="border border-yellow-500/40 rounded-lg p-3 bg-yellow-500/5">
      <div className="flex items-center gap-2 mb-1.5">
        <ShieldAlert className="w-3.5 h-3.5 text-yellow-500" />
        <span className="text-sm font-semibold">This repo ships agent instructions</span>
      </div>
      <p className="text-xs text-muted-foreground mb-2">
        <span className="font-mono">{repo.name}</span> has an <span className="font-mono">AGENTS.md</span> that
        tells CID&rsquo;s agents how to work in this repo. It comes from the repository, not from you — review it
        before it&rsquo;s loaded into the model&rsquo;s context. Until approved, it&rsquo;s ignored.
      </p>

      <button
        onClick={() => setExpanded(!expanded)}
        className="text-[10px] px-1.5 py-0.5 rounded bg-muted hover:bg-muted/70 mb-2"
      >
        {expanded ? "Hide" : "Show"} contents
      </button>

      {expanded && (
        <pre className="text-[11px] bg-background/40 rounded p-2 mb-2 max-h-48 overflow-auto whitespace-pre-wrap">
          {content}
        </pre>
      )}

      {error && <div className="text-[11px] text-red-300 mb-1.5">{error}</div>}

      <div className="flex items-center gap-1.5">
        <button
          onClick={approve}
          disabled={busy}
          className="text-[10px] px-2 py-1 rounded bg-yellow-500/80 text-black font-medium disabled:opacity-50"
        >
          {busy ? <Loader2 className="w-3 h-3 animate-spin" /> : "Looks fine — use it"}
        </button>
      </div>
    </div>
  );
}
