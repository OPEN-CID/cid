import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { CheckpointCard } from "./CheckpointCard";
import { api } from "@/lib/api";

// review_prompt.md §3.2 / 051 Wave 5.4: rewind is a destructive, irreversible
// git operation (review_prompt.md's own "gate destructive actions" priority)
// — these pin the two-step confirm and that a bare click never rewinds.

vi.mock("@/lib/api", () => ({
  api: {
    session: {
      get: vi.fn(),
      checkpointList: vi.fn(),
      checkpointRewind: vi.fn(),
    },
  },
}));

const worktreeSession = { id: "session-1", worktree_path: "/tmp/repo/.cid/worktrees/session-1" };
const checkpoints = [
  { id: "cp-1", session_id: "session-1", sha: "abc1234567", label: "Before Implementer turn 1", created_at: "2026-07-27T00:00:00Z" },
  { id: "cp-2", session_id: "session-1", sha: "def7654321", label: "Before Implementer turn 2", created_at: "2026-07-27T00:05:00Z" },
];

describe("CheckpointCard", () => {
  beforeEach(() => {
    vi.mocked(api.session.get).mockReset();
    vi.mocked(api.session.checkpointList).mockReset();
    vi.mocked(api.session.checkpointRewind).mockReset();
  });

  it("renders nothing for a shared-clone session (no worktree)", async () => {
    vi.mocked(api.session.get).mockResolvedValueOnce({ id: "session-1", worktree_path: null });
    vi.mocked(api.session.checkpointList).mockResolvedValueOnce(checkpoints);
    const { container } = render(<CheckpointCard sessionId="session-1" />);

    await waitFor(() => expect(api.session.get).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
  });

  it("renders nothing when there are no checkpoints yet", async () => {
    vi.mocked(api.session.get).mockResolvedValueOnce(worktreeSession);
    vi.mocked(api.session.checkpointList).mockResolvedValueOnce([]);
    const { container } = render(<CheckpointCard sessionId="session-1" />);

    await waitFor(() => expect(api.session.checkpointList).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
  });

  it("lists checkpoints newest-first for a worktree session", async () => {
    vi.mocked(api.session.get).mockResolvedValueOnce(worktreeSession);
    vi.mocked(api.session.checkpointList).mockResolvedValueOnce(checkpoints);
    render(<CheckpointCard sessionId="session-1" />);

    const labels = await screen.findAllByText(/Before Implementer turn/);
    expect(labels[0]).toHaveTextContent("turn 2");
    expect(labels[1]).toHaveTextContent("turn 1");
  });

  it("clicking Rewind does not call checkpointRewind until Confirm is clicked", async () => {
    vi.mocked(api.session.get).mockResolvedValueOnce(worktreeSession);
    vi.mocked(api.session.checkpointList).mockResolvedValueOnce(checkpoints);
    render(<CheckpointCard sessionId="session-1" />);
    await screen.findAllByText(/Before Implementer turn/);

    fireEvent.click(screen.getAllByText("Rewind")[0]);

    expect(screen.getByText("Discards later changes")).toBeInTheDocument();
    expect(api.session.checkpointRewind).not.toHaveBeenCalled();
  });

  it("Cancel backs out of the confirm step without rewinding", async () => {
    vi.mocked(api.session.get).mockResolvedValueOnce(worktreeSession);
    vi.mocked(api.session.checkpointList).mockResolvedValueOnce(checkpoints);
    render(<CheckpointCard sessionId="session-1" />);
    await screen.findAllByText(/Before Implementer turn/);

    fireEvent.click(screen.getAllByText("Rewind")[0]);
    fireEvent.click(screen.getByText("Cancel"));

    expect(screen.queryByText("Discards later changes")).not.toBeInTheDocument();
    expect(api.session.checkpointRewind).not.toHaveBeenCalled();
  });

  it("Confirm rewinds to the specific checkpoint and reloads", async () => {
    vi.mocked(api.session.get).mockResolvedValue(worktreeSession);
    vi.mocked(api.session.checkpointList).mockResolvedValue(checkpoints);
    vi.mocked(api.session.checkpointRewind).mockResolvedValueOnce({ ok: true });
    render(<CheckpointCard sessionId="session-1" />);
    await screen.findAllByText(/Before Implementer turn/);

    fireEvent.click(screen.getAllByText("Rewind")[0]);
    fireEvent.click(screen.getByText("Confirm"));

    await waitFor(() => expect(api.session.checkpointRewind).toHaveBeenCalledWith("session-1", "cp-2", true));
  });
});
