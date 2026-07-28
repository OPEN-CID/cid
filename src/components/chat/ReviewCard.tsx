import { useCallback, useEffect, useState } from "react";
import { api } from "../../lib/api";
import { toast } from "../../lib/dialog";
import { AlertTriangle, Info, ShieldAlert, RefreshCw, Loader2, CheckCircle2, History } from "lucide-react";

type ReviewSeverity = "critical" | "warning" | "info";
type ReviewVerdict = "clean" | "comments_only" | "changes_requested" | "not_run";

type ReviewFinding = {
  severity: ReviewSeverity;
  file: string;
  description: string;
};

type MissionReview = {
  id: string;
  mission_id: string;
  verdict: ReviewVerdict;
  findings: ReviewFinding[];
  raw_output: string;
  created_at: string;
};

const VERDICT_STYLE: Record<ReviewVerdict, string> = {
  clean: "bg-green-500/15 text-green-300 border-green-500/40",
  comments_only: "bg-yellow-500/15 text-yellow-300 border-yellow-500/40",
  changes_requested: "bg-red-500/15 text-red-300 border-red-500/40",
  not_run: "bg-muted text-muted-foreground border-border",
};

const VERDICT_LABEL: Record<ReviewVerdict, string> = {
  clean: "Clean",
  comments_only: "Comments only",
  changes_requested: "Changes requested",
  not_run: "Not run",
};

const SEVERITY_ICON: Record<ReviewSeverity, JSX.Element> = {
  critical: <ShieldAlert className="w-3 h-3 text-red-400" />,
  warning: <AlertTriangle className="w-3 h-3 text-yellow-400" />,
  info: <Info className="w-3 h-3 text-blue-400" />,
};

/**
 * The Reviewer pass from Flow 1 step 6 — a second pass over the Implementer's
 * accumulated diff before it's presented for approval or opened as a PR
 * (Part 5). Mirrors PlanCard's shape: no modal, rendered inline in the
 * Mission thread. review_prompt.md §4: `mission.review.run/get/list` existed,
 * were tested, and had zero frontend surface — the Reviewer, one of the
 * three founding roles, was unreachable from any UI.
 */
export function ReviewCard({ missionId }: { missionId: string }) {
  const [review, setReview] = useState<MissionReview | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [history, setHistory] = useState<MissionReview[] | null>(null);

  const load = useCallback(async () => {
    try {
      const res = await api.missionReview.get(missionId);
      setReview(res ?? null);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, [missionId]);

  useEffect(() => {
    load();
  }, [load]);

  const runReview = async () => {
    setBusy(true);
    setError(null);
    try {
      const result = await api.missionReview.run(missionId);
      setReview(result ?? null);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  // 051-Editor-Excellence-Roadmap.md Wave 5.1f: mission.review.list had a
  // real backend and no caller — every past review run for this Mission, not
  // just the latest.
  const loadHistory = async () => {
    try {
      const reviews: MissionReview[] = await api.missionReview.list(missionId);
      setHistory(reviews || []);
    } catch (e) {
      toast.error(`Failed to load review history: ${e}`);
    }
  };

  return (
    <div className="border rounded-lg p-3 bg-card">
      <div className="flex items-center gap-2 mb-1">
        <span className="text-sm font-semibold">Reviewer</span>
        {review && (
          <span className={`text-[10px] uppercase tracking-wide px-1.5 py-0.5 rounded border ${VERDICT_STYLE[review.verdict]}`}>
            {VERDICT_LABEL[review.verdict]}
          </span>
        )}
        {review && (
          <button
            onClick={loadHistory}
            className="ml-auto p-1 text-muted-foreground hover:text-foreground"
            aria-label="Show review history"
          >
            <History className="w-3 h-3" />
          </button>
        )}
        <button
          onClick={runReview}
          disabled={busy}
          className={`${review ? "" : "ml-auto"} text-xs flex items-center gap-1 px-2 py-1 rounded bg-primary text-primary-foreground disabled:opacity-50`}
        >
          {busy ? <Loader2 className="w-3 h-3 animate-spin" /> : <RefreshCw className="w-3 h-3" />}
          {review ? "Re-review" : "Run Reviewer"}
        </button>
      </div>

      {error && <div className="text-[11px] text-red-300 mt-1.5">{error}</div>}

      {history && (
        <div className="mt-1.5 space-y-1 border-t pt-1.5">
          <div className="text-[10px] text-muted-foreground">Past reviews:</div>
          {history.length === 0 && <div className="text-[11px] text-muted-foreground">No prior reviews recorded.</div>}
          {history.map((h) => (
            <div key={h.id} className="flex items-center gap-2 text-[11px]">
              <span className={`text-[10px] uppercase tracking-wide px-1.5 py-0.5 rounded border ${VERDICT_STYLE[h.verdict]}`}>
                {VERDICT_LABEL[h.verdict]}
              </span>
              <span className="text-muted-foreground">{new Date(h.created_at).toLocaleString()}</span>
              <span className="text-muted-foreground">{h.findings.length} finding{h.findings.length === 1 ? "" : "s"}</span>
            </div>
          ))}
        </div>
      )}

      {!review && !error && (
        <div className="text-xs text-muted-foreground">
          A second pass over the accumulated diff — flags likely bugs, security issues, and scope
          creep before you approve or open a PR.
        </div>
      )}

      {review && (
        <div className="mt-2">
          {review.findings.length === 0 ? (
            <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <CheckCircle2 className="w-3.5 h-3.5 text-green-400" /> No findings.
            </div>
          ) : (
            <div className="space-y-1.5">
              {review.findings.map((f, i) => (
                <div key={i} className="flex items-start gap-1.5 text-xs bg-background/40 rounded p-1.5">
                  {SEVERITY_ICON[f.severity]}
                  <div className="min-w-0">
                    <span className="font-mono text-[10px] text-muted-foreground">{f.file}</span>
                    <div>{f.description}</div>
                  </div>
                </div>
              ))}
            </div>
          )}

          <button
            onClick={() => setExpanded((v) => !v)}
            className="text-[10px] text-muted-foreground hover:text-foreground mt-2"
          >
            {expanded ? "Hide raw output" : "Show raw output"}
          </button>
          {expanded && (
            <pre className="whitespace-pre-wrap text-[10px] font-mono bg-background/40 rounded p-2 mt-1 max-h-60 overflow-y-auto">
              {review.raw_output}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}
