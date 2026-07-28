import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { DecisionsPanel } from "./DecisionsPanel";
import { api } from "@/lib/api";
import { useCid } from "@/hooks/useCid";

// 051-Editor-Excellence-Roadmap.md Wave 5.1c: decisions.* and deployment.*
// had real backends, zero UI — this pins the new Mission-thread tab.

vi.mock("@/lib/api", () => ({
  api: {
    decisions: { list: vi.fn(), forMission: vi.fn() },
    deployment: { record: vi.fn(), list: vi.fn() },
  },
}));

vi.mock("@/hooks/useCid", () => ({
  useCid: vi.fn(),
}));

const state = {
  missions: [{ id: "mission-1", repo_channel_id: "repo-1" }],
  repos: [{ id: "repo-1", path: "/tmp/repo" }],
  selectedMissionId: "mission-1",
};

describe("DecisionsPanel", () => {
  beforeEach(() => {
    vi.mocked(api.decisions.list).mockReset();
    vi.mocked(api.decisions.forMission).mockReset();
    vi.mocked(api.deployment.record).mockReset();
    vi.mocked(api.deployment.list).mockReset();
    vi.mocked(useCid).mockReturnValue(state as any);
    vi.mocked(api.decisions.forMission).mockResolvedValue([]);
    vi.mocked(api.deployment.list).mockResolvedValue([]);
  });

  it("prompts to select a mission when none is selected", () => {
    vi.mocked(useCid).mockReturnValue({ missions: [], repos: [], selectedMissionId: null } as any);
    render(<DecisionsPanel />);
    expect(screen.getByText(/Select a mission/)).toBeInTheDocument();
  });

  it("shows ADRs relevant to the mission", async () => {
    vi.mocked(api.decisions.forMission).mockResolvedValueOnce([
      { number: "0011", title: "Windows sandbox limits", path: "docs/adr/0011-windows-sandbox.md" },
    ]);
    render(<DecisionsPanel />);

    expect(await screen.findByText(/ADR 0011: Windows sandbox limits/)).toBeInTheDocument();
    expect(api.decisions.forMission).toHaveBeenCalledWith("mission-1");
  });

  it("Show all repo ADRs lists every ADR in the repo", async () => {
    vi.mocked(api.decisions.list).mockResolvedValueOnce([
      { number: "0001", title: "Storage engine", path: "docs/adr/0001-storage.md", status: "Accepted" },
    ]);
    render(<DecisionsPanel />);
    await waitFor(() => expect(api.decisions.forMission).toHaveBeenCalled());

    fireEvent.click(screen.getByText("Show all repo ADRs"));

    await waitFor(() => expect(api.decisions.list).toHaveBeenCalledWith("/tmp/repo"));
    expect(await screen.findByText(/ADR 0001: Storage engine/)).toBeInTheDocument();
  });

  it("lists deployments for the mission", async () => {
    vi.mocked(api.deployment.list).mockResolvedValueOnce([
      {
        id: "dep-1",
        mission_id: "mission-1",
        environment: "production",
        commit_or_tag: "abc1234",
        source: "manual",
        deployed_at: "2026-07-27T00:00:00Z",
      },
    ]);
    render(<DecisionsPanel />);

    expect(await screen.findByText("production")).toBeInTheDocument();
    expect(screen.getByText("abc1234")).toBeInTheDocument();
  });

  it("recording a deployment sends the mission-scoped payload and reloads", async () => {
    vi.mocked(api.deployment.list).mockResolvedValueOnce([]).mockResolvedValueOnce([
      {
        id: "dep-1",
        mission_id: "mission-1",
        environment: "staging",
        commit_or_tag: "def5678",
        source: "manual",
        deployed_at: "2026-07-27T00:00:00Z",
      },
    ]);
    vi.mocked(api.deployment.record).mockResolvedValueOnce({});
    render(<DecisionsPanel />);
    await waitFor(() => expect(api.deployment.list).toHaveBeenCalled());

    fireEvent.click(screen.getByText("Record"));
    fireEvent.change(screen.getByPlaceholderText(/Environment/), { target: { value: "staging" } });
    fireEvent.change(screen.getByPlaceholderText(/Commit SHA/), { target: { value: "def5678" } });
    fireEvent.click(screen.getByText("Save"));

    await waitFor(() =>
      expect(api.deployment.record).toHaveBeenCalledWith(
        expect.objectContaining({ mission_id: "mission-1", environment: "staging", commit_or_tag: "def5678" })
      )
    );
    expect(await screen.findByText("staging")).toBeInTheDocument();
  });
});
