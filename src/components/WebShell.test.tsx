import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { ConnectionBanner, AccessControlPanel, HealthDashboard } from "./WebShell";
import { api } from "@/lib/api";
import { useCid } from "@/hooks/useCid";

// 051-Editor-Excellence-Roadmap.md Wave 5.4. AccessControlPanel is
// security-relevant — it's the surface that warns a user their Core is
// reachable beyond localhost with no token, and (since the token work) the
// surface that lets a browser present one at all.
//
// The mock mirrors the real CidApiClient surface these components use. A mock
// that invents its own shape is how the model-picker `enabled`/`available` bug
// shipped 100% broken with passing tests (docs/053 §1) — every method here
// exists on the real client with the same name.
vi.mock("@/lib/api", () => ({
  api: {
    connect: vi.fn(),
    health: vi.fn(),
    hasAuthToken: vi.fn(() => false),
    setAuthToken: vi.fn(),
    socketUrl: "ws://127.0.0.1:5919/ws",
  },
}));

vi.mock("@/hooks/useCid", () => ({
  useCid: vi.fn(),
}));

const HEALTH_LOCAL = {
  loopback_only: true,
  auth_required: false,
  connected_clients: 1,
  uptime_seconds: 65,
  version: "0.1.0",
};

beforeEach(() => {
  vi.mocked(api.connect).mockReset().mockResolvedValue(undefined);
  vi.mocked(api.setAuthToken).mockReset();
  vi.mocked(api.hasAuthToken).mockReset().mockReturnValue(false);
  vi.mocked(api.health).mockReset().mockResolvedValue(HEALTH_LOCAL);
  vi.mocked(useCid).mockReturnValue({ connected: false, missions: [] } as any);
});

describe("ConnectionBanner", () => {
  it("renders nothing when connected", () => {
    vi.mocked(useCid).mockReturnValue({ connected: true, missions: [] } as any);
    const { container } = render(<ConnectionBanner />);
    expect(container).toBeEmptyDOMElement();
  });

  it("shows an offline banner when disconnected", () => {
    render(<ConnectionBanner />);
    expect(screen.getByText("Core offline")).toBeInTheDocument();
  });

  it("Reconnect calls api.connect", async () => {
    render(<ConnectionBanner />);
    fireEvent.click(screen.getByText("Reconnect"));
    await waitFor(() => expect(api.connect).toHaveBeenCalled());
  });

  it("expanding shows the startup command reference and the real socket target", () => {
    render(<ConnectionBanner />);
    fireEvent.click(screen.getByLabelText("Expand connection details"));

    expect(screen.getByText("cargo run -p cid-core -- --port 5919")).toBeInTheDocument();
    // Read from the client rather than hardcoded, so a hosted wss:// deployment
    // does not show a localhost ws:// address.
    expect(screen.getByText("ws://127.0.0.1:5919/ws")).toBeInTheDocument();
  });

  it("does not ask for a token when Core does not require one", async () => {
    render(<ConnectionBanner />);
    await waitFor(() => expect(api.health).toHaveBeenCalled());
    expect(screen.queryByLabelText(/access token/i)).not.toBeInTheDocument();
  });

  it("prompts for a token when Core requires one, then saves and reconnects", async () => {
    vi.mocked(api.health).mockResolvedValue({ ...HEALTH_LOCAL, auth_required: true });
    render(<ConnectionBanner />);

    const input = await screen.findByLabelText("Core requires an access token:");
    fireEvent.change(input, { target: { value: "  pasted-token  " } });
    fireEvent.click(screen.getByText("Save and reconnect"));

    // Trimmed by the client, and a reconnect is attempted immediately so the
    // user is not left staring at an offline banner after a correct paste.
    await waitFor(() => expect(api.setAuthToken).toHaveBeenCalledWith("  pasted-token  "));
    await waitFor(() => expect(api.connect).toHaveBeenCalled());
  });

  it("says so when a stored token was the thing Core rejected", async () => {
    vi.mocked(api.hasAuthToken).mockReturnValue(true);
    vi.mocked(api.health).mockResolvedValue({ ...HEALTH_LOCAL, auth_required: true });
    render(<ConnectionBanner />);

    expect(
      await screen.findByText("Core rejected the saved access token — paste a current one:"),
    ).toBeInTheDocument();
  });

  it("ignores an empty submission rather than storing a blank token", async () => {
    vi.mocked(api.health).mockResolvedValue({ ...HEALTH_LOCAL, auth_required: true });
    render(<ConnectionBanner />);

    fireEvent.click(await screen.findByText("Save and reconnect"));

    expect(api.setAuthToken).not.toHaveBeenCalled();
  });
});

