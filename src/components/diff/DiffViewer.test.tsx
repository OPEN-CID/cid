import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { DiffViewer } from "./DiffViewer";
import { api } from "@/lib/api";
import { useCid } from "@/hooks/useCid";

vi.mock("@/lib/api", () => ({
  api: {
    call: vi.fn(),
    onNotification: () => () => {},
    git: { diff: vi.fn() },
  },
}));

vi.mock("@/hooks/useCid", () => ({
  useCid: vi.fn(),
}));

const missionSelected = {
  selectedMissionId: "mission-1",
  missions: [{ id: "mission-1", title: "Test Mission", worktree_path: "/tmp/repo" }],
  repos: [{ id: "repo-1", path: "/tmp/repo" }],
};

describe("DiffViewer", () => {
  beforeEach(() => {
    vi.mocked(api.git.diff).mockReset();
    vi.mocked(api.call).mockReset();
    vi.mocked(useCid).mockReturnValue(missionSelected as any);
  });

  it("shows a prompt to select a mission when none is selected", () => {
    vi.mocked(useCid).mockReturnValue({
      selectedMissionId: null,
      missions: [],
      repos: [],
    } as any);

    render(<DiffViewer />);

    expect(screen.getByText(/Select a mission to view diff/)).toBeInTheDocument();
  });

  it("shows a clean-working-tree message when there are no changes", async () => {
    vi.mocked(api.git.diff).mockResolvedValueOnce([]);

    render(<DiffViewer />);

    expect(await screen.findByText(/No changes detected/)).toBeInTheDocument();
  });

  it("displays diff files with per-hunk accept/reject controls", async () => {
    vi.mocked(api.git.diff).mockResolvedValueOnce([
      {
        path: "src/main.ts",
        status: "M",
        additions: 5,
        deletions: 2,
        hunks: [
          {
            id: "hunk-1",
            content: "+  console.log('Hello');\n-  // TODO: implement",
            header: "@@ -10,5 +10,6 @@",
            old_start: 10,
            old_lines: 5,
            new_start: 10,
            new_lines: 6,
          },
        ],
      },
    ]);

    render(<DiffViewer />);

    expect(await screen.findByText(/src\/main\.ts/)).toBeInTheDocument();
    expect(screen.getByText("+5")).toBeInTheDocument();
    expect(screen.getByText("-2")).toBeInTheDocument();
    expect(screen.getByText(/console\.log/)).toBeInTheDocument();
  });

  it("reject hunk calls git.hunk.apply with this hunk's own header and content", async () => {
    vi.mocked(api.git.diff).mockResolvedValue([
      {
        path: "src/main.ts",
        status: "M",
        additions: 1,
        deletions: 1,
        hunks: [
          {
            id: "hunk-1",
            content: "+added line\n-removed line",
            header: "@@ -1,1 +1,1 @@",
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 1,
          },
        ],
      },
    ]);
    vi.mocked(api.call).mockResolvedValueOnce({ ok: true });

    render(<DiffViewer />);
    await screen.findByText(/src\/main\.ts/);

    fireEvent.click(screen.getByText("Reject hunk"));

    expect(vi.mocked(api.call)).toHaveBeenCalledWith("git.hunk.apply", {
      repo_path: "/tmp/repo",
      file_path: "src/main.ts",
      hunk_id: "hunk-1",
      action: "reject",
      header: "@@ -1,1 +1,1 @@",
      content: "+added line\n-removed line",
    });
  });

  it("toggles between unified and split view", async () => {
    vi.mocked(api.git.diff).mockResolvedValueOnce([]);

    render(<DiffViewer />);
    await screen.findByText(/No changes detected/);

    expect(screen.getByRole("button", { name: /Unified/ })).toHaveClass("bg-accent");
    expect(screen.getByRole("button", { name: /Split/ })).not.toHaveClass("bg-accent");

    fireEvent.click(screen.getByRole("button", { name: /Split/ }));

    expect(screen.getByRole("button", { name: /Split/ })).toHaveClass("bg-accent");
    expect(screen.getByRole("button", { name: /Unified/ })).not.toHaveClass("bg-accent");
  });
});
