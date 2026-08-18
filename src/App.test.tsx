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
    mission: { create: vi.fn() },
    model: { list: vi.fn() },
  },
}));

vi.mock("./hooks/useCid", () => ({
  useCid: vi.fn(),
}));

describe("App", () => {
  beforeEach(() => {
    vi.mocked(api.connect).mockReset();
    vi.mocked(api.connect).mockImplementation(() => new Promise(() => {})); // never resolves — offline mode
    vi.mocked(api.model.list).mockReset().mockResolvedValue([]);
    vi.mocked(api.mission.create).mockReset().mockResolvedValue({});
    vi.mocked(useCid).mockReturnValue({
      setConnected: vi.fn(),
      selectedRepoId: "repo-1",
      selectedMissionId: null,
      repos: [{ id: "repo-1", name: "cid" }],
      missions: [],
      loadMissions: vi.fn(),
    } as any);
  });

  it("renders the editor tab by default", async () => {
    render(<App />);
    expect(await screen.findByText("EditorPaneStub")).toBeInTheDocument();
  });

  it("switching tabs renders the corresponding panel", async () => {
    render(<App />);
    await screen.findByText("EditorPaneStub");

    fireEvent.click(screen.getByText("diff"));

    expect(await screen.findByText("DiffViewerStub")).toBeInTheDocument();
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

  it("shows the selected repo's name and the mission title in the center header", async () => {
    vi.mocked(useCid).mockReturnValue({
      setConnected: vi.fn(),
      selectedRepoId: "repo-1",
      selectedMissionId: "mission-1",
      repos: [{ id: "repo-1", name: "cid" }],
      missions: [{ id: "mission-1", title: "Fix the thing" }],
      loadMissions: vi.fn(),
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

  it("New Mission opens the creation modal, and Cancel closes it", async () => {
    render(<App />);
    await screen.findByText("EditorPaneStub");

    fireEvent.click(screen.getByText("New Mission"));
    expect(await screen.findByText(/New Mission/i, { selector: "h2" })).toBeInTheDocument();

    fireEvent.click(screen.getByText("Cancel"));
    expect(screen.queryByText(/New Mission/i, { selector: "h2" })).not.toBeInTheDocument();
  });

  it("Task Description is optional — a title alone is enough to submit", async () => {
    render(<App />);
    await screen.findByText("EditorPaneStub");
    fireEvent.click(screen.getByText("New Mission"));
    await screen.findByText(/New Mission/i, { selector: "h2" });

    expect(screen.getByText("Task Description (optional)")).toBeInTheDocument();
    const createButton = screen.getByText("Create Mission");
    expect(createButton).toBeDisabled();

    fireEvent.change(screen.getByPlaceholderText(/e.g., Build OAuth/), { target: { value: "Fix #245" } });
    expect(createButton).not.toBeDisabled();

    await act(async () => {
      fireEvent.click(createButton);
      await Promise.resolve();
    });

    const payload = vi.mocked(api.mission.create).mock.calls[0][0] as { task?: string };
    expect(payload.task).toBeUndefined();
  });

  it("the Model dropdown loads from model.list and passes provider/id through to mission.create", async () => {
    // Field names mirror the real `model.list` wire response verified against
    // a running Core (`available`, not `enabled`) — a fixture that invents a
    // field cannot fail when the component reads the wrong one.
    vi.mocked(api.model.list).mockResolvedValueOnce([
      { id: "claude-sonnet-5", name: "Claude Sonnet 5", provider: "anthropic", context_length: 1_000_000, default: true, available: true },
      { id: "gpt-4o", name: "GPT-4o", provider: "openai", context_length: 128_000, default: false, available: false },
    ]);

    render(<App />);
    await screen.findByText("EditorPaneStub");
    fireEvent.click(screen.getByText("New Mission"));
    await screen.findByText(/New Mission/i, { selector: "h2" });

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
      fireEvent.click(screen.getByText("Create Mission"));
      await Promise.resolve();
    });

    const payload = vi.mocked(api.mission.create).mock.calls[0][0] as { model_provider?: string; model_id?: string };
    expect(payload.model_provider).toBe("anthropic");
    expect(payload.model_id).toBe("claude-sonnet-5");
  });

  it("degrades to just the default option when model.list fails, without blocking mission creation", async () => {
    vi.mocked(api.model.list).mockRejectedValueOnce(new Error("offline"));

    render(<App />);
    await screen.findByText("EditorPaneStub");
    fireEvent.click(screen.getByText("New Mission"));
    await screen.findByText(/New Mission/i, { selector: "h2" });

    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(screen.getByText(/Use default \(from Settings\)/)).toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText(/e.g., Build OAuth/), { target: { value: "Fix #245" } });
    await act(async () => {
      fireEvent.click(screen.getByText("Create Mission"));
      await Promise.resolve();
    });

    expect(api.mission.create).toHaveBeenCalled();
  });

  it("shows the keyboard shortcuts reference on '?'", async () => {
    render(<App />);
    await screen.findByText("EditorPaneStub");

    fireEvent.keyDown(window, { key: "?" });

    expect(await screen.findByText("Keyboard shortcuts")).toBeInTheDocument();
  });
});
