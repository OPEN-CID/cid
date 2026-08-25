import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, act } from "@testing-library/react";
import App from "./App";
import { api } from "./lib/api";
import { useCid } from "./hooks/useCid";

// 051-Editor-Excellence-Roadmap.md Wave 5.4: a smoke test for the root shell
// itself, given how much of this session's work landed here (resize,
// command palette, the settings DOM-event bridge, tab switching). Every
// panel is stubbed — this is App's own wiring under test, not each panel's.

vi.mock("./components/layout/LeftRail", () => ({ LeftRail: () => <div>LeftRailStub</div> }));
vi.mock("./components/chat/ChatThread", () => ({ ChatThread: () => <div>ChatThreadStub</div> }));
vi.mock("./components/diff/DiffViewer", () => ({ DiffViewer: () => <div>DiffViewerStub</div> }));
vi.mock("./components/history/HistoryPanel", () => ({ HistoryPanel: () => <div>HistoryPanelStub</div> }));
vi.mock("./components/mcp/McpPanel", () => ({ McpPanel: () => <div>McpPanelStub</div> }));
vi.mock("./components/skills/SkillsPanel", () => ({ SkillsPanel: () => <div>SkillsPanelStub</div> }));
vi.mock("./components/acp/AcpPanel", () => ({ AcpPanel: () => <div>AcpPanelStub</div> }));
vi.mock("./components/settings/ProvidersPanel", () => ({ ProvidersPanel: () => <div>ProvidersPanelStub</div> }));
vi.mock("./components/WebShell", () => ({
  ConnectionBanner: () => <div>ConnectionBannerStub</div>,
  HealthDashboard: () => <div>HealthDashboardStub</div>,
  AccessControlPanel: () => <div>AccessControlPanelStub</div>,
}));
vi.mock("./components/health/RepoHealthPanel", () => ({ RepoHealthPanel: () => <div>RepoHealthPanelStub</div> }));
vi.mock("./components/autonomy/AutonomyPanel", () => ({ AutonomyPanel: () => <div>AutonomyPanelStub</div> }));
vi.mock("./components/chat/DecisionsPanel", () => ({ DecisionsPanel: () => <div>DecisionsPanelStub</div> }));
vi.mock("./components/editor/EditorPane", () => ({ EditorPane: () => <div>EditorPaneStub</div> }));
vi.mock("./components/terminal/TerminalPane", () => ({ TerminalPane: () => <div>TerminalPaneStub</div> }));

vi.mock("./lib/api", () => ({
  api: {
    connect: vi.fn(),
    session: { create: vi.fn() },
    model: { list: vi.fn() },
  },
}));

vi.mock("./hooks/useCid", () => ({
  useCid: vi.fn(),
}));

