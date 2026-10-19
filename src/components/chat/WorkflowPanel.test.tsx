import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { WorkflowPanel } from "./WorkflowPanel";
import { api } from "@/lib/api";
import { useCid } from "@/hooks/useCid";

// Workflows v1 had a real backend (11 RPCs) and no surface — this pins the
// new panel: list, create with per-step kinds, start, approve, cancel.

vi.mock("@/lib/api", () => ({
  api: {
    workflow: {
      create: vi.fn(),
      get: vi.fn(),
      list: vi.fn(),
      update: vi.fn(),
      delete: vi.fn(),
      runStart: vi.fn(),
      runGet: vi.fn(),
      runList: vi.fn(),
      runCancel: vi.fn(),
      runApprove: vi.fn(),
      runRetry: vi.fn(),
    },
    session: {
      approveTool: vi.fn(),
      pendingTools: vi.fn(),
    },
  },
}));

vi.mock("@/hooks/useCid", () => ({
  useCid: vi.fn(),
}));

const state = {
  sessions: [{ id: "session-1", repo_channel_id: "repo-1" }],
  repos: [{ id: "repo-1", path: "/tmp/repo" }],
  selectedSessionId: "session-1",
};

const workflow = {
  id: "wf-1",
  name: "Nightly checks",
  description: "",
  steps: [
    { id: "s1", name: "tests", kind: "command", command: "echo hi", on_failure: "stop" },
    { id: "s2", name: "human checkpoint", kind: "manual_gate", on_failure: "stop" },
  ],
};

describe("WorkflowPanel", () => {
  beforeEach(() => {
    localStorage.clear();
    Object.values(api.workflow).forEach((fn: any) => fn.mockReset());
    vi.mocked(api.session.approveTool).mockReset();
    vi.mocked(api.session.pendingTools).mockReset();
    vi.mocked(useCid).mockReturnValue(state as any);
    vi.mocked(api.workflow.list).mockResolvedValue([]);
    vi.mocked(api.workflow.runList).mockResolvedValue([]);
    vi.mocked(api.session.pendingTools).mockResolvedValue([]);
    vi.mocked(api.workflow.runGet).mockReset();
  });

  it("prompts to select a session when none is selected", () => {
    vi.mocked(useCid).mockReturnValue({ sessions: [], repos: [], selectedSessionId: null } as any);
    render(<WorkflowPanel />);
    expect(screen.getByText(/Select a session/)).toBeInTheDocument();
  });

  it("lists workflows for the session's repo", async () => {
    vi.mocked(api.workflow.list).mockResolvedValueOnce([workflow]);
    render(<WorkflowPanel />);

    expect(await screen.findByText("Nightly checks")).toBeInTheDocument();
    expect(api.workflow.list).toHaveBeenCalledWith("repo-1");
  });

  it("creating a workflow sends steps with kinds and loads the list", async () => {
    vi.mocked(api.workflow.list).mockResolvedValueOnce([]).mockResolvedValueOnce([workflow]);
    vi.mocked(api.workflow.create).mockResolvedValueOnce(workflow);
    render(<WorkflowPanel />);
    await waitFor(() => expect(api.workflow.list).toHaveBeenCalled());

    fireEvent.click(screen.getByText("New"));
    fireEvent.change(screen.getByPlaceholderText("Workflow name"), { target: { value: "Nightly checks" } });
    fireEvent.change(screen.getByPlaceholderText("Step name"), { target: { value: "tests" } });
    fireEvent.click(screen.getByText("Save workflow"));

    await waitFor(() =>
      expect(api.workflow.create).toHaveBeenCalledWith(
        expect.objectContaining({
          repo_channel_id: "repo-1",
          name: "Nightly checks",
          steps: expect.arrayContaining([expect.objectContaining({ name: "tests", kind: "agent" })]),
        })
      )
    );
  });

  it("starting a run on the selected workflow calls runStart with the session", async () => {
    vi.mocked(api.workflow.list).mockResolvedValueOnce([workflow]);
    vi.mocked(api.workflow.runList).mockResolvedValue([]);
    vi.mocked(api.workflow.runStart).mockResolvedValueOnce({
      id: "run-1",
      workflow_id: "wf-1",
      session_id: "session-1",
      status: "running",
      current_index: 0,
      steps: [{ step_id: "s1", status: "running" }],
    });
    render(<WorkflowPanel />);
    fireEvent.click(await screen.findByText("Nightly checks"));
    fireEvent.click(screen.getByText("Start on this session"));

    await waitFor(() =>
      expect(api.workflow.runStart).toHaveBeenCalledWith("wf-1", "session-1")
    );
  });

  it("mentions creating workflows from chat", async () => {
    vi.mocked(api.workflow.list).mockResolvedValueOnce([]);
    render(<WorkflowPanel />);
    expect(await screen.findByText(/describe the pipeline in chat/)).toBeInTheDocument();
  });

  it("a command step parked on approval surfaces approve/deny inline", async () => {
    vi.mocked(api.workflow.list).mockResolvedValueOnce([workflow]);
    vi.mocked(api.workflow.runList).mockResolvedValue([]);
    const running = {
      id: "run-1",
      workflow_id: "wf-1",
      session_id: "session-1",
      status: "running",
      current_index: 0,
      steps: [{ step_id: "s1", status: "running" }],
    };
    vi.mocked(api.workflow.runStart).mockResolvedValueOnce(running);
    vi.mocked(api.workflow.runGet).mockResolvedValue(running);
    vi.mocked(api.session.pendingTools).mockResolvedValue([
      { id: "tool-1", session_id: "session-1", name: "run_terminal", arguments: {} },
    ]);
    render(<WorkflowPanel />);
    fireEvent.click(await screen.findByText("Nightly checks"));
    fireEvent.click(screen.getByText("Start on this session"));

    expect(await screen.findByText(/Waiting for your approval/)).toBeInTheDocument();
    fireEvent.click(screen.getByText("Approve"));

    await waitFor(() =>
      expect(api.session.approveTool).toHaveBeenCalledWith("session-1", "tool-1", true)
    );
  });

  it("a waiting run offers approve, and approval sends the note", async () => {
    vi.mocked(api.workflow.list).mockResolvedValueOnce([workflow]);
    vi.mocked(api.workflow.runList).mockResolvedValue([]);
    const waiting = {
      id: "run-1",
      workflow_id: "wf-1",
      session_id: "session-1",
      status: "waiting_approval",
      current_index: 1,
      steps: [
        { step_id: "s1", status: "completed" },
        { step_id: "s2", status: "waiting_approval" },
      ],
    };
    vi.mocked(api.workflow.runStart).mockResolvedValueOnce(waiting);
    vi.mocked(api.workflow.runGet).mockResolvedValue(waiting);
    vi.mocked(api.workflow.runApprove).mockResolvedValueOnce({
      id: "run-1",
      status: "running",
      current_index: 2,
      steps: [],
    });
    render(<WorkflowPanel />);
    fireEvent.click(await screen.findByText("Nightly checks"));
    fireEvent.click(screen.getByText("Start on this session"));

    expect(await screen.findByText("Approve")).toBeInTheDocument();
    fireEvent.change(screen.getByPlaceholderText(/Approval note/), { target: { value: "ok" } });
    fireEvent.click(screen.getByText("Approve"));

    await waitFor(() => expect(api.workflow.runApprove).toHaveBeenCalledWith("run-1", "ok"));
  });
});
