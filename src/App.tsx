import { Suspense, lazy, useEffect, useMemo, useState } from "react";
import { LeftRail } from "./components/layout/LeftRail";
import { ChatThread } from "./components/chat/ChatThread";
import { DiffViewer } from "./components/diff/DiffViewer";
import { HistoryPanel } from "./components/history/HistoryPanel";
import { McpPanel } from "./components/mcp/McpPanel";
import { SkillsPanel } from "./components/skills/SkillsPanel";
import { AcpPanel } from "./components/acp/AcpPanel";
import { ProvidersPanel } from "./components/settings/ProvidersPanel";
import { ConnectionBanner, HealthDashboard, AccessControlPanel } from "./components/WebShell";
import { RepoHealthPanel } from "./components/health/RepoHealthPanel";
import { AutonomyPanel } from "./components/autonomy/AutonomyPanel";
import { DecisionsPanel } from "./components/chat/DecisionsPanel";
import { useCid } from "./hooks/useCid";
import { useTheme } from "./theme/useTheme";
import { api, type ModelInfo } from "./lib/api";
import { toast } from "./lib/dialog";
import { useFocusTrap } from "./lib/useFocusTrap";
import { t } from "./lib/i18n";
import { DialogHost } from "./components/ui/DialogHost";
import { CommandPalette, type Command } from "./components/ui/CommandPalette";
import { Plus, GripVertical, Maximize2, Minimize2 } from "lucide-react";

// review_prompt.md §7 follow-up (Gemini checklist item #5): monaco-editor and
// xterm are already split into separate chunks by vite.config.ts's
// manualChunks, but a static import here pulled both into the eagerly-loaded
// entry bundle regardless — every user paid for both on first load even if
// they never opened the Terminal or Editor tab. React.lazy defers the actual
// fetch until the tab housing it is first rendered.
const EditorPane = lazy(() => import("./components/editor/EditorPane").then((m) => ({ default: m.EditorPane })));
const TerminalPane = lazy(() => import("./components/terminal/TerminalPane").then((m) => ({ default: m.TerminalPane })));

function TabPaneFallback() {
  return <div className="h-full flex items-center justify-center text-sm text-muted-foreground">Loading…</div>;
}

type RightTab =
  | "editor"
  | "terminal"
  | "diff"
  | "history"
  | "decisions"
  | "mcp"
  | "skills"
  | "acp"
  | "models"
  | "autonomy"
  | "health"
  | "server";

