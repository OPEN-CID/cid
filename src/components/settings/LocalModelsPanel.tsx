import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/api";
import { toast } from "@/lib/dialog";
import { Loader2, Play, Square, Download, Check, RefreshCw } from "lucide-react";

type GpuInfo = { name: string; vram_mb: number | null };
type SystemCapability = {
  os: string;
  arch: string;
  cpu_cores: number;
  total_ram_mb: number;
  available_ram_mb: number;
  gpus: GpuInfo[];
  total_vram_mb: number | null;
};

type Fit = "comfortable" | "tight" | "too_large";
type RecommendedModel = {
  id: string;
  name: string;
  parameters: string;
  download_mb: number;
  min_memory_mb: number;
  context_tokens: number;
  notes: string;
  fit: Fit;
  recommended: boolean;
};

type Ownership = "managed" | "external" | "not_running";
type RuntimeStatus = {
  installed: boolean;
  binary_path: string | null;
  running: boolean;
  ownership: Ownership;
  endpoint: string;
  installed_models: string[];
  install_url: string;
};

function gb(mb: number): string {
  return `${(mb / 1024).toFixed(mb >= 10240 ? 0 : 1)} GB`;
}

const FIT_LABEL: Record<Fit, string> = {
  comfortable: "Runs well",
  tight: "Tight fit",
  too_large: "Not enough memory",
};

const FIT_CLASS: Record<Fit, string> = {
  comfortable: "bg-green-500/20 text-green-400",
  tight: "bg-amber-500/20 text-amber-400",
  too_large: "bg-muted text-muted-foreground",
};

