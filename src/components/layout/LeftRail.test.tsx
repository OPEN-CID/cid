import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { LeftRail } from "./LeftRail";

vi.mock("@/lib/api", () => ({
  api: {
    repo: { list: vi.fn().mockResolvedValue([]), connect: vi.fn() },
  },
}));

vi.mock("@/hooks/useCid", () => ({
  useCid: () => ({
    repos: [],
    selectedRepoId: null,
    missions: [],
    selectedMissionId: null,
    selectRepo: vi.fn(),
    selectMission: vi.fn(),
    loadRepos: vi.fn(),
  }),
}));

describe("LeftRail", () => {
  it("renders CID branding and empty state", () => {
    render(<LeftRail />);
    expect(screen.getByText("CID")).toBeInTheDocument();
    expect(screen.getByText(/No repos connected/)).toBeInTheDocument();
  });
});
