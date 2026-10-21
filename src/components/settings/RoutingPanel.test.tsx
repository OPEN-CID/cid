import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { RoutingPanel } from "./RoutingPanel";
import { api } from "@/lib/api";

// Auto-routing mode selection + decision preview against the real RPCs.

vi.mock("@/lib/api", () => ({
  api: {
    model: {
      routingGet: vi.fn(),
      routingSet: vi.fn(),
      routeRecommend: vi.fn(),
    },
  },
}));

describe("RoutingPanel", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.mocked(api.model.routingGet).mockReset().mockResolvedValue({ mode: "manual" });
    vi.mocked(api.model.routingSet).mockReset();
    vi.mocked(api.model.routeRecommend).mockReset();
  });

  it("loads and shows the current mode", async () => {
    render(<RoutingPanel />);
    expect(await screen.findByText("Automatic model routing")).toBeInTheDocument();
    expect(api.model.routingGet).toHaveBeenCalled();
  });

  it("selecting a mode persists it", async () => {
    vi.mocked(api.model.routingSet).mockResolvedValueOnce({ mode: "auto_balanced" });
    render(<RoutingPanel />);
    await screen.findByText("Automatic model routing");

    fireEvent.click(screen.getByText(/Auto · balanced/));
    await waitFor(() => expect(api.model.routingSet).toHaveBeenCalledWith("auto_balanced"));
  });

  it("previewing a task shows the explained decision", async () => {
    vi.mocked(api.model.routeRecommend).mockResolvedValueOnce({
      provider: "google",
      model_id: "gemini-4-argon",
      mode: "auto_balanced",
      complexity: "standard",
      reason: "cheapest capable default at $4.50/M",
      is_local: false,
    });
    render(<RoutingPanel />);
    await screen.findByText("Automatic model routing");

    fireEvent.change(screen.getByPlaceholderText(/Describe a task/), {
      target: { value: "fix typo" },
    });
    fireEvent.click(screen.getByText("Preview"));

    expect(await screen.findByText(/google gemini-4-argon/)).toBeInTheDocument();
    expect(screen.getByText(/cheapest capable default/)).toBeInTheDocument();
  });
});
