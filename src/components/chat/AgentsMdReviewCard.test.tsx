import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { AgentsMdReviewCard } from "./AgentsMdReviewCard";
import { api } from "@/lib/api";
import { useCid } from "@/hooks/useCid";

// review_prompt.md §1.2 / 051 Wave 5.4: the human-approval gate for
// repo-authored AGENTS.md before it ever reaches a model's context — a
// security-relevant surface (050-Gold-Standard-Review.md's own priority
// framing) that had no test coverage.

vi.mock("@/lib/api", () => ({
  api: {
    repo: { agentsMd: vi.fn(), agentsMdApprove: vi.fn() },
  },
}));

vi.mock("@/hooks/useCid", () => ({
  useCid: vi.fn(),
}));

const session = { id: "session-1", repo_channel_id: "repo-1" };
const unapprovedRepo = { id: "repo-1", name: "cid", path: "/tmp/repo", agents_md_approved: false };
const approvedRepo = { ...unapprovedRepo, agents_md_approved: true };

function mockState(repo: typeof unapprovedRepo, loadRepos = vi.fn()) {
  vi.mocked(useCid).mockReturnValue({ sessions: [session], repos: [repo], loadRepos } as any);
  return loadRepos;
}

describe("AgentsMdReviewCard", () => {
  beforeEach(() => {
    vi.mocked(api.repo.agentsMd).mockReset();
    vi.mocked(api.repo.agentsMdApprove).mockReset();
  });

  it("renders nothing once AGENTS.md is already approved", () => {
    mockState(approvedRepo);
    const { container } = render(<AgentsMdReviewCard sessionId="session-1" />);
    expect(container).toBeEmptyDOMElement();
    expect(api.repo.agentsMd).not.toHaveBeenCalled();
  });

  it("renders nothing when there's no session selected", () => {
    vi.mocked(useCid).mockReturnValue({ sessions: [], repos: [], loadRepos: vi.fn() } as any);
    const { container } = render(<AgentsMdReviewCard sessionId={null} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("shows the review gate with content hidden by default when unapproved", async () => {
    mockState(unapprovedRepo);
    vi.mocked(api.repo.agentsMd).mockResolvedValueOnce({ content: "Always run tests before committing." });
    render(<AgentsMdReviewCard sessionId="session-1" />);

    expect(await screen.findByText("This repo ships agent instructions")).toBeInTheDocument();
    expect(screen.queryByText("Always run tests before committing.")).not.toBeInTheDocument();
  });

  it("Show contents reveals the untrusted AGENTS.md text", async () => {
    mockState(unapprovedRepo);
    vi.mocked(api.repo.agentsMd).mockResolvedValueOnce({ content: "Always run tests before committing." });
    render(<AgentsMdReviewCard sessionId="session-1" />);
    await screen.findByText("This repo ships agent instructions");

    fireEvent.click(screen.getByText("Show contents"));

    expect(screen.getByText("Always run tests before committing.")).toBeInTheDocument();
  });

  it("approving calls agentsMdApprove for this repo and reloads repos", async () => {
    const loadRepos = mockState(unapprovedRepo);
    vi.mocked(api.repo.agentsMd).mockResolvedValueOnce({ content: "Some instructions." });
    vi.mocked(api.repo.agentsMdApprove).mockResolvedValueOnce({ ok: true });
    render(<AgentsMdReviewCard sessionId="session-1" />);
    await screen.findByText("This repo ships agent instructions");

    fireEvent.click(screen.getByText("Looks fine — use it"));

    await waitFor(() => expect(api.repo.agentsMdApprove).toHaveBeenCalledWith("repo-1"));
    await waitFor(() => expect(loadRepos).toHaveBeenCalled());
  });

  it("shows an error if approval fails, without crashing", async () => {
    mockState(unapprovedRepo);
    vi.mocked(api.repo.agentsMd).mockResolvedValueOnce({ content: "Some instructions." });
    vi.mocked(api.repo.agentsMdApprove).mockRejectedValueOnce(new Error("network down"));
    render(<AgentsMdReviewCard sessionId="session-1" />);
    await screen.findByText("This repo ships agent instructions");

    fireEvent.click(screen.getByText("Looks fine — use it"));

    expect(await screen.findByText(/network down/)).toBeInTheDocument();
  });
});
