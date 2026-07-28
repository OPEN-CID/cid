import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/api";
import { toast } from "@/lib/dialog";
import { useCid } from "@/hooks/useCid";
import { FileText, Rocket, ExternalLink } from "lucide-react";

type AdrSummary = { number: string; title: string; path: string; status?: string | null };
type DeploymentRecord = {
  id: string;
  mission_id: string;
  environment: string;
  commit_or_tag: string;
  ci_run_url?: string | null;
  note?: string | null;
  source: "manual" | "ci_webhook";
  deployed_at: string;
};

// 051-Editor-Excellence-Roadmap.md Wave 5.1c: decisions.* (ADRs relevant to
// this Mission) and deployment.* (what/when/where — never an action CID
// performs, per the founding non-goal) had real backends and no surface —
// a Mission-thread tab, since both are inherently per-mission.
export function DecisionsPanel() {
  const { missions, repos, selectedMissionId } = useCid();
  const mission = missions.find((m) => m.id === selectedMissionId);
  const repo = repos.find((r) => r.id === mission?.repo_channel_id);

  const [relevantAdrs, setRelevantAdrs] = useState<AdrSummary[]>([]);
  const [allAdrs, setAllAdrs] = useState<AdrSummary[] | null>(null);
  const [deployments, setDeployments] = useState<DeploymentRecord[]>([]);
  const [loading, setLoading] = useState(false);
  const [showRecordForm, setShowRecordForm] = useState(false);
  const [form, setForm] = useState({ environment: "", commit_or_tag: "", ci_run_url: "", note: "" });

  const load = useCallback(async () => {
    if (!selectedMissionId) return;
    setLoading(true);
    try {
      const [adrs, deploys] = await Promise.all([
        api.decisions.forMission(selectedMissionId),
        api.deployment.list(selectedMissionId),
      ]);
      setRelevantAdrs(adrs || []);
      setDeployments(deploys || []);
    } catch (e) {
      toast.error(`Failed to load decisions: ${e}`);
    } finally {
      setLoading(false);
    }
  }, [selectedMissionId]);

  useEffect(() => {
    load();
    setAllAdrs(null);
  }, [load]);

  const loadAllAdrs = async () => {
    if (!repo) return;
    try {
      const adrs = await api.decisions.list(repo.path);
      setAllAdrs(adrs || []);
    } catch (e) {
      toast.error(`Failed to list repo ADRs: ${e}`);
    }
  };

  const submitDeployment = async () => {
    if (!selectedMissionId || !form.environment.trim() || !form.commit_or_tag.trim()) return;
    try {
      await api.deployment.record({
        mission_id: selectedMissionId,
        environment: form.environment.trim(),
        commit_or_tag: form.commit_or_tag.trim(),
        ci_run_url: form.ci_run_url.trim() || undefined,
        note: form.note.trim() || undefined,
      });
      setForm({ environment: "", commit_or_tag: "", ci_run_url: "", note: "" });
      setShowRecordForm(false);
      await load();
    } catch (e) {
      toast.error(`Failed to record deployment: ${e}`);
    }
  };

  if (!selectedMissionId) {
    return <div className="p-4 text-xs text-muted-foreground">Select a mission to see its decisions and deployments.</div>;
  }

  return (
    <div className="p-4 space-y-6 overflow-y-auto h-full text-xs">
      <div>
        <div className="flex items-center gap-1 font-medium mb-2">
          <FileText className="w-3.5 h-3.5" /> Relevant ADRs
        </div>
        {loading && <div className="text-muted-foreground">Loading…</div>}
        {!loading && relevantAdrs.length === 0 && (
          <div className="text-muted-foreground">No ADRs explicitly referenced by this Mission&apos;s task or plan.</div>
        )}
        <div className="space-y-1">
          {relevantAdrs.map((a) => (
            <div key={a.path} className="p-2 border rounded bg-background">
              <div className="font-medium">
                ADR {a.number}: {a.title}
              </div>
              <div className="text-[10px] text-muted-foreground font-mono truncate">{a.path}</div>
            </div>
          ))}
        </div>

        {allAdrs === null ? (
          <button onClick={loadAllAdrs} className="mt-2 px-2 py-1 rounded bg-secondary">
            Show all repo ADRs
          </button>
        ) : (
          <div className="mt-2 space-y-1">
            <div className="text-[10px] text-muted-foreground">All decision records in this repo:</div>
            {allAdrs.map((a) => (
              <div key={a.path} className="p-2 border rounded bg-background">
                <div className="font-medium">
                  ADR {a.number}: {a.title}
                </div>
                {a.status && <div className="text-[10px] text-muted-foreground">{a.status}</div>}
              </div>
            ))}
            {allAdrs.length === 0 && <div className="text-muted-foreground">No ADRs found in docs/adr/.</div>}
          </div>
        )}
      </div>

      <div>
        <div className="flex items-center justify-between mb-2">
          <div className="flex items-center gap-1 font-medium">
            <Rocket className="w-3.5 h-3.5" /> Deployments
          </div>
          <button onClick={() => setShowRecordForm((v) => !v)} className="px-2 py-0.5 rounded bg-primary text-primary-foreground">
            Record
          </button>
        </div>
        <div className="text-[10px] text-muted-foreground mb-2">
          A log of what was deployed, when, and where — CID never performs the deployment itself.
        </div>

        {showRecordForm && (
          <div className="p-2 border rounded bg-background space-y-1.5 mb-2">
            <input
              className="w-full bg-background border rounded px-2 py-1 text-[11px]"
              placeholder="Environment (e.g., production, staging)"
              value={form.environment}
              onChange={(e) => setForm({ ...form, environment: e.target.value })}
            />
            <input
              className="w-full bg-background border rounded px-2 py-1 text-[11px] font-mono"
              placeholder="Commit SHA or tag"
              value={form.commit_or_tag}
              onChange={(e) => setForm({ ...form, commit_or_tag: e.target.value })}
            />
            <input
              className="w-full bg-background border rounded px-2 py-1 text-[11px]"
              placeholder="CI run URL (optional)"
              value={form.ci_run_url}
              onChange={(e) => setForm({ ...form, ci_run_url: e.target.value })}
            />
            <input
              className="w-full bg-background border rounded px-2 py-1 text-[11px]"
              placeholder="Note (optional)"
              value={form.note}
              onChange={(e) => setForm({ ...form, note: e.target.value })}
            />
            <button
              onClick={submitDeployment}
              disabled={!form.environment.trim() || !form.commit_or_tag.trim()}
              className="px-3 py-1 rounded bg-primary text-primary-foreground disabled:opacity-50"
            >
              Save
            </button>
          </div>
        )}

        <div className="space-y-1">
          {deployments.map((d) => (
            <div key={d.id} className="p-2 border rounded bg-background">
              <div className="flex items-center gap-1.5">
                <span className="font-medium">{d.environment}</span>
                <span className="text-[10px] bg-accent px-1 rounded">{d.source === "ci_webhook" ? "CI" : "manual"}</span>
                <span className="text-[10px] text-muted-foreground ml-auto">{new Date(d.deployed_at).toLocaleString()}</span>
              </div>
              <div className="font-mono text-[10px]">{d.commit_or_tag}</div>
              {d.note && <div className="text-[10px] text-muted-foreground">{d.note}</div>}
              {d.ci_run_url && (
                <a href={d.ci_run_url} target="_blank" rel="noreferrer" className="text-[10px] text-primary flex items-center gap-0.5">
                  <ExternalLink className="w-2.5 h-2.5" /> CI run
                </a>
              )}
            </div>
          ))}
          {!loading && deployments.length === 0 && <div className="text-muted-foreground">No deployments recorded for this Mission.</div>}
        </div>
      </div>
    </div>
  );
}
