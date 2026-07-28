import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { SkillsPanel } from "./SkillsPanel";
import { api } from "@/lib/api";
import { useCid } from "@/hooks/useCid";

// 051-Editor-Excellence-Roadmap.md Wave 5.4.

vi.mock("@/lib/api", () => ({
  api: {
    skills: { list: vi.fn(), save: vi.fn() },
    repo: { agentsMd: vi.fn() },
    call: vi.fn(),
  },
}));

vi.mock("@/lib/dialog", () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}));

vi.mock("@/hooks/useCid", () => ({
  useCid: vi.fn(),
}));

const repo = { id: "repo-1", path: "/tmp/repo" };
const skill = {
  id: "skill-1",
  name: "commit-convention",
  content: "Use conventional commits.",
  scope: "workspace",
  created_at: "2026-07-27T00:00:00Z",
  updated_at: "2026-07-27T00:00:00Z",
};

describe("SkillsPanel", () => {
  beforeEach(() => {
    vi.mocked(api.skills.list).mockReset();
    vi.mocked(api.skills.save).mockReset();
    vi.mocked(api.repo.agentsMd).mockReset();
    vi.mocked(api.call).mockReset();
    vi.mocked(useCid).mockReturnValue({ repos: [repo], selectedRepoId: "repo-1" } as any);
    vi.mocked(api.skills.list).mockResolvedValue([]);
    vi.mocked(api.repo.agentsMd).mockResolvedValue({ content: null });
  });

  it("lists existing skills", async () => {
    vi.mocked(api.skills.list).mockResolvedValueOnce([skill]);
    render(<SkillsPanel />);
    expect(await screen.findByText("commit-convention")).toBeInTheDocument();
  });

  it("shows a create prompt when the repo has no AGENTS.md", async () => {
    render(<SkillsPanel />);
    expect(await screen.findByText("Create")).toBeInTheDocument();
    expect(screen.getByText(/No AGENTS.md found/)).toBeInTheDocument();
  });

  it("shows existing AGENTS.md content with an Edit action", async () => {
    vi.mocked(api.repo.agentsMd).mockResolvedValueOnce({ content: "Run tests before committing." });
    render(<SkillsPanel />);
    expect(await screen.findByText("Run tests before committing.")).toBeInTheDocument();
    expect(screen.getByText("Edit")).toBeInTheDocument();
  });

  it("editing and saving AGENTS.md writes back to the real file", async () => {
    vi.mocked(api.repo.agentsMd).mockResolvedValueOnce({ content: "Old content." });
    vi.mocked(api.call).mockResolvedValueOnce({ ok: true });
    render(<SkillsPanel />);
    fireEvent.click(await screen.findByText("Edit"));

    const textarea = screen.getByDisplayValue("Old content.");
    fireEvent.change(textarea, { target: { value: "New content." } });
    // Two "Save" buttons render simultaneously while editing: the header
    // toggle (label flips to "Save" in edit mode) and the explicit form
    // button — click the explicit one.
    fireEvent.click(screen.getAllByText("Save")[1]);

    await waitFor(() =>
      expect(api.call).toHaveBeenCalledWith("repo.agents_md.write", { path: "/tmp/repo", content: "New content." })
    );
    expect(await screen.findByText("New content.")).toBeInTheDocument();
  });

  it("adding a skill saves it scoped to the workspace by default and reloads", async () => {
    vi.mocked(api.skills.save).mockResolvedValueOnce({ ok: true });
    vi.mocked(api.skills.list).mockResolvedValueOnce([]).mockResolvedValueOnce([skill]);
    render(<SkillsPanel />);
    await screen.findByText("Create");

    fireEvent.change(screen.getByPlaceholderText(/Skill name/), { target: { value: "commit-convention" } });
    fireEvent.change(screen.getByPlaceholderText(/Skill content/), { target: { value: "Use conventional commits." } });
    fireEvent.click(screen.getByText("Save Skill"));

    await waitFor(() =>
      expect(api.skills.save).toHaveBeenCalledWith(
        expect.objectContaining({ name: "commit-convention", scope: "workspace", scope_id: null })
      )
    );
    expect(await screen.findByText("commit-convention")).toBeInTheDocument();
  });
});
