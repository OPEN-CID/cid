import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { AcpPanel } from "./AcpPanel";
import { api } from "@/lib/api";
import { useCid } from "@/hooks/useCid";

// 051-Editor-Excellence-Roadmap.md Wave 5.4.

vi.mock("@/lib/api", () => ({
  api: {
    acp: { editors: vi.fn(), handoffs: vi.fn(), handoff: vi.fn(), takeBack: vi.fn() },
  },
}));

vi.mock("@/hooks/useCid", () => ({
  useCid: vi.fn(),
}));

const editor = {
  id: "zed",
  name: "Zed",
  editor_type: "zed",
  available: true,
  executable_path: "/usr/local/bin/zed",
  supports_acp: true,
};

describe("AcpPanel", () => {
  beforeEach(() => {
    vi.mocked(api.acp.editors).mockReset();
    vi.mocked(api.acp.handoffs).mockReset();
    vi.mocked(api.acp.handoff).mockReset();
    vi.mocked(api.acp.takeBack).mockReset();
    vi.mocked(api.acp.editors).mockResolvedValue([editor]);
    vi.mocked(api.acp.handoffs).mockResolvedValue([]);
  });

  it("prompts to select a session when none is selected, but still lists editors", async () => {
    vi.mocked(useCid).mockReturnValue({ selectedSessionId: null } as any);
    render(<AcpPanel />);

    expect(await screen.findByText(/Select a Session to hand its worktree off/)).toBeInTheDocument();
    expect(await screen.findByText("Zed")).toBeInTheDocument();
    expect(screen.getByText("Hand off")).toBeDisabled();
  });

  it("handing off calls acp.handoff with the selected session and reloads", async () => {
    vi.mocked(useCid).mockReturnValue({ selectedSessionId: "session-1" } as any);
    vi.mocked(api.acp.handoff).mockResolvedValueOnce({ id: "handoff-1" });
    render(<AcpPanel />);
    await screen.findByText("Zed");

    fireEvent.click(screen.getByText("Hand off"));

    await waitFor(() => expect(api.acp.handoff).toHaveBeenCalledWith("session-1", "zed"));
  });

  it("shows active handoffs with a Take back action", async () => {
    vi.mocked(useCid).mockReturnValue({ selectedSessionId: "session-1" } as any);
    vi.mocked(api.acp.handoffs).mockResolvedValue([
      { id: "handoff-1", session_id: "session-1", editor_id: "zed", status: "handed_off", worktree_path: "/tmp/wt", created_at: "2026-07-27T00:00:00Z" },
    ]);
    vi.mocked(api.acp.takeBack).mockResolvedValueOnce({ ok: true });
    render(<AcpPanel />);

    expect(await screen.findByText("Take back")).toBeInTheDocument();
    fireEvent.click(screen.getByText("Take back"));

    await waitFor(() => expect(api.acp.takeBack).toHaveBeenCalledWith("handoff-1"));
  });

  it("an unavailable editor cannot be handed off", async () => {
    vi.mocked(useCid).mockReturnValue({ selectedSessionId: "session-1" } as any);
    vi.mocked(api.acp.editors).mockResolvedValueOnce([{ ...editor, available: false }]);
    render(<AcpPanel />);

    expect(await screen.findByText("not installed")).toBeInTheDocument();
    expect(screen.getByText("Hand off")).toBeDisabled();
  });

  it("shows an error message if loading editors fails", async () => {
    vi.mocked(useCid).mockReturnValue({ selectedSessionId: null } as any);
    vi.mocked(api.acp.editors).mockRejectedValueOnce(new Error("core unreachable"));
    render(<AcpPanel />);

    expect(await screen.findByText(/core unreachable/)).toBeInTheDocument();
  });
});
