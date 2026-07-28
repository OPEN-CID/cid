import { useCallback, useEffect, useState } from "react";
import { api, type RpcValue } from "../../lib/api";
import { RefreshCw, Save } from "lucide-react";
import { TeamIntegrationsPanel } from "./TeamIntegrationsPanel";

// Every value here is treated as string-ish with no narrowing throughout this
// file (rendered straight into <input value>) — genuinely the RpcValue case,
// not a fixed shape worth guessing at (051-Editor-Excellence-Roadmap.md Wave
// 2.1 scope boundary).
type Settings = Record<string, RpcValue>;

type LocalRuntime = {
  id?: string;
  name: string;
  runtime_type?: string;
  available: boolean;
  models: { id: string; name?: string }[];
  version?: string | null;
};

const ROLES = [
  { key: "planner", label: "Planner", hint: "Produces the plan you approve — worth a frontier model" },
  { key: "implementer", label: "Implementer", hint: "Executes the approved plan — a cheaper model is usually fine" },
  { key: "reviewer", label: "Reviewer", hint: "Second pass over the diff before you see it" },
];

const PROVIDERS = [
  { value: "", label: "(default)" },
  { value: "anthropic", label: "Anthropic" },
  { value: "openai", label: "OpenAI" },
  { value: "google", label: "Google" },
  { value: "openai_compatible", label: "OpenAI-compatible endpoint" },
  { value: "local", label: "Local runtime" },
];

/** Secrets come back redacted (`sk-…abcd`); only send a key back if the user typed a new one. */
function isRedacted(value: string | null | undefined): boolean {
  return !!value && value.includes("...");
}

