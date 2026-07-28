import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { ConnectionBanner, AccessControlPanel, HealthDashboard } from "./WebShell";
import { api } from "@/lib/api";
import { useCid } from "@/hooks/useCid";

// 051-Editor-Excellence-Roadmap.md Wave 5.4. AccessControlPanel is
// security-relevant — it's the surface that warns a user their Core is
// reachable beyond localhost with no token.

vi.mock("@/lib/api", () => ({
  api: { connect: vi.fn() },
}));

vi.mock("@/hooks/useCid", () => ({
  useCid: vi.fn(),
}));

describe("ConnectionBanner", () => {
  beforeEach(() => {
    vi.mocked(api.connect).mockReset();
  });

  it("renders nothing when connected", () => {
    vi.mocked(useCid).mockReturnValue({ connected: true } as any);
    const { container } = render(<ConnectionBanner />);
    expect(container).toBeEmptyDOMElement();
  });

  it("shows an offline banner when disconnected", () => {
    vi.mocked(useCid).mockReturnValue({ connected: false } as any);
    render(<ConnectionBanner />);
    expect(screen.getByText("Core offline")).toBeInTheDocument();
  });

  it("Reconnect calls api.connect", async () => {
    vi.mocked(useCid).mockReturnValue({ connected: false } as any);
    vi.mocked(api.connect).mockResolvedValueOnce(undefined);
    render(<ConnectionBanner />);

    fireEvent.click(screen.getByText("Reconnect"));

    await waitFor(() => expect(api.connect).toHaveBeenCalled());
  });

  it("expanding shows the startup command reference", () => {
    vi.mocked(useCid).mockReturnValue({ connected: false } as any);
    render(<ConnectionBanner />);

    fireEvent.click(screen.getByLabelText("Expand connection details"));

    expect(screen.getByText("cargo run -p cid-core -- --port 5919")).toBeInTheDocument();
  });
});

describe("AccessControlPanel", () => {
  beforeEach(() => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        json: () => Promise.resolve({ loopback_only: true, auth_required: false, connected_clients: 1 }),
      })
    );
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("shows loopback-only and no-warning state by default", async () => {
    render(<AccessControlPanel />);
    expect(await screen.findByText("loopback only")).toBeInTheDocument();
    expect(screen.queryByText(/reachable beyond localhost with no token/)).not.toBeInTheDocument();
  });

  it("warns when reachable remotely with no auth token", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        json: () => Promise.resolve({ loopback_only: false, auth_required: false, connected_clients: 3 }),
      })
    );
    render(<AccessControlPanel />);

    expect(await screen.findByText("remote")).toBeInTheDocument();
    expect(screen.getByText(/reachable beyond localhost with no token/)).toBeInTheDocument();
  });

  it("does not warn when reachable remotely but a token is required", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        json: () => Promise.resolve({ loopback_only: false, auth_required: true, connected_clients: 3 }),
      })
    );
    render(<AccessControlPanel />);

    expect(await screen.findByText("required")).toBeInTheDocument();
    expect(screen.queryByText(/reachable beyond localhost with no token/)).not.toBeInTheDocument();
  });
});

describe("HealthDashboard", () => {
  beforeEach(() => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        json: () => Promise.resolve({ uptime: 65, connected_clients: 2, active_missions: 1 }),
      })
    );
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("shows Offline when not connected and expands to Online details when connected", async () => {
    vi.mocked(useCid).mockReturnValue({ connected: false } as any);
    render(<HealthDashboard />);
    expect(screen.getByText("Offline")).toBeInTheDocument();

    fireEvent.click(screen.getByText("Offline"));
    expect(screen.getByText("Offline", { selector: "span" })).toBeInTheDocument();
  });
});
