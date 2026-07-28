import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { ReviewCard } from "./ReviewCard";
import { api } from "@/lib/api";

// 051-Editor-Excellence-Roadmap.md Wave 5.1f: mission.review.list had a real
// backend and no caller anywhere.

vi.mock("@/lib/api", () => ({
  api: {
    missionReview: { run: vi.fn(), get: vi.fn(), list: vi.fn() },
  },
}));

vi.mock("@/lib/dialog", () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}));

const review = {
  id: "review-1",
  mission_id: "mission-1",
  verdict: "clean" as const,
  findings: [],
  raw_output: "no issues",
  created_at: "2026-07-27T00:00:00Z",
};

describe("ReviewCard", () => {
  beforeEach(() => {
    vi.mocked(api.missionReview.run).mockReset();
    vi.mocked(api.missionReview.get).mockReset();
    vi.mocked(api.missionReview.list).mockReset();
    vi.mocked(api.missionReview.get).mockResolvedValue(null);
  });

  it("offers to run the Reviewer when no review exists yet", async () => {
    render(<ReviewCard missionId="mission-1" />);
    expect(await screen.findByText("Run Reviewer")).toBeInTheDocument();
  });

  it("running the Reviewer shows the verdict", async () => {
    vi.mocked(api.missionReview.run).mockResolvedValueOnce(review);
    render(<ReviewCard missionId="mission-1" />);
    await screen.findByText("Run Reviewer");

    fireEvent.click(screen.getByText("Run Reviewer"));

    expect(await screen.findByText("Clean")).toBeInTheDocument();
    expect(api.missionReview.run).toHaveBeenCalledWith("mission-1");
  });

  it("shows history for the mission when requested", async () => {
    vi.mocked(api.missionReview.get).mockResolvedValueOnce(review);
    vi.mocked(api.missionReview.list).mockResolvedValueOnce([
      { ...review, id: "review-0", verdict: "changes_requested", created_at: "2026-07-26T00:00:00Z", findings: [{ severity: "warning", file: "a.rs", description: "x" }] },
    ]);
    render(<ReviewCard missionId="mission-1" />);
    await screen.findByText("Clean");

    fireEvent.click(screen.getByLabelText("Show review history"));

    await waitFor(() => expect(api.missionReview.list).toHaveBeenCalledWith("mission-1"));
    expect(await screen.findByText("Changes requested")).toBeInTheDocument();
    expect(screen.getByText("1 finding")).toBeInTheDocument();
  });
});
