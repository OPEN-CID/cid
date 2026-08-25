import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { LeftRail } from "./LeftRail";

vi.mock("@/lib/api", () => ({
  api: {
    repo: { list: vi.fn().mockResolvedValue([]), connect: vi.fn() },
    contextEngine: { status: vi.fn().mockResolvedValue({ enabled: false }) },
  },
}));

vi.mock("./RepoBrowserDialog", () => ({
  RepoBrowserDialog: () => <div>RepoBrowserDialogStub</div>,
}));

let useCidMock = {
  repos: [] as { id: string; name: string; path: string }[],
  selectedRepoId: null as string | null,
  sessions: [] as { id: string; title: string; status: string }[],
  selectedSessionId: null as string | null,
  selectRepo: vi.fn(),
  selectSession: vi.fn(),
  loadRepos: vi.fn(),
  connected: true,
};

vi.mock("@/hooks/useCid", () => ({
  useCid: () => useCidMock,
}));

describe("LeftRail", () => {
  beforeEach(() => {
    useCidMock = {
      repos: [],
      selectedRepoId: null,
      sessions: [],
      selectedSessionId: null,
      selectRepo: vi.fn(),
      selectSession: vi.fn(),
      loadRepos: vi.fn(),
      connected: true,
    };
    delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  afterEach(() => {
    delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  it("renders CID branding and empty state", () => {
    render(<LeftRail />);
    expect(screen.getByText("CID")).toBeInTheDocument();
    expect(screen.getByText(/No repos connected/)).toBeInTheDocument();
  });

  it("shows the Core status dot as connected when useCid().connected is true", () => {
    render(<LeftRail />);
    expect(screen.getByText("Core")).toBeInTheDocument();
    expect(screen.queryByText(/offline/)).not.toBeInTheDocument();
  });

  it("shows the Core status dot as offline when disconnected, instead of always-green", () => {
    useCidMock.connected = false;
    render(<LeftRail />);
    expect(screen.getByText(/Core \(offline\)/)).toBeInTheDocument();
  });

  it("the settings gear dispatches cid:open-tab for the models tab", () => {
    const handler = vi.fn();
    window.addEventListener("cid:open-tab", handler);
    render(<LeftRail />);

    fireEvent.click(screen.getByLabelText("Open settings"));

    expect(handler).toHaveBeenCalledWith(expect.objectContaining({ detail: "models" }));
    window.removeEventListener("cid:open-tab", handler);
  });

  it("the Skills and MCP Servers rows are real controls that dispatch cid:open-tab", () => {
    useCidMock.selectedRepoId = "repo-1";
    useCidMock.repos = [{ id: "repo-1", name: "cid", path: "/repo" }];
    const handler = vi.fn();
    window.addEventListener("cid:open-tab", handler);
    render(<LeftRail />);

    fireEvent.click(screen.getByText("Skills"));
    expect(handler).toHaveBeenCalledWith(expect.objectContaining({ detail: "skills" }));

    fireEvent.click(screen.getByText("MCP Servers"));
    expect(handler).toHaveBeenCalledWith(expect.objectContaining({ detail: "mcp" }));

    window.removeEventListener("cid:open-tab", handler);
  });

  it("the add-repo popover offers Browse and lists already-connected repos under Recents", () => {
    useCidMock.repos = [{ id: "repo-1", name: "cid", path: "C:\\Projects\\cid" }];
    render(<LeftRail />);

    fireEvent.click(screen.getByLabelText("Connect a repo"));

    expect(screen.getByText("Browse…")).toBeInTheDocument();
    expect(screen.getByText("Recents")).toBeInTheDocument();
    expect(screen.queryByText("Browse (native)…")).not.toBeInTheDocument();
  });

  it("clicking Browse opens the RepoBrowserDialog", () => {
    render(<LeftRail />);
    fireEvent.click(screen.getByLabelText("Connect a repo"));

    fireEvent.click(screen.getByText("Browse…"));

    expect(screen.getByText("RepoBrowserDialogStub")).toBeInTheDocument();
  });

  it("clicking a Recents entry reselects that repo without reconnecting", () => {
    useCidMock.repos = [{ id: "repo-1", name: "cid", path: "C:\\Projects\\cid" }];
    render(<LeftRail />);
    fireEvent.click(screen.getByLabelText("Connect a repo"));

    fireEvent.click(screen.getByTitle("C:\\Projects\\cid"));

    expect(useCidMock.selectRepo).toHaveBeenCalledWith("repo-1");
  });

  it("offers a native Browse button only inside the Tauri desktop shell", () => {
    (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    render(<LeftRail />);
    fireEvent.click(screen.getByLabelText("Connect a repo"));

    expect(screen.getByText("Browse (native)…")).toBeInTheDocument();
  });
});
