import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { RoleProfilesPanel } from "./RoleProfilesPanel";
import { DialogHost } from "@/components/ui/DialogHost";
import { api } from "@/lib/api";
import { useCid } from "@/hooks/useCid";
import { useDialogStore } from "@/lib/dialog";

// 051-Editor-Excellence-Roadmap.md Wave 5.1a: role_profile.* had no UI at
// all before this — these pin real create/edit/delete round trips.

vi.mock("@/lib/api", () => ({
  api: {
    roleProfile: {
      listForRepo: vi.fn(),
      create: vi.fn(),
      update: vi.fn(),
      delete: vi.fn(),
    },
  },
}));

vi.mock("@/hooks/useCid", () => ({
  useCid: vi.fn(),
}));

const selectedRepo = {
  repos: [{ id: "repo-1", workspace_id: "ws-1", path: "/tmp/repo" }],
  selectedRepoId: "repo-1",
};

const profile = {
  id: "profile-1",
  name: "Security Reviewer",
  description: "Read-only audit pass",
  scope: "repo",
  scope_id: "repo-1",
  system_prompt: "You review for security issues only.",
  model_provider: null,
  model_id: null,
  allowed_tools: ["read_file"],
};

function renderPanel() {
  return render(
    <>
      <RoleProfilesPanel />
      <DialogHost />
    </>
  );
}

describe("RoleProfilesPanel", () => {
  beforeEach(() => {
    vi.mocked(api.roleProfile.listForRepo).mockReset();
    vi.mocked(api.roleProfile.create).mockReset();
    vi.mocked(api.roleProfile.update).mockReset();
    vi.mocked(api.roleProfile.delete).mockReset();
    vi.mocked(useCid).mockReturnValue(selectedRepo as any);
    useDialogStore.setState({ toasts: [], confirmRequest: null, infoRequest: null });
  });

  it("lists existing profiles scoped to the selected repo", async () => {
    vi.mocked(api.roleProfile.listForRepo).mockResolvedValueOnce([profile]);
    renderPanel();

    expect(await screen.findByText("Security Reviewer")).toBeInTheDocument();
    expect(api.roleProfile.listForRepo).toHaveBeenCalledWith("ws-1", "repo-1");
  });

  it("shows an empty state when no profiles exist", async () => {
    vi.mocked(api.roleProfile.listForRepo).mockResolvedValueOnce([]);
    renderPanel();

    expect(await screen.findByText("No role profiles configured.")).toBeInTheDocument();
  });

  it("creating a profile sends the scoped payload and reloads", async () => {
    vi.mocked(api.roleProfile.listForRepo).mockResolvedValueOnce([]).mockResolvedValueOnce([profile]);
    vi.mocked(api.roleProfile.create).mockResolvedValueOnce(profile);
    renderPanel();
    await screen.findByText("No role profiles configured.");

    fireEvent.click(screen.getByText("New profile"));
    fireEvent.change(screen.getByPlaceholderText(/Name/), { target: { value: "Security Reviewer" } });
    fireEvent.click(screen.getByText("read_file"));
    fireEvent.click(screen.getByText("Create"));

    await waitFor(() =>
      expect(api.roleProfile.create).toHaveBeenCalledWith(
        expect.objectContaining({ name: "Security Reviewer", scope_id: "repo-1", allowed_tools: ["read_file"] })
      )
    );
    expect(await screen.findByText("Security Reviewer")).toBeInTheDocument();
  });

  it("deleting a profile asks for confirmation first", async () => {
    vi.mocked(api.roleProfile.listForRepo).mockResolvedValueOnce([profile]);
    renderPanel();
    await screen.findByText("Security Reviewer");

    fireEvent.click(screen.getByLabelText("Delete Security Reviewer"));
    expect(await screen.findByText(/Delete role profile/)).toBeInTheDocument();
    expect(api.roleProfile.delete).not.toHaveBeenCalled();

    fireEvent.click(screen.getByText("Confirm"));
    await waitFor(() => expect(api.roleProfile.delete).toHaveBeenCalledWith("profile-1"));
  });
});
