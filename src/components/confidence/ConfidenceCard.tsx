import { useState } from "react";
import { api } from "../../lib/api";
import { toast } from "../../lib/dialog";
import { ChevronDown, ChevronUp, RefreshCw, Gauge, History } from "lucide-react";

type SignalResult = {
  signal: string;
  score: number;
  explanation: string;
  details?: unknown;
};

type ConfidenceScore = {
  patch_id: string;
  overall: number;
  signals: SignalResult[];
  generated_at: string;
  explanation: string;
};

const SIGNAL_LABELS: Record<string, string> = {
  symbol_resolution: "Symbol Resolution",
  static_analysis: "Static Analysis",
  type_validation: "Type Validation",
  architecture_validation: "Architecture Validation",
  test_impact: "Test Impact",
  duplicate_detection: "Duplicate Detection",
  dependency_impact: "Dependency Impact",
  semantic_similarity: "Semantic Similarity",
  existing_implementation_reuse: "Existing Implementation Reuse",
};

function bandColor(score: number): string {
  if (score >= 0.85) return "text-green-400";
  if (score >= 0.6) return "text-yellow-400";
  if (score >= 0.35) return "text-orange-400";
  return "text-red-400";
}

function barColor(score: number): string {
  if (score >= 0.85) return "bg-green-500";
  if (score >= 0.6) return "bg-yellow-500";
  if (score >= 0.35) return "bg-orange-500";
  return "bg-red-500";
}

/**
 * Renders each of the nine signals individually rather than collapsing them
 * into one opaque number — Part A's explicit requirement, because a wrong
 * "high confidence" is worse than no score at all.
 */
export function ConfidenceCard({ missionId, filePath }: { missionId: string; filePath: string }) {
  const [card, setCard] = useState<ConfidenceScore | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [history, setHistory] = useState<ConfidenceScore[] | null>(null);

  const run = async () => {
    setLoading(true);
    setError(null);
    try {
      const result = await api.confidence.score(missionId, filePath);
      setCard(result);
      setExpanded(true);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  // 051-Editor-Excellence-Roadmap.md Wave 5.1f: confidence.history had a
  // real, tested backend and no caller anywhere. Scores are stored per
  // Mission, not per file — labelled honestly rather than implying a
  // per-file filter the data doesn't support.
  const loadHistory = async () => {
    try {
      const scores: ConfidenceScore[] = await api.confidence.history(missionId);
      setHistory(scores || []);
    } catch (e) {
      toast.error(`Failed to load confidence history: ${e}`);
    }
  };

  if (!card) {
    return (
      <button
        onClick={run}
        disabled={loading}
        className="text-[11px] flex items-center gap-1 px-1.5 py-0.5 rounded bg-blue-600/20 text-blue-300 hover:bg-blue-600/30 disabled:opacity-50"
      >
        <Gauge className="w-3 h-3" />
        {loading ? "Scoring…" : "Score confidence"}
        {error && <span className="text-red-400 ml-1">error</span>}
      </button>
    );
  }

  return (
    <div className="mt-2 border rounded bg-background/60 overflow-hidden">
      <div className="w-full flex items-center gap-2 px-2 py-1.5">
        <button
          onClick={() => setExpanded((e) => !e)}
          className="flex-1 min-w-0 flex items-center gap-2 text-left"
          aria-expanded={expanded}
        >
          <Gauge className="w-3.5 h-3.5 shrink-0" />
          <span className={`text-sm font-semibold shrink-0 ${bandColor(card.overall)}`}>
            {Math.round(card.overall * 100)}/100
          </span>
          <span className="text-[11px] text-muted-foreground truncate">
            {card.explanation.split("\n")[0]}
          </span>
        </button>
        <button onClick={loadHistory} className="p-1 text-muted-foreground shrink-0" aria-label="Show confidence history">
          <History className="w-3 h-3" />
        </button>
        <button onClick={run} className="p-1 text-muted-foreground shrink-0" aria-label="Re-score">
          <RefreshCw className={`w-3 h-3 ${loading ? "animate-spin" : ""}`} />
        </button>
        <button onClick={() => setExpanded((e) => !e)} className="p-1 shrink-0" aria-label={expanded ? "Collapse" : "Expand"}>
          {expanded ? <ChevronUp className="w-3.5 h-3.5" /> : <ChevronDown className="w-3.5 h-3.5" />}
        </button>
      </div>

      {expanded && history && (
        <div className="border-t px-2 py-2 space-y-1">
          <div className="text-[10px] text-muted-foreground">Recent scores for this Mission (all files):</div>
          {history.length === 0 && <div className="text-[11px] text-muted-foreground">No prior scores recorded.</div>}
          {history.map((h) => (
            <div key={h.patch_id} className="flex items-center gap-2 text-[11px]">
              <span className={`font-semibold ${bandColor(h.overall)}`}>{Math.round(h.overall * 100)}</span>
              <span className="text-muted-foreground">{new Date(h.generated_at).toLocaleString()}</span>
            </div>
          ))}
        </div>
      )}

      {expanded && (
        <div className="border-t px-2 py-2 space-y-1.5">
          {card.signals.map((s) => (
            <div key={s.signal} className="text-[11px]">
              <div className="flex items-center gap-2">
                <span className="w-40 shrink-0 text-muted-foreground">
                  {SIGNAL_LABELS[s.signal] ?? s.signal}
                </span>
                <div className="flex-1 h-1.5 rounded bg-muted overflow-hidden">
                  <div
                    className={`h-full ${barColor(s.score)}`}
                    style={{ width: `${Math.round(s.score * 100)}%` }}
                  />
                </div>
                <span className={`w-9 text-right shrink-0 ${bandColor(s.score)}`}>
                  {Math.round(s.score * 100)}
                </span>
              </div>
              <div className="text-[10px] text-muted-foreground ml-[168px] mt-0.5">
                {s.explanation}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