describe("App", () => {
  beforeEach(() => {
    // Panel visibility and right-panel width persist to localStorage, so
    // without this a test that reveals a panel leaks it into the next one.
    localStorage.clear();
    vi.mocked(api.connect).mockReset();
    vi.mocked(api.connect).mockImplementation(() => new Promise(() => {})); // never resolves — offline mode
    vi.mocked(api.model.list).mockReset().mockResolvedValue([]);
    vi.mocked(api.session.create).mockReset().mockResolvedValue({});
    vi.mocked(useCid).mockReturnValue({
      setConnected: vi.fn(),
      selectedRepoId: "repo-1",
      selectedSessionId: null,
      repos: [{ id: "repo-1", name: "cid" }],
      sessions: [],
      loadSessions: vi.fn(),
    } as any);
  });

  it("renders the editor tab by default", async () => {
    render(<App />);
    expect(await screen.findByText("EditorPaneStub")).toBeInTheDocument();
  });

  /// With no callable model CID cannot run an agent: the Planner records a
  /// placeholder plan and the Implementer stays blocked. That used to be
  /// invisible — worse, the status bar reported `claude-sonnet-5 (anthropic)`
  /// on an install with no Anthropic key, because the display defaulted the
  /// provider and id regardless of whether a key existed.
  it("says so when no model is configured, instead of implying one is active", async () => {
    vi.mocked(useCid).mockReturnValue({
      setConnected: vi.fn(),
      connected: true,
      selectedRepoId: "repo-1",
      selectedSessionId: "session-1",
      repos: [{ id: "repo-1", name: "cid" }],
      sessions: [{ id: "session-1", title: "S", autonomy_level: "co_pilot", isolation_mode: "worktree" }],
      loadSessions: vi.fn(),
    } as any);
    vi.mocked(api.model.list).mockResolvedValue([
      { id: "claude-sonnet-5", provider: "anthropic", available: false, default: true },
    ] as never);

    render(<App />);

    expect(await screen.findByText("No model configured.")).toBeInTheDocument();
    expect(screen.getByText("none configured")).toBeInTheDocument();
    expect(screen.queryByText("claude-sonnet-5")).not.toBeInTheDocument();
  });

  it("shows no banner and names the real model once one is callable", async () => {
    vi.mocked(useCid).mockReturnValue({
      setConnected: vi.fn(),
      connected: true,
      selectedRepoId: "repo-1",
      selectedSessionId: "session-1",
      repos: [{ id: "repo-1", name: "cid" }],
      sessions: [{ id: "session-1", title: "S", autonomy_level: "co_pilot", isolation_mode: "worktree" }],
      loadSessions: vi.fn(),
    } as any);
    vi.mocked(api.model.list).mockResolvedValue([
      { id: "claude-sonnet-5", provider: "anthropic", available: true, default: true },
    ] as never);

    render(<App />);

    expect(await screen.findByText("claude-sonnet-5")).toBeInTheDocument();
    expect(screen.queryByText("No model configured.")).not.toBeInTheDocument();
  });

  it("switching tabs renders the corresponding panel", async () => {
    render(<App />);
    await screen.findByText("EditorPaneStub");

    fireEvent.click(screen.getByText("Diff"));

    expect(await screen.findByText("DiffViewerStub")).toBeInTheDocument();
  });

  it("hidden panels are absent from the tab bar but reachable via the ＋ menu", async () => {
    render(<App />);
    await screen.findByText("EditorPaneStub");

    // Default is the minimal set — History is one of the nine hidden ones.
    expect(screen.queryByText("History")).not.toBeInTheDocument();

    fireEvent.click(screen.getByLabelText("Choose panels"));
    fireEvent.click(screen.getByLabelText("History"));
    // Close the menu so the only remaining "History" is the tab itself.
    fireEvent.keyDown(document, { key: "Escape" });

    expect(screen.getByText("History")).toBeInTheDocument();
  });

  it("the last visible panel cannot be hidden, so the pane is never blank", async () => {
    render(<App />);
    await screen.findByText("EditorPaneStub");

    fireEvent.click(screen.getByLabelText("Choose panels"));
    fireEvent.click(screen.getByLabelText("Terminal"));
    fireEvent.click(screen.getByLabelText("Diff"));

    const editorToggle = screen.getByLabelText("Editor") as HTMLInputElement;
    expect(editorToggle.disabled).toBe(true);
  });

  it("Ctrl+K opens the command palette, and its tab commands switch tabs", async () => {
    render(<App />);
    await screen.findByText("EditorPaneStub");

    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    fireEvent.click(screen.getByText("Go to Terminal"));

    expect(await screen.findByText("TerminalPaneStub")).toBeInTheDocument();
  });

  it("the LeftRail open-tab event switches to the requested tab", async () => {
    render(<App />);
    await screen.findByText("EditorPaneStub");

    fireEvent(window, new CustomEvent("cid:open-tab", { detail: "models" }));

    expect(await screen.findByText("ProvidersPanelStub")).toBeInTheDocument();
  });

  it("shows the selected repo's name and the session title in the center header", async () => {
    vi.mocked(useCid).mockReturnValue({
      setConnected: vi.fn(),
      selectedRepoId: "repo-1",
      selectedSessionId: "session-1",
      repos: [{ id: "repo-1", name: "cid" }],
      sessions: [{ id: "session-1", title: "Fix the thing" }],
      loadSessions: vi.fn(),
    } as any);
    render(<App />);

    expect(await screen.findByText("cid")).toBeInTheDocument();
    expect(screen.getByText(/Fix the thing/)).toBeInTheDocument();
  });

  it("Maximize hides the center thread and expands the right panel", async () => {
    render(<App />);
    await screen.findByText("EditorPaneStub");
    expect(screen.getByText("ChatThreadStub")).toBeInTheDocument();

    fireEvent.click(screen.getByLabelText("Maximize panel"));

    expect(screen.queryByText("ChatThreadStub")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Restore panel")).toBeInTheDocument();
  });

  it("New Session opens the creation modal, and Cancel closes it", async () => {
    render(<App />);
    await screen.findByText("EditorPaneStub");

    fireEvent.click(screen.getByText("New Session"));
    expect(await screen.findByText(/New Session/i, { selector: "h2" })).toBeInTheDocument();

    fireEvent.click(screen.getByText("Cancel"));
    expect(screen.queryByText(/New Session/i, { selector: "h2" })).not.toBeInTheDocument();
  });

  it("Task Description is optional — a title alone is enough to submit", async () => {
    render(<App />);
    await screen.findByText("EditorPaneStub");
    fireEvent.click(screen.getByText("New Session"));
    await screen.findByText(/New Session/i, { selector: "h2" });

    expect(screen.getByText("Task Description (optional)")).toBeInTheDocument();
    const createButton = screen.getByText("Create Session");
    expect(createButton).toBeDisabled();

    fireEvent.change(screen.getByPlaceholderText(/e.g., Build OAuth/), { target: { value: "Fix #245" } });
    expect(createButton).not.toBeDisabled();

    await act(async () => {
      fireEvent.click(createButton);
      await Promise.resolve();
    });

    const payload = vi.mocked(api.session.create).mock.calls[0][0] as { task?: string };
    expect(payload.task).toBeUndefined();
  });

  it("the Model dropdown loads from model.list and passes provider/id through to session.create", async () => {
    // Field names mirror the real `model.list` wire response verified against
    // a running Core (`available`, not `enabled`) — a fixture that invents a
    // field cannot fail when the component reads the wrong one.
    vi.mocked(api.model.list).mockResolvedValueOnce([
      { id: "claude-sonnet-5", name: "Claude Sonnet 5", provider: "anthropic", context_length: 1_000_000, default: true, available: true },
      { id: "gpt-4o", name: "GPT-4o", provider: "openai", context_length: 128_000, default: false, available: false },
    ]);

    render(<App />);
    await screen.findByText("EditorPaneStub");
    fireEvent.click(screen.getByText("New Session"));
    await screen.findByText(/New Session/i, { selector: "h2" });

    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(screen.getByText(/Use default \(from Settings\)/)).toBeInTheDocument();
    const disabledOption = screen.getByText(/GPT-4o/).closest("option") as HTMLOptionElement;
    expect(disabledOption).toBeDisabled();
    // The available model must NOT be disabled — the half this assertion pair
    // was missing, and precisely the symptom of reading a field the wire never
    // sends: every option came back disabled and no model could be picked.
    const enabledOption = screen.getByText(/Claude Sonnet 5/).closest("option") as HTMLOptionElement;
    expect(enabledOption).not.toBeDisabled();

    fireEvent.change(screen.getByPlaceholderText(/e.g., Build OAuth/), { target: { value: "Fix #245" } });
    fireEvent.change(screen.getByDisplayValue(/Use default/), { target: { value: "anthropic::claude-sonnet-5" } });

    expect(screen.getByText(/1,000,000 token context/)).toBeInTheDocument();

    await act(async () => {
      fireEvent.click(screen.getByText("Create Session"));
      await Promise.resolve();
    });

    const payload = vi.mocked(api.session.create).mock.calls[0][0] as { model_provider?: string; model_id?: string };
    expect(payload.model_provider).toBe("anthropic");
    expect(payload.model_id).toBe("claude-sonnet-5");
  });

  it("degrades to just the default option when model.list fails, without blocking session creation", async () => {
    vi.mocked(api.model.list).mockRejectedValueOnce(new Error("offline"));

    render(<App />);
    await screen.findByText("EditorPaneStub");
    fireEvent.click(screen.getByText("New Session"));
    await screen.findByText(/New Session/i, { selector: "h2" });

    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(screen.getByText(/Use default \(from Settings\)/)).toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText(/e.g., Build OAuth/), { target: { value: "Fix #245" } });
    await act(async () => {
      fireEvent.click(screen.getByText("Create Session"));
      await Promise.resolve();
    });

    expect(api.session.create).toHaveBeenCalled();
  });

  it("shows the keyboard shortcuts reference on '?'", async () => {
    render(<App />);
    await screen.findByText("EditorPaneStub");

    fireEvent.keyDown(window, { key: "?" });

    expect(await screen.findByText("Keyboard shortcuts")).toBeInTheDocument();
  });
});
