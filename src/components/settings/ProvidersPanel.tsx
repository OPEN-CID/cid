import { useCallback, useEffect, useState } from "react";
import { api, type ModelInfo, type RpcValue } from "../../lib/api";
import { RefreshCw, Save } from "lucide-react";
import { TeamIntegrationsPanel } from "./TeamIntegrationsPanel";
import { LocalModelsPanel } from "./LocalModelsPanel";

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

function CloudProvidersPanel() {
  const [settings, setSettings] = useState<Settings>({});
  const [runtimes, setRuntimes] = useState<LocalRuntime[]>([]);
  const [models, setModels] = useState<ModelInfo[]>([]);
  // Per-role override to show the free-text id input instead of the <select>
  // — undefined defers to whether the stored value actually matches a known
  // model, so a role someone already pointed at a custom id opens in custom
  // mode without extra clicks.
  const [customRole, setCustomRole] = useState<Record<string, boolean>>({});
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
    try {
      setModels((await api.model.list()) ?? []);
    } catch {
      // The per-role picker degrades to the plain text input below —
      // model.list() is a convenience, not a prerequisite for routing.
      setModels([]);
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
          {ROLES.map((role) => {
            const provider = valueOf(`${role.key}_provider`);
            const filteredModels = provider ? models.filter((m) => m.provider === provider) : models;
            const modelValue = valueOf(`${role.key}_model`);
            const matchesKnownModel = filteredModels.some((m) => m.id === modelValue);
            const isCustom = customRole[role.key] ?? (modelValue !== "" && !matchesKnownModel);
            const showSelect = filteredModels.length > 0 && !isCustom;
            return (
              <div key={role.key} className="p-2 border rounded bg-background">
                <div className="text-xs font-medium">{role.label}</div>
                <div className="text-[10px] text-muted-foreground mb-1.5">{role.hint}</div>
                <div className="grid grid-cols-2 gap-2">
                  <select
                    className="bg-background border rounded px-2 py-1 text-xs"
                    value={provider}
                    onChange={(e) => set(`${role.key}_provider`, e.target.value)}
                  >
                    {PROVIDERS.map((p) => (
                      <option key={p.value} value={p.value}>
                        {p.label}
                      </option>
                    ))}
                  </select>
                  {showSelect ? (
                    <select
                      className="bg-background border rounded px-2 py-1 text-xs"
                      value={modelValue}
                      onChange={(e) => set(`${role.key}_model`, e.target.value)}
                    >
                      <option value="">(default)</option>
                      {filteredModels.map((m) => (
                        <option key={m.id} value={m.id} disabled={!m.available}>
                          {m.name} — {m.context_length.toLocaleString()} tok
                          {m.default ? " (default)" : ""}
                          {!m.available ? " (no API key)" : ""}
                        </option>
                      ))}
                    </select>
                  ) : (
                    <input
                      className="bg-background border rounded px-2 py-1 text-xs"
                      placeholder="model id"
                      value={modelValue}
                      onChange={(e) => set(`${role.key}_model`, e.target.value)}
                    />
                  )}
                </div>
                {filteredModels.length > 0 && (
                  <button
                    type="button"
                    onClick={() => setCustomRole((c) => ({ ...c, [role.key]: !isCustom }))}
                    className="text-[10px] text-muted-foreground underline mt-1"
                  >
                    {isCustom ? "Choose from list" : "custom model id…"}
                  </button>
                )}
              </div>
            );
          })}
        </div>
      </div>

      {/* Detected runtimes stay visible here as a one-line summary; setting
          them up lives on the Local tab, which has the machine's capability and
          the start/stop controls. */}
      <div className="text-[11px] text-muted-foreground">
        {availableRuntimes.length > 0
          ? `${availableRuntimes.length} local runtime${availableRuntimes.length === 1 ? "" : "s"} detected — see the Local tab.`
          : "No local runtime running. See the Local tab to set one up."}{" "}
        <button onClick={rescanRuntimes} disabled={loading} className="underline disabled:opacity-50">
          <RefreshCw className={`w-3 h-3 inline ${loading ? "animate-spin" : ""}`} /> Rescan
        </button>
      </div>

      <TeamIntegrationsPanel />
    </div>
  );
}

/// Cloud providers and local models are different jobs — one is API keys and
/// per-role routing, the other is hardware, downloads and a server process.
/// They were one scrolling page, which buried both.
export function ProvidersPanel() {
  const [tab, setTab] = useState<"cloud" | "local">("cloud");

  return (
    <div className="h-full flex flex-col">
      <div className="h-9 border-b flex items-center gap-1 px-2 shrink-0" role="tablist">
        {(["cloud", "local"] as const).map((t) => (
          <button
            key={t}
            role="tab"
            aria-selected={tab === t}
            onClick={() => setTab(t)}
            className={`text-xs px-2.5 py-1 rounded ${
              tab === t ? "bg-accent text-accent-foreground" : "text-muted-foreground hover:text-foreground hover:bg-accent/50"
            }`}
          >
            {t === "cloud" ? "Cloud providers" : "Local models"}
          </button>
        ))}
      </div>
      <div className="flex-1 min-h-0">
        {tab === "cloud" ? <CloudProvidersPanel /> : <LocalModelsPanel />}
      </div>
    </div>
  );
}