export function ProvidersPanel() {
  const [settings, setSettings] = useState<Settings>({});
  const [runtimes, setRuntimes] = useState<LocalRuntime[]>([]);
  const [dirty, setDirty] = useState<Settings>({});
  const [status, setStatus] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const [s, r] = await Promise.all([api.settings.get(), api.localRuntime.list()]);
      setSettings(s ?? {});
      setRuntimes(r ?? []);
      setDirty({});
    } catch (e) {
      setStatus(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const rescanRuntimes = async () => {
    setLoading(true);
    try {
      setRuntimes((await api.localRuntime.detect(true)) ?? []);
      setStatus("Local runtimes rescanned");
    } catch (e) {
      setStatus(String(e));
    } finally {
      setLoading(false);
    }
  };

  const set = (key: string, value: string) => setDirty((d) => ({ ...d, [key]: value }));
  const valueOf = (key: string) => (key in dirty ? dirty[key] : settings[key] ?? "");

  const save = async () => {
    setStatus(null);
    try {
      const payload = { ...settings, ...dirty };
      for (const key of Object.keys(payload)) {
        if (key.endsWith("_api_key") && isRedacted(payload[key]) && !(key in dirty)) {
          delete payload[key];
        }
      }
      await api.settings.update(payload);
      await load();
      setStatus("Saved");
    } catch (e) {
      setStatus(String(e));
    }
  };

  const keyField = (key: string, label: string, placeholder: string) => (
    <div key={key}>
      <label className="text-[11px] text-muted-foreground">{label}</label>
      <input
        type="password"
        className="w-full mt-1 bg-background border rounded px-2 py-1.5 text-xs"
        placeholder={isRedacted(settings[key]) ? settings[key] : placeholder}
        value={dirty[key] ?? ""}
        onChange={(e) => set(key, e.target.value)}
      />
    </div>
  );

  const availableRuntimes = runtimes.filter((r) => r.available);

  return (
    <div className="h-full overflow-y-auto p-3 text-sm">
      <div className="flex items-center justify-between mb-3">
        <div>
          <div className="font-medium">Models &amp; Routing</div>
          <div className="text-[11px] text-muted-foreground">
            Providers, per-role model selection, and detected local runtimes
          </div>
        </div>
        <button onClick={save} className="text-xs flex items-center gap-1 px-2 py-1 rounded bg-primary text-primary-foreground">
          <Save className="w-3 h-3" /> Save
        </button>
      </div>

      {status && (
        <div className="mb-3 p-2 rounded border bg-background text-[11px] text-muted-foreground">{status}</div>
      )}

      <div className="mb-4">
        <div className="text-xs font-medium mb-1.5">Provider credentials</div>
        <div className="space-y-2">
          {keyField("anthropic_api_key", "Anthropic API key", "sk-ant-…")}
          {keyField("openai_api_key", "OpenAI API key", "sk-…")}
          {keyField("google_api_key", "Google API key", "AIza…")}
          <div>
            <label className="text-[11px] text-muted-foreground">OpenAI-compatible endpoint</label>
            <input
              className="w-full mt-1 bg-background border rounded px-2 py-1.5 text-xs"
              placeholder="https://openrouter.ai/api/v1"
              value={valueOf("openai_compatible_endpoint")}
              onChange={(e) => set("openai_compatible_endpoint", e.target.value)}
            />
            <div className="text-[10px] text-muted-foreground mt-1">
              Covers OpenRouter, Groq, vLLM, and most self-hosted setups through one slot.
            </div>
          </div>
          {keyField("openai_compatible_api_key", "OpenAI-compatible API key", "…")}
          {keyField("github_token", "GitHub token", "ghp_…")}
        </div>
      </div>

      <div className="mb-4">
        <div className="text-xs font-medium mb-1.5">Per-role routing</div>
        <div className="space-y-2">
          {ROLES.map((role) => (
            <div key={role.key} className="p-2 border rounded bg-background">
              <div className="text-xs font-medium">{role.label}</div>
              <div className="text-[10px] text-muted-foreground mb-1.5">{role.hint}</div>
              <div className="grid grid-cols-2 gap-2">
                <select
                  className="bg-background border rounded px-2 py-1 text-xs"
                  value={valueOf(`${role.key}_provider`)}
                  onChange={(e) => set(`${role.key}_provider`, e.target.value)}
                >
                  {PROVIDERS.map((p) => (
                    <option key={p.value} value={p.value}>
                      {p.label}
                    </option>
                  ))}
                </select>
                <input
                  className="bg-background border rounded px-2 py-1 text-xs"
                  placeholder="model id"
                  value={valueOf(`${role.key}_model`)}
                  onChange={(e) => set(`${role.key}_model`, e.target.value)}
                />
              </div>
            </div>
          ))}
        </div>
      </div>

      <div>
        <div className="flex items-center justify-between mb-1.5">
          <div className="text-xs font-medium">Local runtimes</div>
          <button
            onClick={rescanRuntimes}
            disabled={loading}
            className="text-[11px] flex items-center gap-1 px-2 py-0.5 rounded bg-secondary disabled:opacity-50"
          >
            <RefreshCw className={`w-3 h-3 ${loading ? "animate-spin" : ""}`} /> Rescan
          </button>
        </div>
        <div className="space-y-1.5">
          {runtimes.map((r) => (
            <div key={r.name} className="p-2 border rounded bg-background">
              <div className="flex items-center gap-2">
                <span className={`w-2 h-2 rounded-full ${r.available ? "bg-green-500" : "bg-muted-foreground/40"}`} />
                <span className="text-xs font-medium">{r.name}</span>
                {r.version && <span className="text-[10px] text-muted-foreground">{r.version}</span>}
                <span className="ml-auto text-[10px] text-muted-foreground">{r.models?.length ?? 0} models</span>
              </div>
              {r.available && r.models?.length > 0 && (
                <div className="text-[10px] text-muted-foreground mt-1 truncate">
                  {r.models.slice(0, 6).map((m) => m.id).join(", ")}
                  {r.models.length > 6 ? " …" : ""}
                </div>
              )}
            </div>
          ))}
          {availableRuntimes.length === 0 && (
            <div className="text-[11px] text-muted-foreground">
              No local runtime detected. Install Ollama or LM Studio, or start <code>llama.cpp --server</code>.
            </div>
          )}
        </div>
      </div>

      <TeamIntegrationsPanel />
    </div>
  );
}
