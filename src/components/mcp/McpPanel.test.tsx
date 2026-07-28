import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, act, waitFor } from "@testing-library/react";
import { McpPanel } from "./McpPanel";
import { DialogHost } from "@/components/ui/DialogHost";
import { api } from "@/lib/api";
import { useDialogStore } from "@/lib/dialog";

vi.mock("@/lib/api", () => ({
  api: {
    mcp: {
      list: vi.fn(),
      add: vi.fn(),
      remove: vi.fn(),
      tools: vi.fn(),
    },
  },
}));

async function flushAsyncWork() {
  await act(async () => {
    for (let i = 0; i < 6; i++) {
      await Promise.resolve();
    }
  });
}

function renderPanel() {
  return render(
    <>
      <McpPanel />
      <DialogHost />
    </>
  );
}

const servers = [
  { id: "srv-1", name: "filesystem", transport_type: "stdio", status: "connected", enabled_for_repos: [] },
];

describe("McpPanel", () => {
  beforeEach(() => {
    vi.mocked(api.mcp.list).mockReset();
    vi.mocked(api.mcp.add).mockReset();
    vi.mocked(api.mcp.remove).mockReset();
    vi.mocked(api.mcp.tools).mockReset();
    useDialogStore.setState({ toasts: [], confirmRequest: null, infoRequest: null });
  });

  it("shows the empty state when no servers are configured", async () => {
    vi.mocked(api.mcp.list).mockResolvedValueOnce([]);
    renderPanel();
    await flushAsyncWork();
    expect(screen.getByText(/No MCP servers configured/)).toBeInTheDocument();
  });

  it("lists configured servers with their status", async () => {
    vi.mocked(api.mcp.list).mockResolvedValueOnce(servers);
    renderPanel();
    await flushAsyncWork();
    expect(screen.getByText("filesystem")).toBeInTheDocument();
    expect(screen.getByText("stdio")).toBeInTheDocument();
    expect(screen.getByText("connected")).toBeInTheDocument();
  });

  it("adding a stdio server sends command config and reloads the list", async () => {
    vi.mocked(api.mcp.list).mockResolvedValueOnce([]).mockResolvedValueOnce(servers);
    vi.mocked(api.mcp.add).mockResolvedValueOnce({ id: "srv-1" });

    renderPanel();
    await flushAsyncWork();

    fireEvent.click(screen.getByText("Add Server"));
    fireEvent.change(screen.getByPlaceholderText(/Server name/), { target: { value: "filesystem" } });
    fireEvent.change(screen.getByPlaceholderText(/Command/), {
      target: { value: "npx -y @modelcontextprotocol/server-filesystem /tmp" },
    });
    fireEvent.click(screen.getByText("Add"));
    await flushAsyncWork();

    expect(vi.mocked(api.mcp.add)).toHaveBeenCalledWith("filesystem", "stdio", {
      command: "npx -y @modelcontextprotocol/server-filesystem /tmp",
      args: [],
    });
    expect(screen.getByText("filesystem")).toBeInTheDocument();
  });

  it("removing a server asks for confirmation and reloads on confirm", async () => {
    vi.mocked(api.mcp.list).mockResolvedValueOnce(servers).mockResolvedValueOnce([]);
    vi.mocked(api.mcp.remove).mockResolvedValueOnce({});

    renderPanel();
    await flushAsyncWork();

    fireEvent.click(screen.getByText("Remove"));
    fireEvent.click(await screen.findByText("Confirm"));
    await flushAsyncWork();

    expect(vi.mocked(api.mcp.remove)).toHaveBeenCalledWith("srv-1");
  });

  it("removing a server does nothing if the user cancels the confirmation", async () => {
    vi.mocked(api.mcp.list).mockResolvedValueOnce(servers);

    renderPanel();
    await flushAsyncWork();

    fireEvent.click(screen.getByText("Remove"));
    fireEvent.click(await screen.findByText("Cancel"));
    await flushAsyncWork();

    expect(vi.mocked(api.mcp.remove)).not.toHaveBeenCalled();
  });

  it("listing tools shows them in an info dialog instead of window.alert", async () => {
    vi.mocked(api.mcp.list).mockResolvedValueOnce(servers);
    vi.mocked(api.mcp.tools).mockResolvedValueOnce([{ name: "read_file" }]);

    renderPanel();
    await flushAsyncWork();

    fireEvent.click(screen.getByText("List Tools"));
    await waitFor(() => expect(api.mcp.tools).toHaveBeenCalledWith("srv-1"));

    expect(await screen.findByText("Tools — filesystem")).toBeInTheDocument();
    expect(screen.getByText(/read_file/)).toBeInTheDocument();
  });
});
