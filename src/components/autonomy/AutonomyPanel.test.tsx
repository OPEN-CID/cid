import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, act } from "@testing-library/react";
import { AutonomyPanel } from "./AutonomyPanel";
import { api } from "@/lib/api";
import { useCid } from "@/hooks/useCid";

vi.mock("@/lib/api", () => ({
  api: {
    autonomy: {
      allowlistGet: vi.fn(),
      allowlistSet: vi.fn(),
      allowlistDefault: vi.fn(),
    },
    // RoleProfilesPanel (051 Wave 5.1a) is rendered inside AutonomyPanel now.
    roleProfile: {
      listForRepo: vi.fn().mockResolvedValue([]),
    },
  },
}));

vi.mock("@/hooks/useCid", () => ({
  useCid: vi.fn(),
}));

type AllowlistPayload = {
  allowed_commands: Array<{ pattern: string; description: string; requires_approval: boolean }>;
};

async function flushAsyncWork() {
  await act(async () => {
    for (let i = 0; i < 6; i++) {
      await Promise.resolve();
    }
  });
}

const selectedRepo = {
  repos: [{ id: "repo-1", path: "/tmp/repo" }],
  selectedRepoId: "repo-1",
};

const list = {
  scope_id: "repo-1",
  allowed_commands: [
    { pattern: "^git commit", description: "commit", requires_approval: false },
    { pattern: "^git push", description: "push", requires_approval: true },
  ],
  allowed_paths: [],
  denied_paths: ["/etc"],
  max_tool_calls: null,
};

describe("AutonomyPanel", () => {
  beforeEach(() => {
    vi.mocked(api.autonomy.allowlistGet).mockReset();
    vi.mocked(api.autonomy.allowlistSet).mockReset();
    vi.mocked(api.autonomy.allowlistDefault).mockReset();
    vi.mocked(useCid).mockReturnValue(selectedRepo as any);
  });

  it("prompts to select a repo when none is selected", () => {
    vi.mocked(useCid).mockReturnValue({ repos: [], selectedRepoId: null } as any);
    render(<AutonomyPanel />);
    expect(screen.getByText(/Select a repo channel/)).toBeInTheDocument();
  });

  it("loads and displays the allowlist for the selected repo", async () => {
    vi.mocked(api.autonomy.allowlistGet).mockResolvedValueOnce({ ...list, exists: true });

    render(<AutonomyPanel />);
    await flushAsyncWork();

    expect(screen.getByText("^git commit")).toBeInTheDocument();
    expect(screen.getByText("^git push")).toBeInTheDocument();
    expect(screen.getByText("auto-run")).toBeInTheDocument();
    expect(screen.getByText("ask first")).toBeInTheDocument();
  });

  it("falls back to the default allowlist when none exists yet", async () => {
    vi.mocked(api.autonomy.allowlistGet).mockResolvedValueOnce({ exists: false });
    vi.mocked(api.autonomy.allowlistDefault).mockResolvedValueOnce(list);

    render(<AutonomyPanel />);
    await flushAsyncWork();

    expect(vi.mocked(api.autonomy.allowlistDefault)).toHaveBeenCalledWith("repo-1");
    expect(screen.getByText("^git commit")).toBeInTheDocument();
  });

  it("toggling a command's approval saves the flipped value", async () => {
    vi.mocked(api.autonomy.allowlistGet).mockResolvedValueOnce({ ...list, exists: true });
    vi.mocked(api.autonomy.allowlistSet).mockResolvedValueOnce(list);

    render(<AutonomyPanel />);
    await flushAsyncWork();

    fireEvent.click(screen.getByText("auto-run"));
    await flushAsyncWork();

    const saved = vi.mocked(api.autonomy.allowlistSet).mock.calls[0][0] as AllowlistPayload;
    const commit = saved.allowed_commands.find((c) => c.pattern === "^git commit");
    expect(commit?.requires_approval).toBe(true);
  });

  it("adding a new pattern includes it in the next save call", async () => {
    vi.mocked(api.autonomy.allowlistGet).mockResolvedValueOnce({ ...list, exists: true });
    vi.mocked(api.autonomy.allowlistSet).mockResolvedValueOnce(list);

    render(<AutonomyPanel />);
    await flushAsyncWork();

    fireEvent.change(screen.getByPlaceholderText(/regex pattern/), {
      target: { value: "^npm test" },
    });
    fireEvent.click(screen.getByText("Add pattern"));
    await flushAsyncWork();

    const saved = vi.mocked(api.autonomy.allowlistSet).mock.calls[0][0] as AllowlistPayload;
    expect(saved.allowed_commands.some((c) => c.pattern === "^npm test")).toBe(true);
  });
});
