import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { TerminalPane } from "./TerminalPane";
import { api } from "@/lib/api";
import { useCid } from "@/hooks/useCid";

// 051-Editor-Excellence-Roadmap.md Wave 5.4. xterm.js does real canvas
// rendering that jsdom can't do — the Terminal/FitAddon classes are faked
// with minimal stand-ins that record calls, so what's actually under test is
// TerminalPane's own PTY wiring (create → write → notification → cancel),
// not xterm's rendering.

const writelnCalls: string[] = [];
const writeCalls: string[] = [];
let onDataHandler: ((data: string) => void) | null = null;
let disposeCalled = false;

vi.mock("@xterm/xterm", () => ({
  Terminal: vi.fn().mockImplementation(() => ({
    open: vi.fn(),
    loadAddon: vi.fn(),
    writeln: vi.fn((s: string) => writelnCalls.push(s)),
    write: vi.fn((s: string) => writeCalls.push(s)),
    onData: vi.fn((handler: (data: string) => void) => {
      onDataHandler = handler;
    }),
    dispose: vi.fn(() => {
      disposeCalled = true;
    }),
  })),
}));

vi.mock("@xterm/addon-fit", () => ({
  FitAddon: vi.fn().mockImplementation(() => ({
    fit: vi.fn(),
    proposeDimensions: vi.fn(() => ({ cols: 80, rows: 24 })),
  })),
}));

let notificationHandler: ((notif: unknown) => void) | null = null;

vi.mock("@/lib/api", () => ({
  api: {
    pty: { create: vi.fn(), write: vi.fn(), resize: vi.fn() },
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

describe("TerminalPane", () => {
  beforeEach(() => {
    writelnCalls.length = 0;
    writeCalls.length = 0;
    onDataHandler = null;
    notificationHandler = null;
    disposeCalled = false;
    vi.mocked(api.pty.create).mockReset();
    vi.mocked(api.pty.write).mockReset();
    vi.mocked(api.pty.resize).mockReset();
    vi.mocked(api.pty.write).mockResolvedValue(undefined);
  });

  it("prompts to select a mission when none is selected, and creates no PTY", () => {
    vi.mocked(useCid).mockReturnValue({ selectedMissionId: null } as any);
    render(<TerminalPane />);
    expect(screen.getByText("Select a mission to open terminal")).toBeInTheDocument();
    expect(api.pty.create).not.toHaveBeenCalled();
  });

  it("creates a PTY for the selected mission and shows Connected", async () => {
    vi.mocked(useCid).mockReturnValue({ selectedMissionId: "mission-1" } as any);
    vi.mocked(api.pty.create).mockResolvedValueOnce({ id: "pty-abc12345" });
    render(<TerminalPane />);

    await waitFor(() => expect(api.pty.create).toHaveBeenCalledWith("mission-1", 120, 30));
    expect(await screen.findByText("Connected")).toBeInTheDocument();
    expect(screen.getByText(/pty-abc1/)).toBeInTheDocument();
  });

  it("typing into the terminal writes to the freshly-created PTY, not a stale id", async () => {
    vi.mocked(useCid).mockReturnValue({ selectedMissionId: "mission-1" } as any);
    vi.mocked(api.pty.create).mockResolvedValueOnce({ id: "pty-abc12345" });
    render(<TerminalPane />);
    await waitFor(() => expect(onDataHandler).not.toBeNull());

    onDataHandler?.("ls -la\n");

    expect(api.pty.write).toHaveBeenCalledWith("pty-abc12345", "ls -la\n");
  });

  it("routes pty.output notifications for this PTY into the terminal", async () => {
    vi.mocked(useCid).mockReturnValue({ selectedMissionId: "mission-1" } as any);
    vi.mocked(api.pty.create).mockResolvedValueOnce({ id: "pty-abc12345" });
    render(<TerminalPane />);
    await waitFor(() => expect(notificationHandler).not.toBeNull());

    notificationHandler?.({ method: "pty.output", params: { pty_id: "pty-abc12345", data: "hello\r\n" } });

    expect(writeCalls).toContain("hello\r\n");
  });

  it("ignores pty.output notifications for a different PTY", async () => {
    vi.mocked(useCid).mockReturnValue({ selectedMissionId: "mission-1" } as any);
    vi.mocked(api.pty.create).mockResolvedValueOnce({ id: "pty-abc12345" });
    render(<TerminalPane />);
    await waitFor(() => expect(notificationHandler).not.toBeNull());

    notificationHandler?.({ method: "pty.output", params: { pty_id: "some-other-pty", data: "should not appear" } });

    expect(writeCalls).not.toContain("should not appear");
  });

  it("writes a failure message if PTY creation fails", async () => {
    vi.mocked(useCid).mockReturnValue({ selectedMissionId: "mission-1" } as any);
    vi.mocked(api.pty.create).mockRejectedValueOnce(new Error("core unreachable"));
    render(<TerminalPane />);

    await waitFor(() => expect(writelnCalls.some((s) => s.includes("Failed to create PTY"))).toBe(true));
  });

  it("disposes the xterm instance on unmount", async () => {
    // The terminal container (and so the xterm instance) only mounts once a
    // mission is selected — the "select a mission" early return never
    // renders the ref'd container at all.
    vi.mocked(useCid).mockReturnValue({ selectedMissionId: "mission-1" } as any);
    vi.mocked(api.pty.create).mockResolvedValueOnce({ id: "pty-abc12345" });
    const { unmount } = render(<TerminalPane />);
    await waitFor(() => expect(api.pty.create).toHaveBeenCalled());

    unmount();

    expect(disposeCalled).toBe(true);
  });
});
