import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { AgentsPanel } from "./AgentsPanel";
import { api } from "@/lib/api";
import { useCid } from "@/hooks/useCid";

// Agents surface had RPCs and zero UI — including subagent get/cancel calls
// that sent the wrong field names and always failed. This pins the panel and
// the corrected call shapes.

vi.mock("@/lib/api", () => ({
  api: {
    session: { create: vi.fn(), setModel: vi.fn() },
    subagent: { list: vi.fn(), cancel: vi.fn() },
    backgroundModel: { listTasks: vi.fn() },
    localRuntime: { status: vi.fn() },
  },
}));

vi.mock("@/hooks/useCid", () => ({
  useCid: vi.fn(),
}));

const state = {
  sessions: [{ id: "session-1", repo_channel_id: "repo-1" }],
  repos: [{ id: "repo-1", path: "/tmp/repo", name: "repo" }],
  selectedSessionId: "session-1",
  selectSession: vi.fn(),
  loadSessions: vi.fn(),
};

describe("AgentsPanel", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.mocked(api.subagent.list).mockReset().mockResolvedValue([]);
    vi.mocked(api.subagent.cancel).mockReset();
    vi.mocked(api.backgroundModel.listTasks).mockReset().mockResolvedValue([]);
    vi.mocked(api.session.create).mockReset();
    vi.mocked(api.session.setModel).mockReset();
    vi.mocked(api.localRuntime.status).mockReset().mockResolvedValue({ running: false });
    vi.mocked(state.selectSession).mockReset();
    vi.mocked(state.loadSessions).mockReset().mockResolvedValue(undefined);
    vi.mocked(useCid).mockReturnValue(state as any);
  });

  it("prompts to select a session when none is selected", () => {
    vi.mocked(useCid).mockReturnValue({ sessions: [], repos: [], selectedSessionId: null } as any);
    render(<AgentsPanel />);
    expect(screen.getByText(/Select a session to see its agents/)).toBeInTheDocument();
  });

  it("lists subagents with role, model, and status", async () => {
    vi.mocked(api.subagent.list).mockResolvedValueOnce([
      { id: "sa-1", session_id: "session-1", role: "TestRunner", prompt: "run tests", status: "running", model_provider: "google", model_id: "gemini-4-argon", result: null },
    ]);
    render(<AgentsPanel />);

    expect(await screen.findByText("TestRunner")).toBeInTheDocument();
    expect(screen.getByText(/google gemini-4-argon/)).toBeInTheDocument();
    expect(api.subagent.list).toHaveBeenCalledWith("session-1");
  });

  it("cancelling an active subagent calls cancel with the id", async () => {
    vi.mocked(api.subagent.list).mockResolvedValueOnce([
      { id: "sa-1", session_id: "session-1", role: "TestRunner", prompt: "run", status: "running", result: null },
    ]);
    render(<AgentsPanel />);
    fireEvent.click(await screen.findByText("Cancel"));

    await waitFor(() => expect(api.subagent.cancel).toHaveBeenCalledWith("sa-1"));
  });

  it("one click opens an isolated manual sandbox and pins a running local model", async () => {
    vi.mocked(api.session.create).mockResolvedValueOnce({ id: "sandbox-1" });
    vi.mocked(api.localRuntime.status).mockResolvedValueOnce({
      running: true,
      installed_models: ["qwen2.5-coder:7b"],
    });
    render(<AgentsPanel />);
    await screen.findByText(/No subagents on this session/);

    fireEvent.click(screen.getByText("New isolated sandbox"));

    await waitFor(() =>
      expect(api.session.create).toHaveBeenCalledWith(
        expect.objectContaining({ isolation_mode: "worktree", autonomy_level: "manual" })
      )
    );
    await waitFor(() =>
      expect(api.session.setModel).toHaveBeenCalledWith("sandbox-1", "ollama", "qwen2.5-coder:7b")
    );
    expect(state.selectSession).toHaveBeenCalledWith("sandbox-1");
  });

  it("sandbox without a local runtime still opens on the default model", async () => {
    vi.mocked(api.session.create).mockResolvedValueOnce({ id: "sandbox-2" });
    render(<AgentsPanel />);
    await screen.findByText(/No subagents on this session/);

    fireEvent.click(screen.getByText("New isolated sandbox"));

    await waitFor(() => expect(api.session.create).toHaveBeenCalled());
    expect(api.session.setModel).not.toHaveBeenCalled();
    expect(state.selectSession).toHaveBeenCalledWith("sandbox-2");
  });
});
