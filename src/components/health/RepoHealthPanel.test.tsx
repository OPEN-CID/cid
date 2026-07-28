import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { RepoHealthPanel } from "./RepoHealthPanel";
import { api } from "@/lib/api";
import { useCid } from "@/hooks/useCid";

// 051-Editor-Excellence-Roadmap.md Wave 5.1b: test-impact, docs, and blame
// were real backend features with zero UI — these pin the new tabs.

vi.mock("@/lib/api", () => ({
  api: {
    repoHealth: { scan: vi.fn() },
    semanticEngine: {
      testImpactForSymbol: vi.fn(),
      testImpactEntries: vi.fn(),
      docsForSymbol: vi.fn(),
      docsStale: vi.fn(),
      gitBlame: vi.fn(),
      loadBlame: vi.fn(),
    },
  },
}));

vi.mock("@/hooks/useCid", () => ({
  useCid: vi.fn(),
}));

const selectedRepo = {
  repos: [{ id: "repo-1", path: "/tmp/repo" }],
  selectedRepoId: "repo-1",
};

const report = {
  modules: [],
  total_fns: 10,
  total_tests: 4,
  untested_modules: [],
  duplicate_test_groups: [],
};

describe("RepoHealthPanel", () => {
  beforeEach(() => {
    vi.mocked(api.repoHealth.scan).mockReset();
    vi.mocked(api.semanticEngine.testImpactForSymbol).mockReset();
    vi.mocked(api.semanticEngine.testImpactEntries).mockReset();
    vi.mocked(api.semanticEngine.docsForSymbol).mockReset();
    vi.mocked(api.semanticEngine.docsStale).mockReset();
    vi.mocked(api.semanticEngine.gitBlame).mockReset();
    vi.mocked(api.semanticEngine.loadBlame).mockReset();
    vi.mocked(useCid).mockReturnValue(selectedRepo as any);
    vi.mocked(api.repoHealth.scan).mockResolvedValue(report);
  });

  it("defaults to the Tests tab and shows the scan report", async () => {
    render(<RepoHealthPanel />);
    expect(await screen.findByText("No untested modules or duplicate tests found.")).toBeInTheDocument();
  });

  it("Test Impact tab looks up covering tests for a symbol", async () => {
    vi.mocked(api.semanticEngine.testImpactForSymbol).mockResolvedValueOnce({
      symbol: "run_migrations",
      covering_tests: ["a_fresh_database_is_stamped_at_the_latest_migration_version"],
    });
    render(<RepoHealthPanel />);
    await screen.findByText("No untested modules or duplicate tests found.");

    fireEvent.click(screen.getByText("test impact"));
    fireEvent.change(screen.getByPlaceholderText(/Symbol name/), { target: { value: "run_migrations" } });
    fireEvent.keyDown(screen.getByPlaceholderText(/Symbol name/), { key: "Enter" });

    await waitFor(() => expect(api.semanticEngine.testImpactForSymbol).toHaveBeenCalledWith("/tmp/repo", "run_migrations"));
    expect(await screen.findByText("a_fresh_database_is_stamped_at_the_latest_migration_version")).toBeInTheDocument();
  });

  it("Docs tab finds stale docs", async () => {
    vi.mocked(api.semanticEngine.docsStale).mockResolvedValueOnce([
      { doc_path: "docs/012-Semantic-Editing.md", missing_symbols: ["OldSymbol"] },
    ]);
    render(<RepoHealthPanel />);
    await screen.findByText("No untested modules or duplicate tests found.");

    fireEvent.click(screen.getByText("docs"));
    fireEvent.click(screen.getByText("Find stale docs"));

    await waitFor(() => expect(api.semanticEngine.docsStale).toHaveBeenCalledWith("/tmp/repo"));
    expect(await screen.findByText("docs/012-Semantic-Editing.md")).toBeInTheDocument();
  });

  it("Blame tab loads blame and caches it into the semantic engine", async () => {
    const blames = [
      { file_path: "src/lib.rs", line: 1, author: "a", email: "a@x.com", commit_hash: "abc123", commit_date: "2026-01-01", commit_summary: "init" },
    ];
    vi.mocked(api.semanticEngine.gitBlame).mockResolvedValueOnce(blames);
    render(<RepoHealthPanel />);
    await screen.findByText("No untested modules or duplicate tests found.");

    fireEvent.click(screen.getByText("blame"));
    fireEvent.change(screen.getByPlaceholderText("src/lib.rs"), { target: { value: "src/lib.rs" } });
    fireEvent.keyDown(screen.getByPlaceholderText("src/lib.rs"), { key: "Enter" });

    await waitFor(() => expect(api.semanticEngine.gitBlame).toHaveBeenCalledWith("/tmp/repo", "src/lib.rs"));
    await waitFor(() => expect(api.semanticEngine.loadBlame).toHaveBeenCalledWith("/tmp/repo", "src/lib.rs", blames));
    expect(await screen.findByText("init")).toBeInTheDocument();
  });
});
