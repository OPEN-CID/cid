import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { GovernancePanel } from "./GovernancePanel";
import { api } from "@/lib/api";
import { useCid } from "@/hooks/useCid";
import { storedSessionToken } from "../auth/AuthDialog";

vi.mock("@/lib/api", () => ({
  api: {
    governance: { getPolicy: vi.fn(), setPolicy: vi.fn() },
  },
}));

vi.mock("@/lib/dialog", () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}));

vi.mock("@/hooks/useCid", () => ({
  useCid: vi.fn(),
}));

vi.mock("../auth/AuthDialog", () => ({
  storedSessionToken: vi.fn(),
}));

const policy = {
  autonomous_enabled: false,
  autonomous_repos: [],
  session_spend_cap_usd: null,
  daily_spend_cap_usd: null,
  min_role_for_autonomous: "admin",
  min_role_for_plan_approval: "developer",
  min_role_for_merge: "developer",
};

describe("GovernancePanel", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.mocked(useCid).mockReturnValue({
      repos: [{ id: "repo-1", name: "cid", path: "/tmp/cid" }],
    } as any);
    vi.mocked(api.governance.getPolicy).mockReset().mockResolvedValue({ ...policy });
    vi.mocked(api.governance.setPolicy).mockReset().mockResolvedValue({ ...policy, autonomous_enabled: true });
    vi.mocked(storedSessionToken).mockReset().mockReturnValue(null);
  });

  it("loads and shows the current policy", async () => {
    render(<GovernancePanel />);
    expect(await screen.findByText("Workspace governance")).toBeInTheDocument();
  });

  it("refuses to save without a sign-in instead of failing silently", async () => {
    render(<GovernancePanel />);
    await screen.findByText("Workspace governance");

    fireEvent.click(screen.getByLabelText(/Autonomous mode permitted/));
    await waitFor(() =>
      expect(api.governance.setPolicy).not.toHaveBeenCalled()
    );
  });

  it("sends the token and the updated policy when signed in", async () => {
    vi.mocked(storedSessionToken).mockReturnValue("tok");
    render(<GovernancePanel />);
    await screen.findByText("Workspace governance");

    fireEvent.click(screen.getByLabelText(/Autonomous mode permitted/));
    await waitFor(() =>
      expect(api.governance.setPolicy).toHaveBeenCalledWith("tok", expect.objectContaining({ autonomous_enabled: true }))
    );
  });

  it("rejects invalid spend caps client-side", async () => {
    vi.mocked(storedSessionToken).mockReturnValue("tok");
    render(<GovernancePanel />);
    await screen.findByText("Workspace governance");

    fireEvent.change(screen.getByPlaceholderText(/Per session/), { target: { value: "-5" } });
    fireEvent.click(screen.getByText("Save"));

    await waitFor(() => expect(api.governance.setPolicy).not.toHaveBeenCalled());
  });
});
