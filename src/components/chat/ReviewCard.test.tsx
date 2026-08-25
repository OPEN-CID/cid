import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { ReviewCard } from "./ReviewCard";
import { api } from "@/lib/api";

// 051-Editor-Excellence-Roadmap.md Wave 5.1f: session.review.list had a real
// backend and no caller anywhere.

vi.mock("@/lib/api", () => ({
  api: {
    sessionReview: { run: vi.fn(), get: vi.fn(), list: vi.fn() },
  },
}));

vi.mock("@/lib/dialog", () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}));

const review = {
  id: "review-1",
  session_id: "session-1",
  verdict: "clean" as const,
  findings: [],
  raw_output: "no issues",
  created_at: "2026-07-27T00:00:00Z",
};

describe("ReviewCard", () => {
  beforeEach(() => {
    vi.mocked(api.sessionReview.run).mockReset();
    vi.mocked(api.sessionReview.get).mockReset();
    vi.mocked(api.sessionReview.list).mockReset();
    vi.mocked(api.sessionReview.get).mockResolvedValue(null);
  });

  it("offers to run the Reviewer when no review exists yet", async () => {
    render(<ReviewCard sessionId="session-1" />);
    expect(await screen.findByText("Run Reviewer")).toBeInTheDocument();
  });

  it("running the Reviewer shows the verdict", async () => {
    vi.mocked(api.sessionReview.run).mockResolvedValueOnce(review);
    render(<ReviewCard sessionId="session-1" />);
    await screen.findByText("Run Reviewer");

    fireEvent.click(screen.getByText("Run Reviewer"));

    expect(await screen.findByText("Clean")).toBeInTheDocument();
    expect(api.sessionReview.run).toHaveBeenCalledWith("session-1");
  });

  it("shows history for the session when requested", async () => {
    vi.mocked(api.sessionReview.get).mockResolvedValueOnce(review);
    vi.mocked(api.sessionReview.list).mockResolvedValueOnce([
      { ...review, id: "review-0", verdict: "changes_requested", created_at: "2026-07-26T00:00:00Z", findings: [{ severity: "warning", file: "a.rs", description: "x" }] },
    ]);
    render(<ReviewCard sessionId="session-1" />);
    await screen.findByText("Clean");

    fireEvent.click(screen.getByLabelText("Show review history"));

    await waitFor(() => expect(api.sessionReview.list).toHaveBeenCalledWith("session-1"));
    expect(await screen.findByText("Changes requested")).toBeInTheDocument();
    expect(screen.getByText("1 finding")).toBeInTheDocument();
  });
});
