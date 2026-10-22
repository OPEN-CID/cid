import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { ChatThread } from "./ChatThread";
import { api } from "@/lib/api";
import { useCid } from "@/hooks/useCid";

// Mock API
vi.mock("@/lib/api", () => ({
  api: {
    onNotification: () => () => {},
    call: vi.fn().mockResolvedValue({}),
    mcp: { list: vi.fn(), tools: vi.fn(), callTool: vi.fn() },
    session: {
      sendMessage: vi.fn(),
      contextUsage: vi.fn(),
      get: vi.fn().mockResolvedValue({}),
    },
    model: { list: vi.fn().mockResolvedValue([]), routeRecommend: vi.fn() },
    repo: { agentsMd: vi.fn().mockResolvedValue(null), agentsMdApprove: vi.fn() },
  },
}));

vi.mock("@/hooks/useCid", () => ({
  useCid: vi.fn(),
}));

describe("ChatThread", () => {
  beforeEach(() => {
    // jsdom has no layout engine, so scrolling is a no-op stub.
    window.HTMLElement.prototype.scrollIntoView = vi.fn();
    vi.mocked(useCid).mockReturnValue({
      selectedSessionId: null,
      messages: {},
      addMessage: vi.fn(),
      updateMessage: vi.fn(),
      loadMessages: vi.fn(),
    } as any);
  });

  it("shows empty state when no session selected", () => {
    render(<ChatThread />);
    expect(screen.getByText(/No session selected/)).toBeInTheDocument();
    expect(screen.getByText(/Flow 1 – First Session/)).toBeInTheDocument();
  });

  it("offers MCP tools in an @mention dropdown", async () => {
    vi.mocked(useCid).mockReturnValue({
      selectedSessionId: "session-1",
      sessions: [{ id: "session-1", repo_channel_id: "repo-1" }],
      repos: [{ id: "repo-1", path: "/tmp/repo" }],
      messages: { "session-1": [] },
      addMessage: vi.fn(),
      updateMessage: vi.fn(),
      loadMessages: vi.fn(),
      loadRepos: vi.fn(),
    } as any);
    vi.mocked(api.mcp.list).mockResolvedValue([{ id: "jira", name: "jira" }]);
    vi.mocked(api.mcp.tools).mockResolvedValue([{ name: "get_issue", description: "Get a ticket" }]);

    render(<ChatThread />);
    const box = screen.getByPlaceholderText(/@mention/);
    fireEvent.change(box, { target: { value: "look at @jir", selectionStart: 10 } });

    expect(await screen.findByText("@jira.get_issue")).toBeInTheDocument();
  });

  it("resolves @mentions through real tool calls before sending", async () => {
    const sendMessage = vi.fn().mockResolvedValue({});
    vi.mocked(useCid).mockReturnValue({
      selectedSessionId: "session-1",
      sessions: [{ id: "session-1", repo_channel_id: "repo-1" }],
      repos: [{ id: "repo-1", path: "/tmp/repo" }],
      messages: { "session-1": [] },
      addMessage: vi.fn(),
      updateMessage: vi.fn(),
      loadMessages: vi.fn(),
      loadRepos: vi.fn(),
    } as any);
    vi.mocked(api.mcp.list).mockResolvedValue([{ id: "jira", name: "jira" }]);
    vi.mocked(api.mcp.tools).mockResolvedValue([{ name: "get_issue" }]);
    vi.mocked(api.session.sendMessage).mockImplementation(sendMessage);
    vi.mocked(api.session.contextUsage).mockRejectedValue(new Error("no usage"));
    vi.mocked(api.mcp.callTool).mockResolvedValue({ key: "ENG-1", title: "Bug" });

    render(<ChatThread />);
    const box = screen.getByPlaceholderText(/@mention/);
    fireEvent.change(box, {
      target: { value: 'check @jira.get_issue {"key": "ENG-1"}', selectionStart: 38 },
    });
    fireEvent.keyDown(box, { key: "Enter", shiftKey: false });

    await waitFor(() =>
      expect(api.mcp.callTool).toHaveBeenCalledWith("jira", "get_issue", { key: "ENG-1" })
    );
    await waitFor(() =>
      expect(sendMessage).toHaveBeenCalledWith(
        "session-1",
        expect.stringContaining("[Tool @jira.get_issue result:")
      )
    );
  });

  it("leaves unknown @mentions as plain text instead of calling anything", async () => {
    const sendMessage = vi.fn().mockResolvedValue({});
    vi.mocked(useCid).mockReturnValue({
      selectedSessionId: "session-1",
      sessions: [{ id: "session-1", repo_channel_id: "repo-1" }],
      repos: [{ id: "repo-1", path: "/tmp/repo" }],
      messages: { "session-1": [] },
      addMessage: vi.fn(),
      updateMessage: vi.fn(),
      loadMessages: vi.fn(),
      loadRepos: vi.fn(),
    } as any);
    vi.mocked(api.mcp.list).mockResolvedValue([]);
    vi.mocked(api.mcp.tools).mockResolvedValue([]);
    vi.mocked(api.session.sendMessage).mockImplementation(sendMessage);
    vi.mocked(api.session.contextUsage).mockRejectedValue(new Error("no usage"));
    vi.mocked(api.mcp.callTool).mockClear();

    render(<ChatThread />);
    const box = screen.getByPlaceholderText(/@mention/);
    fireEvent.change(box, { target: { value: "hi @nosuch.tool", selectionStart: 14 } });
    fireEvent.keyDown(box, { key: "Enter", shiftKey: false });

    await waitFor(() => expect(sendMessage).toHaveBeenCalled());
    expect(api.mcp.callTool).not.toHaveBeenCalled();
    expect(sendMessage).toHaveBeenCalledWith("session-1", "hi @nosuch.tool");
  });
});
