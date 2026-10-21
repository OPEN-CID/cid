import { useCallback, useEffect, useState } from "react";
import { api, type ModelInfo } from "@/lib/api";
import { toast } from "@/lib/dialog";

// Per-session model visibility and switching, mid-conversation. Creation
// already offers a picker, but nothing after creation showed or changed what
// a Session actually runs on — the override columns existed server-side with
// no surface at all. Changing takes effect on the very next turn; clearing
// restores settings-level resolution.
export function SessionModelBar({ sessionId }: { sessionId: string }) {
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [override, setOverride] = useState<{ provider: string | null; id: string | null }>({
    provider: null,
    id: null,
  });
  const [saving, setSaving] = useState(false);
  const [auto, setAuto] = useState<{ label: string; reason: string } | null>(null);

  const load = useCallback(async () => {
    try {
      const [list, session] = await Promise.all([
        api.model.list(),
        api.session.get(sessionId),
      ]);
      if (Array.isArray(list)) setModels(list);
      const ov = {
        provider: (session?.model_provider as string | null) ?? null,
        id: (session?.model_id as string | null) ?? null,
      };
      setOverride(ov);
      // No explicit override: show what auto-routing currently recommends,
      // so an automatic choice is visible before the turn runs. Best-effort:
      // a manual install simply shows the resolved default instead.
      if (!ov.provider && !ov.id) {
        try {
          const rec = await api.model.routeRecommend({ session_id: sessionId });
          if (rec && rec.mode && rec.mode !== "manual") {
            setAuto({
              label: `Auto (${rec.mode.replace("auto_", "")}): ${rec.provider} ${rec.model_id}`,
              reason: rec.reason ?? "",
            });
          } else {
            setAuto(null);
          }
        } catch {
          setAuto(null);
        }
      } else {
        setAuto(null);
      }
    } catch (e) {
      toast.error(`Failed to load models: ${e}`);
    }
  }, [sessionId]);

  useEffect(() => {
    load();
  }, [load]);

  const available = models.filter((m) => m.available);
  const current =
    override.provider || override.id
      ? `${override.provider ?? "?"} ${override.id ?? "(provider default)"} · session override`
      : auto
        ? `${auto.label}`
        : (available.find((m) => m.default) ? `${available.find((m) => m.default)!.provider} ${available.find((m) => m.default)!.id}` : "none configured");

  const change = async (value: string) => {
    setSaving(true);
    try {
      if (!value) {
        await api.session.setModel(sessionId, null, null);
      } else {
        const [provider, ...rest] = value.split("::");
        await api.session.setModel(sessionId, provider, rest.join("::") || null);
      }
      await load();
      toast.success("Session model updated — applies from the next turn");
    } catch (e) {
      toast.error(`Failed to switch model: ${e}`);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="flex items-center gap-2 px-3 py-1.5 border-b border-border text-[11px] text-muted-foreground">
      <span className="shrink-0" title={auto?.reason || undefined}>
        Model: <span data-testid="current-model" className="text-foreground font-medium">{current}</span>
      </span>
      <select
        aria-label="Switch session model"
        disabled={saving}
        value={override.provider || override.id ? `${override.provider ?? ""}::${override.id ?? ""}` : ""}
        onChange={(e) => change(e.target.value)}
        className="ml-auto bg-background border rounded px-1.5 py-0.5 text-[11px] max-w-[55%]"
      >
        <option value="">Session default</option>
        {available.map((m) => (
          <option key={`${m.provider}::${m.id}`} value={`${m.provider}::${m.id}`}>
            {m.provider} {m.id}
          </option>
        ))}
      </select>
    </div>
  );
}