function MissionCreationModal({ onClose, onCreated }: { onClose: () => void; onCreated: () => void }) {
  const modalRef = useFocusTrap<HTMLDivElement>(true, onClose);
  const { selectedRepoId, loadMissions } = useCid();
  const [title, setTitle] = useState("");
  const [task, setTask] = useState("");
  const [sessionMode, setSessionMode] = useState<"worktree" | "shared">("worktree");
  const [autonomy, setAutonomy] = useState<"manual" | "co_pilot" | "autonomous">("co_pilot");
  const [vibe, setVibe] = useState(false);
  const [isCreating, setIsCreating] = useState(false);
  const [models, setModels] = useState<ModelInfo[]>([]);
  // Value is "" for "use default" or `${provider}::${id}` — a select can only
  // carry one string, and provider+id must travel together to mission.create.
  const [modelChoice, setModelChoice] = useState("");

  useEffect(() => {
    let cancelled = false;
    api.model
      .list()
      .then((list) => {
        if (!cancelled) setModels(Array.isArray(list) ? list : []);
      })
      .catch(() => {
        // Model picking is a convenience, not a prerequisite — mission
        // creation must still work with just the "use default" option.
        if (!cancelled) setModels([]);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const selectedModel = models.find((m) => `${m.provider}::${m.id}` === modelChoice);

  const handleCreate = async () => {
    if (!selectedRepoId || !title.trim()) return;
    setIsCreating(true);
    try {
      await api.mission.create({
        repo_channel_id: selectedRepoId,
        title: title.trim(),
        task: task.trim() || undefined,
        session_mode: sessionMode,
        autonomy_level: autonomy,
        vibe,
        model_provider: selectedModel?.provider ?? null,
        model_id: selectedModel?.id ?? null,
      });
      await loadMissions(selectedRepoId);
      onCreated();
      onClose();
    } catch (e) {
      toast.error(`Failed to create mission: ${e}`);
    } finally {
      setIsCreating(false);
    }
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div
        ref={modalRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="new-mission-title"
        tabIndex={-1}
        className="bg-card border rounded-lg p-6 w-[480px] max-w-[90vw]"
      >
        <h2 id="new-mission-title" className="font-semibold mb-4">
          {t().mission.newMission}
        </h2>
        <div className="space-y-3">
          <div>
            <label className="text-xs text-muted-foreground">Title</label>
            <input
              className="w-full mt-1 bg-background border rounded px-3 py-2 text-sm"
              placeholder="e.g., Build OAuth, Fix #245"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
            />
          </div>
          <div>
            <label className="text-xs text-muted-foreground">Task Description (optional)</label>
            <textarea
              className="w-full mt-1 bg-background border rounded px-3 py-2 text-sm min-h-[80px]"
              placeholder="Describe what you want CID to do..."
              value={task}
              onChange={(e) => setTask(e.target.value)}
            />
            <div className="text-[11px] text-muted-foreground mt-1">
              Leave blank to have the agent start from the title alone.
            </div>
          </div>
          <div>
            <label className="text-xs text-muted-foreground">Model</label>
            <select
              className="w-full mt-1 bg-background border rounded px-2 py-2 text-sm"
              value={modelChoice}
              onChange={(e) => setModelChoice(e.target.value)}
            >
              <option value="">Use default (from Settings)</option>
              {models.map((m) => (
                <option key={`${m.provider}::${m.id}`} value={`${m.provider}::${m.id}`} disabled={!m.available}>
                  {m.name} ({m.provider}){m.default ? " — default" : ""}
                  {!m.available ? " — no API key configured" : ""}
                </option>
              ))}
            </select>
            {selectedModel && (
              <div className="text-[11px] text-muted-foreground mt-1">
                {selectedModel.context_length.toLocaleString()} token context
              </div>
            )}
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="text-xs text-muted-foreground">Session Mode</label>
              <select
                className="w-full mt-1 bg-background border rounded px-2 py-2 text-sm"
                value={sessionMode}
                onChange={(e) => setSessionMode(e.target.value as "worktree" | "shared")}
              >
                <option value="worktree">Isolated worktree (default)</option>
                <option value="shared">Shared clone</option>
              </select>
              <div className="text-[11px] text-muted-foreground mt-1">
                {sessionMode === "worktree" ? "Creates a dedicated branch + worktree, safe for parallel work" : "Works directly in main repo, solo sequential work"}
              </div>
            </div>
            <div>
              <label className="text-xs text-muted-foreground">Autonomy Level</label>
              <select
                className="w-full mt-1 bg-background border rounded px-2 py-2 text-sm"
                value={autonomy}
                onChange={(e) => setAutonomy(e.target.value as "manual" | "co_pilot" | "autonomous")}
              >
                <option value="manual">Manual</option>
                <option value="co_pilot">Co-Pilot (default)</option>
                <option value="autonomous">Autonomous</option>
              </select>
              <div className="text-[11px] text-muted-foreground mt-1">
                {autonomy === "manual"
                  ? "You drive; the agent assists only when asked"
                  : autonomy === "co_pilot"
                  ? "Every tool call is shown and requires approval"
                  : "Runs the approved plan without per-step approval, inside the repo's command allow-list"}
              </div>
            </div>
          </div>
          <label className="flex items-start gap-2 text-xs cursor-pointer pt-1">
            <input
              type="checkbox"
              className="mt-0.5"
              checked={vibe}
              onChange={(e) => setVibe(e.target.checked)}
            />
            <span>
              <span className="text-foreground font-medium">Vibe mode</span>{" "}
              <span className="text-muted-foreground">
                — skip the Planner step for a quick, low-stakes change. The plan is auto-approved
                and the Implementer starts immediately; tool-call approval, diffs, and History are
                unaffected.
              </span>
            </span>
          </label>
        </div>
        <div className="flex justify-end gap-2 mt-6">
          <button onClick={onClose} className="px-4 py-2 text-sm bg-secondary rounded">
            Cancel
          </button>
          <button
            onClick={handleCreate}
            disabled={isCreating || !title.trim()}
            className="px-4 py-2 text-sm bg-primary text-primary-foreground rounded disabled:opacity-50"
          >
            {isCreating ? "Creating..." : "Create Mission"}
          </button>
        </div>
      </div>
    </div>
  );
}

function BottomStatus() {
  const { connected, selectedMissionId, missions } = useCid();
  const mission = missions.find((m) => m.id === selectedMissionId);
  const [activeModel, setActiveModel] = useState<string | null>(null);

  useEffect(() => {
    if (!connected) return;
    api.settings
      .get()
      .then((s) => {
        const provider = s?.implementer_provider || "anthropic";
        const model = s?.implementer_model || s?.anthropic_model || "unset";
        setActiveModel(`${model} (${provider})`);
      })
      .catch(() => setActiveModel(null));
  }, [connected]);

  return (
    <div className="h-7 border-t bg-card flex items-center px-3 text-[11px] text-muted-foreground gap-4">
      <div className="flex items-center gap-1.5">
        <span className={`w-2 h-2 rounded-full ${connected ? "bg-green-500" : "bg-yellow-500"}`} />
        <span>Core: {connected ? "connected" : "offline"} (ws://127.0.0.1:5919)</span>
      </div>
      {mission && (
        <>
          <span>•</span>
          <span>
            Autonomy: <span className="text-foreground">{mission.autonomy_level}</span>
          </span>
          <span>•</span>
          <span>
            Model: <span className="text-foreground">{activeModel ?? "…"}</span>
          </span>
          <span>•</span>
          <span>
            Session: <span className="text-foreground">{mission.session_mode}</span>
          </span>
        </>
      )}
      <span className="ml-auto">Tauri v2 • Rust core • SQLite • git2-rs • portable-pty • MCP 2026-07-28 • ACP host</span>
    </div>
  );
}

const RIGHT_PANEL_WIDTH_KEY = "cid-right-panel-width";
const MIN_RIGHT_PANEL_WIDTH = 320;
const MAX_RIGHT_PANEL_WIDTH = 1200;

export default function App() {
  const { setConnected, selectedRepoId, selectedMissionId, repos, missions } = useCid();
  const [rightTab, setRightTab] = useState<RightTab>("editor");
  const [showNewMission, setShowNewMission] = useState(false);
  // 051-Editor-Excellence-Roadmap.md Wave 4.6: the fixed 520px right panel
  // was the real constraint on the editor being usable at all — no drag
  // resize meant no way to see more than a sliver of a wide file.
  const [rightPanelWidth, setRightPanelWidth] = useState<number>(() => {
    const saved = typeof localStorage !== "undefined" ? localStorage.getItem(RIGHT_PANEL_WIDTH_KEY) : null;
    const parsed = saved ? parseInt(saved, 10) : NaN;
    return Number.isFinite(parsed) ? Math.min(MAX_RIGHT_PANEL_WIDTH, Math.max(MIN_RIGHT_PANEL_WIDTH, parsed)) : 520;
  });
  const [isResizing, setIsResizing] = useState(false);
  const [maximized, setMaximized] = useState(false);

  // LeftRail's settings/Skills/MCP Servers rows have no reach into this
  // component's tab state otherwise — a lightweight DOM event rather than
  // prop-drilling or a new store just for a handful of buttons. Generalized
  // from a settings-only `cid:open-settings` event so every LeftRail row that
  // wants to jump to a right-panel tab can reuse it.
  useEffect(() => {
    const handler = (e: Event) => {
      const tab = (e as CustomEvent<RightTab>).detail;
      if (!tab) return;
      setMaximized(false);
      setRightTab(tab);
    };
    window.addEventListener("cid:open-tab", handler);
    return () => window.removeEventListener("cid:open-tab", handler);
  }, []);

  useEffect(() => {
    if (!isResizing) return;
    const onMove = (e: MouseEvent) => {
      const next = window.innerWidth - e.clientX;
      setRightPanelWidth(Math.min(MAX_RIGHT_PANEL_WIDTH, Math.max(MIN_RIGHT_PANEL_WIDTH, next)));
    };
    const onUp = () => setIsResizing(false);
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, [isResizing]);

  useEffect(() => {
    localStorage.setItem(RIGHT_PANEL_WIDTH_KEY, String(rightPanelWidth));
  }, [rightPanelWidth]);

  useEffect(() => {
    api
      .connect()
      .then(() => {
        setConnected(true);
        // review_prompt.md §7: reconcile with the user's saved theme once
        // Core is actually reachable — index.html's inline script already
        // applied a same-device default before first paint, this just picks
        // up a choice made on a different device.
        useTheme.getState().syncFromSettings();
      })
      .catch(() => {
        // Not a "mock mode" — nothing is stubbed out and no placeholder data
        // is served. The UI simply has no data until Core is reachable, and
        // the interval below keeps retrying.
        console.warn("[CID] Core not reachable — retrying every 3s");
        setConnected(false);
        // Retry periodically
        const interval = setInterval(() => {
          api
            .connect()
            .then(() => {
              setConnected(true);
              clearInterval(interval);
            })
            .catch(() => {});
        }, 3000);
        return () => clearInterval(interval);
      });
  }, [setConnected]);

  const [showShortcuts, setShowShortcuts] = useState(false);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      const typing = target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable);
      if (e.key === "?" && !typing) {
        e.preventDefault();
        setShowShortcuts(true);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  const commands = useMemo<Command[]>(() => {
    const tabLabels: Record<RightTab, string> = {
      editor: "Editor",
      terminal: "Terminal",
      diff: "Diff",
      history: "History",
      decisions: "Decisions",
      mcp: "MCP",
      skills: "Skills",
      acp: "ACP",
      models: "Models",
      autonomy: "Autonomy",
      health: "Health",
      server: "Server",
    };
    const tabCommands: Command[] = (Object.keys(tabLabels) as RightTab[]).map((tab) => ({
      id: `tab-${tab}`,
      label: `Go to ${tabLabels[tab]}`,
      hint: "tab",
      keywords: tab,
      action: () => {
        setMaximized(false);
        setRightTab(tab);
      },
    }));
    return [
      { id: "new-mission", label: "New Mission…", hint: "N", action: () => setShowNewMission(true) },
      {
        id: "toggle-theme",
        label: "Toggle light/dark theme",
        action: () => useTheme.getState().toggleTheme(),
      },
      {
        id: "toggle-maximize",
        label: maximized ? "Restore panel" : "Maximize right panel",
        action: () => setMaximized((v) => !v),
      },
      { id: "shortcuts", label: "Show keyboard shortcuts", hint: "?", action: () => setShowShortcuts(true) },
      ...tabCommands,
    ];
  }, [maximized]);

  return (
    <div className="h-screen flex flex-col bg-background text-foreground">
      <ConnectionBanner />
      <div className="flex-1 flex min-h-0">
        {/* Left rail */}
        <LeftRail />

        {/* Center - Chat thread */}
        {!maximized && (
          <div className="flex-1 flex flex-col min-w-0 border-r">
            {/* Center header */}
            <div className="h-10 border-b flex items-center px-3 gap-2 bg-card">
              {selectedRepoId ? (
                <>
                  <span className="text-sm font-medium">
                    {repos.find((r) => r.id === selectedRepoId)?.name ?? "Mission Thread"}
                  </span>
                  <span className="text-xs text-muted-foreground">
                    •{" "}
                    {selectedMissionId
                      ? missions.find((m) => m.id === selectedMissionId)?.title ?? selectedMissionId.slice(0, 8)
                      : "no mission selected"}
                  </span>
                  <button
                    onClick={() => setShowNewMission(true)}
                    className="ml-auto flex items-center gap-1 text-xs bg-primary text-primary-foreground px-2 py-1 rounded"
                  >
                    <Plus className="w-3 h-3" /> New Mission
                  </button>
                </>
              ) : (
                <span className="text-sm text-muted-foreground">Select a repo channel to start</span>
              )}
            </div>

            <ChatThread />
          </div>
        )}

        {/* Drag handle */}
        {!maximized && (
          <div
            onMouseDown={() => setIsResizing(true)}
            className="w-1 shrink-0 cursor-col-resize hover:bg-primary/50 active:bg-primary/70 flex items-center justify-center group"
            role="separator"
            aria-orientation="vertical"
            aria-label="Resize right panel"
          >
            <GripVertical className="w-3 h-3 text-muted-foreground opacity-0 group-hover:opacity-100 -ml-1" />
          </div>
        )}

        {/* Right panel - tabbed */}
        <div
          className={`flex flex-col bg-card ${maximized ? "flex-1" : ""} ${isResizing ? "select-none" : ""}`}
          style={maximized ? undefined : { width: rightPanelWidth }}
        >
          <div className="h-10 border-b flex items-center gap-1 px-2 overflow-x-auto">
            {(["editor", "terminal", "diff", "history", "decisions", "mcp", "skills", "acp", "models", "autonomy", "health", "server"] as RightTab[]).map((tab) => (
              <button
                key={tab}
                onClick={() => setRightTab(tab)}
                className={`text-xs px-2.5 py-1 rounded capitalize whitespace-nowrap ${rightTab === tab ? "bg-accent text-accent-foreground" : "text-muted-foreground hover:text-foreground hover:bg-accent/50"}`}
              >
                {tab}
              </button>
            ))}
            <button
              onClick={() => setMaximized((v) => !v)}
              className="ml-auto p-1 rounded text-muted-foreground hover:text-foreground hover:bg-accent shrink-0"
              title={maximized ? "Restore" : "Maximize"}
              aria-label={maximized ? "Restore panel" : "Maximize panel"}
            >
              {maximized ? <Minimize2 className="w-3.5 h-3.5" /> : <Maximize2 className="w-3.5 h-3.5" />}
            </button>
          </div>

          <div className="flex-1 min-h-0 overflow-hidden">
            <Suspense fallback={<TabPaneFallback />}>
              {rightTab === "editor" && <EditorPane />}
              {rightTab === "terminal" && <TerminalPane />}
              {rightTab === "diff" && <DiffViewer />}
              {rightTab === "history" && <HistoryPanel />}
              {rightTab === "decisions" && <DecisionsPanel />}
              {rightTab === "mcp" && <McpPanel />}
              {rightTab === "skills" && <SkillsPanel />}
              {rightTab === "acp" && <AcpPanel />}
              {rightTab === "models" && <ProvidersPanel />}
              {rightTab === "autonomy" && <AutonomyPanel />}
              {rightTab === "health" && <RepoHealthPanel />}
              {rightTab === "server" && (
                <div className="h-full overflow-y-auto">
                  <HealthDashboard />
                  <AccessControlPanel />
                </div>
              )}
            </Suspense>
          </div>
        </div>
      </div>

      <BottomStatus />

      {showNewMission && <MissionCreationModal onClose={() => setShowNewMission(false)} onCreated={() => {}} />}
      {showShortcuts && <KeyboardShortcutsModal onClose={() => setShowShortcuts(false)} />}
      <CommandPalette commands={commands} />
      <DialogHost />
    </div>
  );
}

const SHORTCUTS: { keys: string; description: string }[] = [
  { keys: "Ctrl/Cmd + K", description: "Open the command palette" },
  { keys: "Ctrl/Cmd + S", description: "Save the active file (Editor tab)" },
  { keys: "Escape", description: "Close the open dialog" },
  { keys: "?", description: "Show this shortcuts reference" },
];

function KeyboardShortcutsModal({ onClose }: { onClose: () => void }) {
  const ref = useFocusTrap<HTMLDivElement>(true, onClose);
  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-[120]">
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-labelledby="shortcuts-title"
        tabIndex={-1}
        className="bg-card border rounded-lg p-6 w-[420px] max-w-[90vw]"
      >
        <div className="flex items-center justify-between mb-3">
          <h2 id="shortcuts-title" className="font-semibold">
            {t().shortcuts.title}
          </h2>
          <button onClick={onClose} aria-label={t().common.close} className="text-muted-foreground hover:text-foreground">
            ✕
          </button>
        </div>
        <div className="space-y-1.5 text-sm">
          {SHORTCUTS.map((s) => (
            <div key={s.keys} className="flex items-center justify-between gap-3">
              <span className="text-muted-foreground">{s.description}</span>
              <kbd className="text-[11px] border rounded px-1.5 py-0.5 bg-background whitespace-nowrap">{s.keys}</kbd>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
