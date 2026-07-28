import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { TeamIntegrationsPanel } from "./TeamIntegrationsPanel";
import { DialogHost } from "@/components/ui/DialogHost";
import { api } from "@/lib/api";
import { useDialogStore } from "@/lib/dialog";

// 051-Editor-Excellence-Roadmap.md Wave 5.1d: slack.configure/config.get and
// the Teams equivalents had no UI at all.

vi.mock("@/lib/api", () => ({
  api: {
    slack: { configGet: vi.fn(), configure: vi.fn() },
    teams: { configGet: vi.fn(), configure: vi.fn() },
  },
}));

function renderPanel() {
  return render(
    <>
      <TeamIntegrationsPanel />
      <DialogHost />
    </>
  );
}

describe("TeamIntegrationsPanel", () => {
  beforeEach(() => {
    vi.mocked(api.slack.configGet).mockReset();
    vi.mocked(api.slack.configure).mockReset();
    vi.mocked(api.teams.configGet).mockReset();
    vi.mocked(api.teams.configure).mockReset();
    vi.mocked(api.slack.configGet).mockResolvedValue({ configured: false });
    vi.mocked(api.teams.configGet).mockResolvedValue({ configured: false });
    useDialogStore.setState({ toasts: [], confirmRequest: null, infoRequest: null });
  });

  it("loads existing Slack and Teams config on mount", async () => {
    vi.mocked(api.slack.configGet).mockResolvedValueOnce({
      configured: true,
      webhook_url: "https://hooks.slack.example/abc",
      enabled: true,
      allowed_channels: ["general"],
      trigger_prefix: "/cid",
    });
    renderPanel();

    expect(await screen.findByDisplayValue("https://hooks.slack.example/abc")).toBeInTheDocument();
    expect(api.slack.configGet).toHaveBeenCalledWith("default");
    expect(api.teams.configGet).toHaveBeenCalledWith("default");
  });

  it("saving Slack config sends the workspace-scoped payload", async () => {
    vi.mocked(api.slack.configure).mockResolvedValueOnce({});
    renderPanel();
    await waitFor(() => expect(api.slack.configGet).toHaveBeenCalled());

    fireEvent.change(screen.getAllByPlaceholderText("Webhook URL")[0], { target: { value: "https://hooks.slack.example/new" } });
    fireEvent.click(screen.getByText("Save Slack config"));

    await waitFor(() =>
      expect(api.slack.configure).toHaveBeenCalledWith(
        expect.objectContaining({ workspace_id: "default", webhook_url: "https://hooks.slack.example/new" })
      )
    );
    expect(await screen.findByText("Slack configuration saved")).toBeInTheDocument();
  });

  it("saving Teams config parses comma-separated lists", async () => {
    vi.mocked(api.teams.configure).mockResolvedValueOnce({});
    renderPanel();
    await waitFor(() => expect(api.teams.configGet).toHaveBeenCalled());

    const teamsWebhook = screen.getAllByPlaceholderText("Webhook URL")[1];
    fireEvent.change(teamsWebhook, { target: { value: "https://teams.example/hook" } });
    fireEvent.change(screen.getByPlaceholderText("Allowed teams (comma-separated)"), {
      target: { value: "Engineering, Platform" },
    });
    fireEvent.click(screen.getByText("Save Teams config"));

    await waitFor(() =>
      expect(api.teams.configure).toHaveBeenCalledWith(
        expect.objectContaining({
          workspace_id: "default",
          webhook_url: "https://teams.example/hook",
          allowed_teams: ["Engineering", "Platform"],
        })
      )
    );
  });
});
