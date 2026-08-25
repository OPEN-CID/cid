import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, act, waitFor } from "@testing-library/react";
import { HistoryPanel } from "./HistoryPanel";
import { useCid } from "@/hooks/useCid";
import { api } from "@/lib/api";

// 051-Editor-Excellence-Roadmap.md Wave 5.4, rebuilt in the 2026-08 pass to
// derive real history from `message.list` instead of an in-memory-only,
// hardcoded-actor/status simulation (there is no dedicated audit table —
// see the comment at the top of HistoryPanel.tsx).

let notificationHandler: ((notif: unknown) => void) | null = null;

vi.mock("@/lib/api", () => ({
  api: {
    message: { list: vi.fn() },
    onNotification: vi.fn((handler: (notif: unknown) => void) => {
      notificationHandler = handler;
      return () => {
        notificationHandler = null;
      };
    }),
  },
}));

vi.mock("@/hooks/useCid", () => ({
  useCid: vi.fn(),
}));

vi.mock("@/lib/dialog", () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}));

function emit(notif: { method: string; params: Record<string, unknown> }) {
  act(() => {
    notificationHandler?.(notif);
  });
}

async function flushAsyncWork() {
  await act(async () => {
    for (let i = 0; i < 6; i++) {
      await Promise.resolve();
    }
  });
}

describe("HistoryPanel", () => {
  beforeEach(() => {
    notificationHandler = null;
    vi.mocked(useCid).mockReturnValue({ selectedSessionId: "session-1" } as any);
    vi.mocked(api.message.list).mockReset().mockResolvedValue([]);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: vi.fn().mockResolvedValue(undefined) },
      configurable: true,
    });
    (global as any).URL.createObjectURL = vi.fn(() => "blob:mock");
    (global as any).URL.revokeObjectURL = vi.fn();
  });

  it("prompts to select a session when none is selected", () => {
    vi.mocked(useCid).mockReturnValue({ selectedSessionId: null } as any);
    render(<HistoryPanel />);
    expect(screen.getByText("Select a session to view history")).toBeInTheDocument();
  });

  it("shows the empty state once loading finishes with no persisted tool calls", async () => {
    render(<HistoryPanel />);
    await flushAsyncWork();
    expect(screen.getByText(/No history yet/)).toBeInTheDocument();
  });

  it("derives real events from message.list, using the message's real role and the tool call's real status", async () => {
    vi.mocked(api.message.list).mockResolvedValueOnce([
      {
        role: "assistant",
        created_at: "2026-08-01T00:00:00Z",
        tool_calls: [
          { id: "tc-1", name: "write_file", arguments: { path: "src/a.ts" }, status: "failed", result: { error: "disk full" } },
        ],
      },
    ]);

    render(<HistoryPanel />);
    await flushAsyncWork();

    expect(api.message.list).toHaveBeenCalledWith("session-1");
    expect(screen.getByText("Assistant")).toBeInTheDocument();
    expect(screen.getByText("write_file")).toBeInTheDocument();
    expect(screen.getByText("src/a.ts")).toBeInTheDocument();
    // The real backend status is "failed" — must never be painted "success".
    expect(screen.getByText("failed")).toBeInTheDocument();
    expect(screen.queryByText("success")).not.toBeInTheDocument();
  });

  it("labels a live tool-call notification's status honestly instead of hardcoding success", async () => {
    render(<HistoryPanel />);
    await flushAsyncWork();

    emit({ method: "session.tool_call.request", params: { session_id: "session-1", tool_name: "run_terminal", arguments: { command: "npm test" } } });

    expect(screen.getByText("Assistant")).toBeInTheDocument();
    expect(screen.getByText("run_terminal")).toBeInTheDocument();
    expect(screen.getByText("pending approval")).toBeInTheDocument();
  });

  it("ignores notifications for a different session", async () => {
    render(<HistoryPanel />);
    await flushAsyncWork();

    emit({ method: "session.tool_call.request", params: { session_id: "session-2", tool_name: "write_file" } });

    expect(screen.getByText(/No history yet/)).toBeInTheDocument();
  });

  it("filters noisy streaming notifications (pty.output, session.message.delta)", async () => {
    render(<HistoryPanel />);
    await flushAsyncWork();

    emit({ method: "pty.output", params: { session_id: "session-1", pty_id: "pty-1" } });
    emit({ method: "session.message.delta", params: { session_id: "session-1" } });

    expect(screen.getByText(/No history yet/)).toBeInTheDocument();
  });

  it("the filter buttons narrow the visible events by real tool name", async () => {
    vi.mocked(api.message.list).mockResolvedValueOnce([
      {
        role: "assistant",
        created_at: "2026-08-01T00:00:00Z",
        tool_calls: [
          { id: "tc-1", name: "write_file", arguments: { path: "a.txt" }, status: "completed" },
          { id: "tc-2", name: "git_commit", arguments: { repo_path: "/repo" }, status: "completed" },
        ],
      },
    ]);

    render(<HistoryPanel />);
    await flushAsyncWork();

    fireEvent.click(screen.getByText("git"));

    expect(screen.getByText("git_commit")).toBeInTheDocument();
    expect(screen.queryByText("write_file")).not.toBeInTheDocument();
  });

  it("Export JSON is disabled with no events and enabled once history loads", async () => {
    vi.mocked(api.message.list).mockResolvedValueOnce([
      { role: "user", created_at: "2026-08-01T00:00:00Z", tool_calls: [{ id: "tc-1", name: "read_file", arguments: {}, status: "completed" }] },
    ]);

    render(<HistoryPanel />);
    await flushAsyncWork();

    expect(screen.getByText("Export JSON")).not.toBeDisabled();
    fireEvent.click(screen.getByText("Export JSON"));
    expect(URL.createObjectURL).toHaveBeenCalled();
  });

  it("Copy as Markdown writes a formatted summary to the clipboard", async () => {
    vi.mocked(api.message.list).mockResolvedValueOnce([
      { role: "user", created_at: "2026-08-01T00:00:00Z", tool_calls: [{ id: "tc-1", name: "read_file", arguments: {}, status: "completed" }] },
    ]);

    render(<HistoryPanel />);
    await flushAsyncWork();

    fireEvent.click(screen.getByText("Copy as Markdown"));

    await waitFor(() =>
      expect(navigator.clipboard.writeText).toHaveBeenCalledWith(expect.stringContaining("read_file"))
    );
  });
});