export function LocalModelsPanel() {
  const [system, setSystem] = useState<SystemCapability | null>(null);
  const [models, setModels] = useState<RecommendedModel[]>([]);
  const [status, setStatus] = useState<RuntimeStatus | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [pulling, setPulling] = useState<string | null>(null);
  const [progress, setProgress] = useState<string>("");

  const refresh = useCallback(async (force = false) => {
    try {
      const [rec, st] = await Promise.all([
        api.localRuntime.recommended(force) as Promise<{ system: SystemCapability; models: RecommendedModel[] }>,
        api.localRuntime.status() as Promise<RuntimeStatus>,
      ]);
      setSystem(rec?.system ?? null);
      setModels(rec?.models ?? []);
      setStatus(st ?? null);
    } catch (e) {
      toast.error(`Could not read local model state: ${e}`);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // The runtime's own progress lines, streamed while a pull runs.
  useEffect(() => {
    const unsub = api.onNotification((n) => {
      if (n.method === "local.model.pull.progress") setProgress(n.params?.line ?? "");
    });
    return () => unsub();
  }, []);

  const startStop = async (action: "start" | "stop") => {
    setBusy(action);
    try {
      const next = (await api.localRuntime[action]()) as RuntimeStatus;
      setStatus(next);
      await refresh();
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(null);
    }
  };

  const pull = async (model: RecommendedModel) => {
    setPulling(model.id);
    setProgress("starting…");
    try {
      await api.localRuntime.pull(model.id);
      toast.success(`${model.name} is ready to use`);
      await refresh();
    } catch (e) {
      toast.error(`Download failed: ${e}`);
    } finally {
      setPulling(null);
      setProgress("");
    }
  };

  const isInstalled = (id: string) =>
    // Ollama reports `qwen2.5-coder:7b`; a tag pulled without one comes back
    // as `name:latest`, so compare on the base name too.
    (status?.installed_models ?? []).some((m) => m === id || m.split(":")[0] === id.split(":")[0]);

  return (
    <div className="h-full overflow-y-auto p-4 space-y-4 text-sm">
      <div>
        <h3 className="font-semibold">Local models</h3>
        <p className="text-xs text-muted-foreground mt-0.5">
          Run a model on this machine. Nothing leaves it, and no API key is needed.
        </p>
      </div>

      {/* What this machine can do — stated up front, because it decides
          everything below it. */}
      <div className="border rounded p-3 space-y-1">
        <div className="flex items-center justify-between">
          <span className="text-xs font-medium">This machine</span>
          <button
            onClick={() => refresh(true)}
            className="text-[11px] text-muted-foreground hover:text-foreground flex items-center gap-1"
          >
            <RefreshCw className="w-3 h-3" /> Rescan
          </button>
        </div>
        {system ? (
          <div className="text-[11px] text-muted-foreground grid grid-cols-2 gap-x-4 gap-y-0.5">
            <span>
              {system.cpu_cores} cores · {system.os}/{system.arch}
            </span>
            <span>
              RAM {gb(system.total_ram_mb)} ({gb(system.available_ram_mb)} free)
            </span>
            <span className="col-span-2">
              GPU:{" "}
              {system.gpus.length === 0
                ? "none detected"
                : system.gpus
                    .map((g) => `${g.name}${g.vram_mb ? ` (${gb(g.vram_mb)})` : ""}`)
                    .join(", ")}
            </span>
          </div>
        ) : (
          <div className="text-[11px] text-muted-foreground">Reading system capability…</div>
        )}
      </div>

      {/* Runtime state and the start/stop control. */}
      <div className="border rounded p-3 space-y-2">
        <div className="flex items-center gap-2">
          <span className="text-xs font-medium">Ollama</span>
          <span
            className={`text-[10px] px-1 rounded ${
              status?.running ? "bg-green-500/20 text-green-400" : "bg-muted text-muted-foreground"
            }`}
          >
            {status?.running ? "running" : status?.installed ? "stopped" : "not installed"}
          </span>
          {status?.ownership === "external" && (
            <span className="text-[10px] px-1 rounded bg-blue-500/20 text-blue-400">
              started outside CID
            </span>
          )}
          <span className="ml-auto flex items-center gap-2">
            {status?.installed && !status.running && (
              <button
                onClick={() => startStop("start")}
                disabled={busy !== null}
                className="flex items-center gap-1 text-xs bg-primary text-primary-foreground px-2 py-1 rounded disabled:opacity-50"
              >
                {busy === "start" ? <Loader2 className="w-3 h-3 animate-spin" /> : <Play className="w-3 h-3" />}
                Start
              </button>
            )}
            {status?.running && (
              <button
                onClick={() => startStop("stop")}
                disabled={busy !== null || status.ownership === "external"}
                title={
                  status.ownership === "external"
                    ? "This server was not started by CID, so CID will not stop it."
                    : undefined
                }
                className="flex items-center gap-1 text-xs bg-secondary px-2 py-1 rounded disabled:opacity-50"
              >
                {busy === "stop" ? <Loader2 className="w-3 h-3 animate-spin" /> : <Square className="w-3 h-3" />}
                Stop
              </button>
            )}
          </span>
        </div>

        {!status?.installed && (
          <p className="text-[11px] text-muted-foreground">
            CID does not install software on your machine. Install Ollama from{" "}
            <a
              href={status?.install_url ?? "https://ollama.com/download"}
              target="_blank"
              rel="noreferrer"
              className="underline"
            >
              ollama.com/download
            </a>
            , then come back and press Start.
          </p>
        )}
        {status?.running && (
          <p className="text-[11px] text-muted-foreground">
            Serving on {status.endpoint} — pick a downloaded model when you create a Session.
          </p>
        )}
      </div>

      {/* The catalogue, sized to the machine above. */}
      <div className="space-y-2">
        <div className="text-xs font-medium">Models for this machine</div>
        {models.map((m) => {
          const installed = isInstalled(m.id);
          const canPull = status?.running && m.fit !== "too_large";
          return (
            <div key={m.id} className="border rounded p-2.5 space-y-1">
              <div className="flex items-center gap-2">
                <span className="text-xs font-medium">{m.name}</span>
                <span className="text-[10px] text-muted-foreground">{m.parameters}</span>
                <span className={`text-[10px] px-1 rounded ${FIT_CLASS[m.fit]}`}>{FIT_LABEL[m.fit]}</span>
                {m.recommended && (
                  <span className="text-[10px] px-1 rounded bg-primary/20 text-primary">recommended</span>
                )}
                <span className="ml-auto">
                  {installed ? (
                    <span className="flex items-center gap-1 text-[11px] text-green-400">
                      <Check className="w-3 h-3" /> Downloaded
                    </span>
                  ) : (
                    <button
                      onClick={() => pull(m)}
                      disabled={!canPull || pulling !== null}
                      title={
                        m.fit === "too_large"
                          ? "This machine does not have enough memory for this model."
                          : !status?.running
                          ? "Start Ollama first."
                          : undefined
                      }
                      className="flex items-center gap-1 text-xs bg-secondary px-2 py-1 rounded disabled:opacity-50"
                    >
                      {pulling === m.id ? (
                        <Loader2 className="w-3 h-3 animate-spin" />
                      ) : (
                        <Download className="w-3 h-3" />
                      )}
                      {gb(m.download_mb)}
                    </button>
                  )}
                </span>
              </div>
              <div className="text-[11px] text-muted-foreground">
                {m.notes} · needs {gb(m.min_memory_mb)} · {m.context_tokens.toLocaleString()} tok context
              </div>
              {pulling === m.id && progress && (
                <div className="text-[10px] text-muted-foreground truncate font-mono">{progress}</div>
              )}
            </div>
          );
        })}
        {models.length === 0 && (
          <div className="text-[11px] text-muted-foreground">No model recommendations yet.</div>
        )}
      </div>
    </div>
  );
}
