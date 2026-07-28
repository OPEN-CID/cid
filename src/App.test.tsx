import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
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
  },
}));

vi.mock("./hooks/useCid", () => ({
  useCid: vi.fn(),
}));

describe("App", () => {
  beforeEach(() => {
    vi.mocked(api.connect).mockReset();
    vi.mocked(api.connect).mockImplementation(() => new Promise(() => {})); // never resolves — offline mode
    vi.mocked(useCid).mockReturnValue({
      setConnected: vi.fn(),
      selectedRepoId: "repo-1",
      selectedMissionId: null,
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

  it("the LeftRail settings event switches to the models tab", async () => {
    render(<App />);
    await screen.findByText("EditorPaneStub");

    fireEvent(window, new CustomEvent("cid:open-settings"));

    expect(await screen.findByText("ProvidersPanelStub")).toBeInTheDocument();
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

  it("shows the keyboard shortcuts reference on '?'", async () => {
    render(<App />);
    await screen.findByText("EditorPaneStub");

    fireEvent.keyDown(window, { key: "?" });

    expect(await screen.findByText("Keyboard shortcuts")).toBeInTheDocument();
  });
});