describe("AccessControlPanel", () => {
  it("shows loopback-only and no-warning state by default", async () => {
    render(<AccessControlPanel />);
    expect(await screen.findByText("loopback only")).toBeInTheDocument();
    expect(screen.queryByText(/reachable beyond localhost with no token/)).not.toBeInTheDocument();
  });

  it("warns when reachable remotely with no auth token", async () => {
    vi.mocked(api.health).mockResolvedValue({
      ...HEALTH_LOCAL,
      loopback_only: false,
      auth_required: false,
      connected_clients: 3,
    });
    render(<AccessControlPanel />);

    expect(await screen.findByText("remote")).toBeInTheDocument();
    expect(screen.getByText(/reachable beyond localhost with no token/)).toBeInTheDocument();
  });

  it("does not warn when reachable remotely but a token is required", async () => {
    vi.mocked(api.health).mockResolvedValue({
      ...HEALTH_LOCAL,
      loopback_only: false,
      auth_required: true,
      connected_clients: 3,
    });
    render(<AccessControlPanel />);

    expect(await screen.findByText("required")).toBeInTheDocument();
    expect(screen.queryByText(/reachable beyond localhost with no token/)).not.toBeInTheDocument();
  });

  it("offers token management only when Core requires a token", async () => {
    render(<AccessControlPanel />);
    await screen.findByText("loopback only");
    expect(screen.queryByLabelText("This browser's access token")).not.toBeInTheDocument();

    vi.mocked(api.health).mockResolvedValue({ ...HEALTH_LOCAL, auth_required: true });
    render(<AccessControlPanel />);
    expect(await screen.findByLabelText("This browser's access token")).toBeInTheDocument();
  });

  it("saves a pasted token and reconnects with it", async () => {
    vi.mocked(api.health).mockResolvedValue({ ...HEALTH_LOCAL, auth_required: true });
    render(<AccessControlPanel />);

    const input = await screen.findByLabelText("This browser's access token");
    fireEvent.change(input, { target: { value: "team-token" } });
    fireEvent.click(screen.getByText("Save"));

    await waitFor(() => expect(api.setAuthToken).toHaveBeenCalledWith("team-token"));
    await waitFor(() => expect(api.connect).toHaveBeenCalled());
  });

  it("clears a stored token", async () => {
    vi.mocked(api.hasAuthToken).mockReturnValue(true);
    vi.mocked(api.health).mockResolvedValue({ ...HEALTH_LOCAL, auth_required: true });
    render(<AccessControlPanel />);

    fireEvent.click(await screen.findByText("Clear"));

    expect(api.setAuthToken).toHaveBeenCalledWith(null);
    expect(await screen.findByText("No token stored yet.")).toBeInTheDocument();
  });
});

describe("HealthDashboard", () => {
  it("shows Offline when not connected and expands to Online details when connected", async () => {
    render(<HealthDashboard />);
    expect(screen.getByText("Offline")).toBeInTheDocument();

    fireEvent.click(screen.getByText("Offline"));
    expect(screen.getByText("Offline", { selector: "span" })).toBeInTheDocument();
  });

  it("reports the uptime Core actually sends, not a field it never had", async () => {
    vi.mocked(useCid).mockReturnValue({ connected: true, missions: [] } as any);
    render(<HealthDashboard />);

    fireEvent.click(screen.getByText("Healthy"));

    // 65s from uptime_seconds. The old code read `data.uptime`, which Core has
    // never sent, so this tile displayed 0s on every install.
    expect(await screen.findByText("1m 5s")).toBeInTheDocument();
  });

  it("counts in-flight missions from the store, excluding terminal states", async () => {
    vi.mocked(useCid).mockReturnValue({
      connected: true,
      // MissionStatus serializes snake_case in cid-core/src/api/types.rs.
      missions: [
        { id: "1", status: "running" },
        { id: "2", status: "review" },
        { id: "3", status: "done" },
        { id: "4", status: "failed" },
        { id: "5", status: "closed" },
      ],
    } as any);
    render(<HealthDashboard />);

    fireEvent.click(screen.getByText("Healthy"));

    expect(await screen.findByText("Active (this repo)")).toBeInTheDocument();
    expect(screen.getByText("2")).toBeInTheDocument();
  });
});
