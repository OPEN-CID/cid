import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor, act } from "@testing-library/react";
import { MobileApp } from "./MobileApp";
import { api } from "../lib/api";

// 051-Editor-Excellence-Roadmap.md Wave 5.4. Notification/SpeechRecognition
// are absent in jsdom by default, which is itself the "unsupported platform"
// path this component is built to degrade gracefully on — left unmocked
// deliberately so that path is what's actually exercised.

let notificationHandler: ((notif: any) => void) | null = null;

vi.mock("../lib/api", () => ({
  api: {
    connect: vi.fn(),
    session: { list: vi.fn(), approveTool: vi.fn(), sendMessage: vi.fn() },
    message: { list: vi.fn() },
    git: { diff: vi.fn() },
    onNotification: vi.fn((handler: (notif: any) => void) => {
      notificationHandler = handler;
      return () => {
        notificationHandler = null;
      };
    }),
  },
}));

const session = {
  id: "session-1",
  title: "Fix login bug",
  status: "blocked_on_approval",
  autonomy_level: "co_pilot",
  isolation_mode: "worktree",
  worktree_path: "/tmp/repo/.cid/worktrees/session-1",
  repo_channel_id: "repo-1",
  updated_at: "2026-07-27T00:00:00Z",
};

async function flush() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe("MobileApp", () => {
  beforeEach(() => {
    notificationHandler = null;
    vi.mocked(api.connect).mockReset();
    vi.mocked(api.session.list).mockReset();
    vi.mocked(api.session.approveTool).mockReset();
    vi.mocked(api.session.sendMessage).mockReset();
    vi.mocked(api.message.list).mockReset();
    vi.mocked(api.git.diff).mockReset();
    vi.mocked(api.connect).mockResolvedValue(undefined);
    vi.mocked(api.message.list).mockResolvedValue([]);
  });

  it("lists sessions with the blocked-on-approval one flagged", async () => {
    vi.mocked(api.session.list).mockResolvedValueOnce([session]);
    render(<MobileApp />);

    expect(await screen.findByText("Fix login bug")).toBeInTheDocument();
    expect(screen.getByText("needs you")).toBeInTheDocument();
  });

  it("opening a session loads its thread and shows the pending approval", async () => {
    vi.mocked(api.session.list).mockResolvedValueOnce([session]);
    render(<MobileApp />);
    fireEvent.click(await screen.findByText("Fix login bug"));
    await flush();

    act(() => {
      notificationHandler?.({
        method: "session.tool_call.request",
        params: { session_id: "session-1", tool_call_id: "tc-1", tool_name: "write_file", arguments: { path: "a.rs" } },
      });
    });

    expect(await screen.findByText(/Approval needed: write_file/)).toBeInTheDocument();
  });

  it("Approve calls approveTool with approved=true and clears the card", async () => {
    vi.mocked(api.session.list).mockResolvedValueOnce([session]);
    vi.mocked(api.session.approveTool).mockResolvedValueOnce({ ok: true });
    render(<MobileApp />);
    fireEvent.click(await screen.findByText("Fix login bug"));
    await flush();
    act(() => {
      notificationHandler?.({
        method: "session.tool_call.request",
        params: { session_id: "session-1", tool_call_id: "tc-1", tool_name: "write_file", arguments: {} },
      });
    });
    await screen.findByText(/Approval needed/);

    fireEvent.click(screen.getByText("Approve"));

    await waitFor(() => expect(api.session.approveTool).toHaveBeenCalledWith("session-1", "tc-1", true));
    await waitFor(() => expect(screen.queryByText(/Approval needed/)).not.toBeInTheDocument());
  });

  it("Deny calls approveTool with approved=false", async () => {
    vi.mocked(api.session.list).mockResolvedValueOnce([session]);
    vi.mocked(api.session.approveTool).mockResolvedValueOnce({ ok: true });
    render(<MobileApp />);
    fireEvent.click(await screen.findByText("Fix login bug"));
    await flush();
    act(() => {
      notificationHandler?.({
        method: "session.tool_call.request",
        params: { session_id: "session-1", tool_call_id: "tc-1", tool_name: "run_terminal", arguments: {} },
      });
    });
    await screen.findByText(/Approval needed/);

    fireEvent.click(screen.getByText("Deny"));

    await waitFor(() => expect(api.session.approveTool).toHaveBeenCalledWith("session-1", "tc-1", false));
  });

  it("sending a reply calls sendMessage and clears the input", async () => {
    vi.mocked(api.session.list).mockResolvedValueOnce([session]);
    vi.mocked(api.session.sendMessage).mockResolvedValueOnce({ ok: true });
    render(<MobileApp />);
    fireEvent.click(await screen.findByText("Fix login bug"));
    await flush();

    const textarea = screen.getByPlaceholderText("Comment…");
    fireEvent.change(textarea, { target: { value: "Looks good, proceed" } });
    fireEvent.click(screen.getByLabelText("Send"));

    await waitFor(() => expect(api.session.sendMessage).toHaveBeenCalledWith("session-1", "Looks good, proceed"));
    expect(textarea).toHaveValue("");
  });

  it("the diff tab fetches and shows the worktree diff", async () => {
    vi.mocked(api.session.list).mockResolvedValueOnce([session]);
    vi.mocked(api.git.diff).mockResolvedValueOnce([{ path: "a.rs" }]);
    render(<MobileApp />);
    fireEvent.click(await screen.findByText("Fix login bug"));
    await flush();

    fireEvent.click(screen.getByText("diff"));

    await waitFor(() => expect(api.git.diff).toHaveBeenCalledWith("/tmp/repo/.cid/worktrees/session-1"));
    expect(await screen.findByText(/"path": "a.rs"/)).toBeInTheDocument();
  });

  it("does not crash when SpeechRecognition and Notification are unavailable (jsdom default)", async () => {
    vi.mocked(api.session.list).mockResolvedValueOnce([session]);
    render(<MobileApp />);
    fireEvent.click(await screen.findByText("Fix login bug"));
    await flush();

    expect(screen.queryByLabelText("Voice input")).not.toBeInTheDocument();
  });

  it("Back returns to the session list", async () => {
    vi.mocked(api.session.list).mockResolvedValue([session]);
    render(<MobileApp />);
    fireEvent.click(await screen.findByText("Fix login bug"));
    await flush();

    fireEvent.click(screen.getByLabelText("Back"));

    expect(await screen.findByText("Sessions")).toBeInTheDocument();
  });
});
