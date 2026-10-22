import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/api";
import { toast } from "@/lib/dialog";
import { useCid } from "@/hooks/useCid";
import { storedSessionToken } from "../auth/AuthDialog";

type Policy = {
  autonomous_enabled: boolean;
  autonomous_repos: string[];
  session_spend_cap_usd?: number | null;
  daily_spend_cap_usd?: number | null;
  min_role_for_autonomous?: string;
  min_role_for_plan_approval?: string;
  min_role_for_merge?: string;
};

// Workspace governance that previously had no UI at all: enabling Autonomous
// mode, scoping it to repos, and setting spend caps all require an admin
// session token (`session_token`), which no surface ever supplied — so these
// levers were effectively unreachable from any client. Reads are open;
// writes attach the signed-in session, and the server refuses
// non-admins, which is surfaced rather than hidden.
export function GovernancePanel() {
  const { repos } = useCid();
  const [policy, setPolicy] = useState<Policy | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [sessionCap, setSessionCap] = useState("");
  const [dailyCap, setDailyCap] = useState("");

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const p = await api.governance.getPolicy();
      setPolicy(p);
      setSessionCap(p?.session_spend_cap_usd != null ? String(p.session_spend_cap_usd) : "");
      setDailyCap(p?.daily_spend_cap_usd != null ? String(p.daily_spend_cap_usd) : "");
    } catch (e) {
      toast.error(`Failed to load governance policy: ${e}`);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const save = async (next: Partial<Policy>) => {
    if (!policy) return;
    const token = storedSessionToken();
    if (!token) {
      toast.error("Sign in as an admin first (status bar → user icon) — changing policy requires it");
      return;
    }
    setSaving(true);
    try {
      const { workspace_id: _ws, ...rest } = policy as Policy & { workspace_id?: string };
      const saved = await api.governance.setPolicy(token, { ...rest, ...next });
      setPolicy(saved);
      toast.success("Governance policy updated");
    } catch (e) {
      toast.error(`Failed to update policy: ${e}`);
    } finally {
      setSaving(false);
    }
  };

  const parseCap = (raw: string): number | null | undefined => {
    const t = raw.trim();
    if (t === "") return null;
    const n = Number(t);
    return Number.isFinite(n) && n >= 0 ? n : undefined;
  };

  const saveCaps = () => {
    const s = parseCap(sessionCap);
    const d = parseCap(dailyCap);
    if (s === undefined || d === undefined) {
      toast.error("Spend caps must be non-negative numbers, or empty for unlimited");
      return;
    }
    save({ session_spend_cap_usd: s, daily_spend_cap_usd: d });
  };

  if (loading) return <div className="text-muted-foreground">Loading policy…</div>;
  if (!policy) return <div className="text-muted-foreground">No governance policy found.</div>;

  return (
    <div className="space-y-3">
      <div className="font-medium">Workspace governance</div>
      <div className="text-[11px] text-muted-foreground">
        Changes require an admin sign-in. Role bars below are enforced server-side at the real
        decision points (merge, spend, autonomous enable), not just displayed here.
      </div>

      <label className="flex items-center gap-2 p-2 border rounded bg-background">
        <input
          type="checkbox"
          checked={policy.autonomous_enabled}
          disabled={saving}
          onChange={(e) => save({ autonomous_enabled: e.target.checked })}
        />
        <span>
          Autonomous mode permitted
          <span className="block text-[11px] text-muted-foreground">
            Master switch. Individual repos still opt in below; sessions still need an allowed role.
          </span>
        </span>
      </label>

      <div className="p-2 border rounded bg-background space-y-1.5">
        <div className="font-medium">Autonomous repos</div>
        {repos.length === 0 && <div className="text-muted-foreground">No repos connected.</div>}
        {repos.map((r) => {
          const allowed = policy.autonomous_repos.includes(r.path) || policy.autonomous_repos.includes(r.id);
          return (
            <label key={r.id} className="flex items-center gap-2 text-[11px]">
              <input
                type="checkbox"
                checked={allowed}
                disabled={saving || !policy.autonomous_enabled}
                onChange={(e) => {
                  const next = e.target.checked
                    ? [...policy.autonomous_repos, r.path]
                    : policy.autonomous_repos.filter((p) => p !== r.path && p !== r.id);
                  save({ autonomous_repos: next });
                }}
              />
              <span className="truncate">{r.name}</span>
            </label>
          );
        })}
      </div>

      <div className="p-2 border rounded bg-background space-y-1.5">
        <div className="font-medium">Spend caps (USD, empty = unlimited)</div>
        <div className="flex gap-1.5">
          <input
            className="flex-1 bg-background border rounded px-2 py-1 text-[11px]"
            placeholder="Per session, e.g. 5"
            value={sessionCap}
            onChange={(e) => setSessionCap(e.target.value)}
          />
          <input
            className="flex-1 bg-background border rounded px-2 py-1 text-[11px]"
            placeholder="Daily workspace, e.g. 50"
            value={dailyCap}
            onChange={(e) => setDailyCap(e.target.value)}
          />
          <button onClick={saveCaps} disabled={saving} className="px-2 py-1 rounded bg-primary text-primary-foreground disabled:opacity-50">
            Save
          </button>
        </div>
      </div>

      <div className="text-[11px] text-muted-foreground space-y-0.5">
        <div>Autonomous needs role: {policy.min_role_for_autonomous ?? "—"}</div>
        <div>Plan approval needs: {policy.min_role_for_plan_approval ?? "—"}</div>
        <div>Merging / PRs need: {policy.min_role_for_merge ?? "—"}</div>
      </div>
    </div>
  );
}
