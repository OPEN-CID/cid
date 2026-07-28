import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, act, waitFor } from "@testing-library/react";
import { HistoryPanel } from "./HistoryPanel";
import { useCid } from "@/hooks/useCid";

// 051-Editor-Excellence-Roadmap.md Wave 5.4. Also fixes and pins a real dead-
// control bug found while writing these: "Export JSON"/"Copy as Markdown"
// previously had no onClick at all.

let notificationHandler: ((notif: unknown) => void) | null = null;

vi.mock("@/lib/api", () => ({
  api: {
    onNotification: vi.fn((handler: (notif: unknown) => void) => {
      notificationHandler = handler;
      return () => {
        notificationHandler = null;
      };
    }),
  },
}));

vi.mock("@/hooks/useCid", () => ({
  useCid: vi.fn(),
}));

vi.mock("@/lib/dialog", () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}));

function emit(notif: { method: string; params: Record<string, unknown> }) {
  act(() => {
    notificationHandler?.(notif);
  });
}

describe("HistoryPanel", () => {
  beforeEach(() => {
    notificationHandler = null;
    vi.mocked(useCid).mockReturnValue({ selectedMissionId: "mission-1" } as any);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: vi.fn().mockResolvedValue(undefined) },
      configurable: true,
    });
    (global as any).URL.createObjectURL = vi.fn(() => "blob:mock");
    (global as any).URL.revokeObjectURL = vi.fn();
  });

  it("prompts to select a mission when none is selected", () => {
    vi.mocked(useCid).mockReturnValue({ selectedMissionId: null } as any);
    render(<HistoryPanel />);
    expect(screen.getByText("Select a mission to view history")).toBeInTheDocument();
  });

  it("shows the empty state before any notification arrives", () => {
    render(<HistoryPanel />);
    expect(screen.getByText(/No history yet/)).toBeInTheDocument();
  });

  it("appends an event when a matching-mission notification arrives", () => {
    render(<HistoryPanel />);
    emit({ method: "file.write", params: { mission_id: "mission-1", tool_name: "write_file" } });

    expect(screen.getByText("file.write")).toBeInTheDocument();
    expect(screen.getByText("write_file")).toBeInTheDocument();
  });

  it("ignores notifications for a different mission", () => {
    render(<HistoryPanel />);
    emit({ method: "file.write", params: { mission_id: "mission-2", tool_name: "write_file" } });

    expect(screen.getByText(/No history yet/)).toBeInTheDocument();
  });

  it("filters noisy streaming notifications (pty.output, mission.message.delta)", () => {
    render(<HistoryPanel />);
    emit({ method: "pty.output", params: { mission_id: "mission-1", pty_id: "pty-1" } });
    emit({ method: "mission.message.delta", params: { mission_id: "mission-1" } });

    expect(screen.getByText(/No history yet/)).toBeInTheDocument();
  });

  it("the filter buttons narrow the visible events by action", () => {
    render(<HistoryPanel />);
    emit({ method: "file.write", params: { mission_id: "mission-1", tool_name: "a.txt" } });
    emit({ method: "git.commit", params: { mission_id: "mission-1", tool_name: "commit" } });

    fireEvent.click(screen.getByText("git"));

    expect(screen.getByText("git.commit")).toBeInTheDocument();
    expect(screen.queryByText("file.write")).not.toBeInTheDocument();
  });

  it("Export JSON is disabled with no events and enabled once one exists", () => {
    render(<HistoryPanel />);
    expect(screen.getByText("Export JSON")).toBeDisabled();

    emit({ method: "file.write", params: { mission_id: "mission-1", tool_name: "a.txt" } });

    expect(screen.getByText("Export JSON")).not.toBeDisabled();
    fireEvent.click(screen.getByText("Export JSON"));
    expect(URL.createObjectURL).toHaveBeenCalled();
  });

  it("Copy as Markdown writes a formatted summary to the clipboard", async () => {
    render(<HistoryPanel />);
    emit({ method: "file.write", params: { mission_id: "mission-1", tool_name: "a.txt" } });

    fireEvent.click(screen.getByText("Copy as Markdown"));

    await waitFor(() =>
      expect(navigator.clipboard.writeText).toHaveBeenCalledWith(expect.stringContaining("file.write"))
    );
  });
});
