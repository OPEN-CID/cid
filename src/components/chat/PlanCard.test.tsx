import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, act } from "@testing-library/react";
import { PlanCard } from "./PlanCard";
import { api } from "@/lib/api";

// Flushes the microtask queue enough times for a `click -> await api.call ->
// await load() -> await api.call -> setState` chain to fully settle, inside
// an `act()` so React commits the resulting re-render synchronously. Used
// instead of `findByText` polling for multi-await chains — MutationObserver
// timing across a suite with several jsdom environments running is a
// flakiness source `act`-flushing avoids entirely.
async function flushAsyncWork() {
  await act(async () => {
    for (let i = 0; i < 6; i++) {
      await Promise.resolve();
    }
  });
}

vi.mock("@/lib/api", () => ({
  api: {
    call: vi.fn(),
    onNotification: () => () => {},
  },
}));

describe("PlanCard", () => {
  beforeEach(() => {
    vi.mocked(api.call).mockReset();
  });

  it("shows no plan state when no plan exists", async () => {
    vi.mocked(api.call).mockResolvedValueOnce({ plan: null });

    render(<PlanCard missionId="mission-1" />);

    expect(await screen.findByText(/No plan yet/)).toBeInTheDocument();
    expect(screen.getByText(/Run Planner/)).toBeInTheDocument();
  });

  it("displays an approved plan and does not show approve/reject actions for it", async () => {
    vi.mocked(api.call).mockResolvedValueOnce({
      plan: {
        id: "plan-1",
        mission_id: "mission-1",
        content: "# Test Plan\nThis is the plan.",
        status: "approved",
        approved_by: "user123",
        updated_at: "2024-01-01T00:00:00Z",
      },
    });

    render(<PlanCard missionId="mission-1" />);

    expect(await screen.findByText(/This is the plan\./)).toBeInTheDocument();
    expect(screen.getByText("approved")).toBeInTheDocument();
    expect(screen.getByText(/approved by user123/)).toBeInTheDocument();
    // review_prompt.md §5: an already-approved plan must not still offer
    // Approve/Reject — those actions apply to a draft, not a decided plan.
    expect(screen.queryByText(/Approve plan/)).not.toBeInTheDocument();
    expect(screen.queryByText(/^Reject$/)).not.toBeInTheDocument();
  });

  it("enters edit mode and returns an approved plan to draft on save", async () => {
    vi.mocked(api.call)
      .mockResolvedValueOnce({
        plan: {
          id: "plan-1",
          mission_id: "mission-1",
          content: "# Test Plan\nThis is the plan.",
          status: "draft",
          approved_by: null,
          updated_at: "2024-01-01T00:00:00Z",
        },
      })
      .mockResolvedValueOnce({}) // mission.plan.update
      .mockResolvedValueOnce({
        plan: {
          id: "plan-1",
          mission_id: "mission-1",
          content: "# Edited plan",
          status: "draft",
          approved_by: null,
          updated_at: "2024-01-01T00:01:00Z",
        },
      }); // reload after save

    render(<PlanCard missionId="mission-1" />);

    const editBtn = await screen.findByRole("button", { name: /Edit/i });
    fireEvent.click(editBtn);

    const textarea = screen.getByRole("textbox");
    fireEvent.change(textarea, { target: { value: "# Edited plan" } });

    const saveBtn = screen.getByRole("button", { name: /Save/i });
    fireEvent.click(saveBtn);
    await flushAsyncWork();

    expect(screen.getByText(/Edited plan/)).toBeInTheDocument();
    expect(vi.mocked(api.call)).toHaveBeenCalledWith("mission.plan.update", {
      mission_id: "mission-1",
      content: "# Edited plan",
    });
  });

  it("approving a plan calls mission.plan.approve for this mission", async () => {
    vi.mocked(api.call)
      .mockResolvedValueOnce({
        plan: {
          id: "plan-1",
          mission_id: "mission-1",
          content: "# Test Plan",
          status: "draft",
          approved_by: null,
          updated_at: "2024-01-01T00:00:00Z",
        },
      })
      .mockResolvedValueOnce({}) // mission.plan.approve
      .mockResolvedValueOnce({
        plan: {
          id: "plan-1",
          mission_id: "mission-1",
          content: "# Test Plan",
          status: "approved",
          approved_by: "me",
          updated_at: "2024-01-01T00:01:00Z",
        },
      });

    render(<PlanCard missionId="mission-1" />);

    const approveBtn = await screen.findByText(/Approve plan/);
    fireEvent.click(approveBtn);
    await flushAsyncWork();

    expect(screen.getByText(/approved by me/)).toBeInTheDocument();
    expect(vi.mocked(api.call)).toHaveBeenCalledWith("mission.plan.approve", {
      mission_id: "mission-1",
    });
  });
});
