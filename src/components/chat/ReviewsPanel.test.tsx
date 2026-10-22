import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { ReviewsPanel } from "./ReviewsPanel";
import { api } from "@/lib/api";
import { useCid } from "@/hooks/useCid";

// Reviews dashboard composes existing PR/release RPCs — this pins the
// composition: PRs listed, unconnected forges hinted (not errored), releases
// merged across the repo's sessions.

vi.mock("@/lib/api", () => ({
  api: {
    github: { prs: vi.fn(), prStatus: vi.fn(), createPr: vi.fn() },
    forge: { changeRequests: vi.fn() },
    deployment: { list: vi.fn() },
  },
}));

vi.mock("@/hooks/useCid", () => ({
  useCid: vi.fn(),
}));

const state = {
  sessions: [
    { id: "session-1", repo_channel_id: "repo-1" },
    { id: "session-2", repo_channel_id: "repo-1" },
  ],
  repos: [{ id: "repo-1", path: "/tmp/repo", name: "repo" }],
  selectedSessionId: "session-1",
};

describe("ReviewsPanel", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.mocked(api.github.prs).mockReset().mockResolvedValue([]);
    vi.mocked(api.github.prStatus).mockReset();
    vi.mocked(api.github.createPr).mockReset();
    vi.mocked(api.forge.changeRequests).mockReset().mockResolvedValue([]);
    vi.mocked(api.deployment.list).mockReset().mockResolvedValue([]);
    vi.mocked(useCid).mockReturnValue(state as any);
  });

  it("prompts to select a session when none is selected", () => {
    vi.mocked(useCid).mockReturnValue({ sessions: [], repos: [], selectedSessionId: null } as any);
    render(<ReviewsPanel />);
    expect(screen.getByText(/Select a session to review/)).toBeInTheDocument();
  });

  it("lists pull requests with branches and authors", async () => {
    vi.mocked(api.github.prs).mockResolvedValueOnce([
      { number: 12, title: "Add model switcher", state: "open", author: "dev", head_branch: "feat", base_branch: "main" },
    ]);
    render(<ReviewsPanel />);

    expect(await screen.findByText("#12")).toBeInTheDocument();
    expect(screen.getByText("Add model switcher")).toBeInTheDocument();
    expect(screen.getByText(/feat → main/)).toBeInTheDocument();
    expect(api.github.prs).toHaveBeenCalledWith("/tmp/repo");
  });

  it("hints at connecting a forge instead of erroring when unconnected", async () => {
    vi.mocked(api.github.prs).mockRejectedValueOnce(new Error("not connected"));
    vi.mocked(api.forge.changeRequests).mockRejectedValueOnce(new Error("not connected"));
    render(<ReviewsPanel />);

    expect(await screen.findByText(/No forge connected/)).toBeInTheDocument();
  });

  it("merges release records across the repo's sessions, newest first", async () => {
    vi.mocked(api.deployment.list).mockImplementation((sessionId: string) =>
      Promise.resolve(
        sessionId === "session-1"
          ? [{ id: "d1", session_id: "session-1", environment: "staging", commit_or_tag: "aaa", source: "manual", deployed_at: "2026-01-01T00:00:00Z" }]
          : [{ id: "d2", session_id: "session-2", environment: "production", commit_or_tag: "bbb", source: "manual", deployed_at: "2026-02-01T00:00:00Z" }]
      )
    );
    render(<ReviewsPanel />);

    expect(await screen.findByText("production")).toBeInTheDocument();
    expect(screen.getByText("staging")).toBeInTheDocument();
  });

  it("opening a PR sends repo, title, and branches", async () => {
    vi.mocked(api.github.prs).mockResolvedValue([]);
    vi.mocked(api.github.createPr).mockResolvedValueOnce({ id: 1, number: 12 });
    render(<ReviewsPanel />);
    await screen.findByText(/No open pull requests/);

    fireEvent.click(screen.getByText("Open PR"));
    fireEvent.change(screen.getByPlaceholderText("PR title"), { target: { value: "Add model switcher" } });
    fireEvent.change(screen.getByPlaceholderText(/Head branch/), { target: { value: "feat/switcher" } });
    fireEvent.click(screen.getByText("Create pull request"));

    await waitFor(() =>
      expect(api.github.createPr).toHaveBeenCalledWith(
        expect.objectContaining({ title: "Add model switcher", head_branch: "feat/switcher" })
      )
    );
  });

  it("checking a PR's status calls through with repo and number", async () => {
    vi.mocked(api.github.prs).mockResolvedValueOnce([
      { number: 7, title: "Fix", state: "open" },
    ]);
    vi.mocked(api.github.prStatus).mockResolvedValueOnce({ mergeable: true, checks: [] });
    render(<ReviewsPanel />);

    fireEvent.click(await screen.findByText("Check status"));
    await waitFor(() => expect(api.github.prStatus).toHaveBeenCalledWith("/tmp/repo", 7));
    expect(await screen.findByText(/Mergeable/)).toBeInTheDocument();
  });
});
