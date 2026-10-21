import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { SessionModelBar } from "./SessionModelBar";
import { api } from "@/lib/api";

// The session's model was set at creation and invisible afterwards —
// this pins the mid-conversation visibility and switching.

vi.mock("@/lib/api", () => ({
  api: {
    model: { list: vi.fn(), routeRecommend: vi.fn() },
    session: { get: vi.fn(), setModel: vi.fn() },
  },
}));

const models = [
  { id: "claude-sonnet-5", name: "Sonnet", provider: "anthropic", context_length: 1000000, default: false, available: true },
  { id: "gemini-4-argon", name: "Argon", provider: "google", context_length: 1048576, default: true, available: true },
  { id: "retired-x", name: "Retired", provider: "openai", context_length: 1000, default: false, available: false },
];

describe("SessionModelBar", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.mocked(api.model.list).mockReset().mockResolvedValue(models as any);
    vi.mocked(api.model.routeRecommend).mockReset().mockResolvedValue({ mode: "manual" } as any);
    vi.mocked(api.session.get).mockReset().mockResolvedValue({ id: "s1", model_provider: null, model_id: null } as any);
    vi.mocked(api.session.setModel).mockReset().mockResolvedValue({ id: "s1" } as any);
  });

  it("shows the resolved default when the session has no override", async () => {
    render(<SessionModelBar sessionId="s1" />);
    expect(await screen.findByTestId("current-model")).toHaveTextContent("google gemini-4-argon");
  });

  it("shows the session override when one is set", async () => {
    vi.mocked(api.session.get).mockResolvedValueOnce({
      id: "s1",
      model_provider: "openai_compatible",
      model_id: "local-model",
    } as any);
    render(<SessionModelBar sessionId="s1" />);
    expect(await screen.findByTestId("current-model")).toHaveTextContent(
      "openai_compatible local-model · session override"
    );
  });

  it("switching sends provider and id, clearing restores default", async () => {
    vi.mocked(api.session.get)
      .mockResolvedValueOnce({ id: "s1", model_provider: null, model_id: null } as any)
      .mockResolvedValue({ id: "s1", model_provider: "anthropic", model_id: "claude-sonnet-5" } as any);
    render(<SessionModelBar sessionId="s1" />);
    await screen.findByTestId("current-model");

    fireEvent.change(screen.getByLabelText("Switch session model"), {
      target: { value: "anthropic::claude-sonnet-5" },
    });
    await waitFor(() =>
      expect(api.session.setModel).toHaveBeenCalledWith("s1", "anthropic", "claude-sonnet-5")
    );

    fireEvent.change(screen.getByLabelText("Switch session model"), { target: { value: "" } });
    await waitFor(() => expect(api.session.setModel).toHaveBeenCalledWith("s1", null, null));
  });

  it("shows the auto-routing recommendation when no override is set", async () => {
    vi.mocked(api.model.routeRecommend).mockResolvedValueOnce({
      mode: "auto_balanced",
      provider: "google",
      model_id: "gemini-4-argon",
      reason: "balanced · standard task",
    } as any);
    render(<SessionModelBar sessionId="s1" />);
    expect(await screen.findByTestId("current-model")).toHaveTextContent(
      "Auto (balanced): google gemini-4-argon"
    );
  });

  it("hides unavailable models from the switcher", async () => {
    render(<SessionModelBar sessionId="s1" />);
    await screen.findByTestId("current-model");
    expect(screen.queryByText(/retired-x/)).not.toBeInTheDocument();
  });
});
