import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { ConfidenceCard } from "./ConfidenceCard";
import { api } from "@/lib/api";

// 051-Editor-Excellence-Roadmap.md Wave 5.1f: confidence.history had a real
// backend and no caller anywhere.

vi.mock("@/lib/api", () => ({
  api: {
    confidence: { score: vi.fn(), history: vi.fn() },
  },
}));

vi.mock("@/lib/dialog", () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}));

const card = {
  patch_id: "patch-1",
  overall: 0.82,
  signals: [{ signal: "static_analysis", score: 0.9, explanation: "No issues found" }],
  generated_at: "2026-07-27T00:00:00Z",
  explanation: "High confidence overall",
};

describe("ConfidenceCard", () => {
  beforeEach(() => {
    vi.mocked(api.confidence.score).mockReset();
    vi.mocked(api.confidence.history).mockReset();
  });

  it("scores on demand and shows the overall band", async () => {
    vi.mocked(api.confidence.score).mockResolvedValueOnce(card);
    render(<ConfidenceCard missionId="mission-1" filePath="src/lib.rs" />);

    fireEvent.click(screen.getByText("Score confidence"));

    expect(await screen.findByText("82/100")).toBeInTheDocument();
    expect(api.confidence.score).toHaveBeenCalledWith("mission-1", "src/lib.rs");
  });

  it("shows history for the mission when requested", async () => {
    vi.mocked(api.confidence.score).mockResolvedValueOnce(card);
    vi.mocked(api.confidence.history).mockResolvedValueOnce([
      { patch_id: "patch-0", overall: 0.55, signals: [], generated_at: "2026-07-26T00:00:00Z", explanation: "" },
    ]);
    render(<ConfidenceCard missionId="mission-1" filePath="src/lib.rs" />);
    fireEvent.click(screen.getByText("Score confidence"));
    await screen.findByText("82/100");

    fireEvent.click(screen.getByLabelText("Show confidence history"));

    await waitFor(() => expect(api.confidence.history).toHaveBeenCalledWith("mission-1"));
    expect(await screen.findByText("55")).toBeInTheDocument();
  });
});
