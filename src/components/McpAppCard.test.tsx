import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { extractMcpAppContent, McpAppCard, McpToolResultCard } from "./McpAppCard";
import { api } from "@/lib/api";

// 051-Editor-Excellence-Roadmap.md Wave 5.4.

vi.mock("@/lib/api", () => ({
  api: { mcp: { callTool: vi.fn() } },
}));

describe("extractMcpAppContent", () => {
  it("returns null for a plain string result", () => {
    expect(extractMcpAppContent("just some text")).toBeNull();
  });

  it("returns null when no known location has a recognized app type", () => {
    expect(extractMcpAppContent({ foo: "bar" })).toBeNull();
  });

  it("finds an app payload under _meta['mcp/app']", () => {
    const result = extractMcpAppContent({
      _meta: { "mcp/app": { type: "markdown", body: "# hi" } },
    });
    expect(result).toEqual({ type: "markdown", body: "# hi" });
  });

  it("finds an app payload under _meta.mcp_app", () => {
    const result = extractMcpAppContent({ _meta: { mcp_app: { type: "form", fields: [] } } });
    expect(result?.type).toBe("form");
  });

  it("finds an app payload directly under mcp_app or app", () => {
    expect(extractMcpAppContent({ mcp_app: { type: "dashboard" } })?.type).toBe("dashboard");
    expect(extractMcpAppContent({ app: { type: "choice_cards", choices: [] } })?.type).toBe("choice_cards");
  });

  it("treats the whole result as the payload when it has a recognized type itself", () => {
    expect(extractMcpAppContent({ type: "html", html: "<b>hi</b>" })?.type).toBe("html");
  });

  it("does not treat an unrecognized type string as an app", () => {
    expect(extractMcpAppContent({ type: "not_a_real_type" })).toBeNull();
  });
});

describe("McpToolResultCard", () => {
  it("renders a string result as-is", () => {
    render(<McpToolResultCard toolName="read_file" result="hello world" />);
    fireEvent.click(screen.getByText(/read_file/));
    expect(screen.getByText("hello world")).toBeInTheDocument();
  });

  it("renders an object result as formatted JSON", () => {
    render(<McpToolResultCard toolName="list_files" result={{ files: ["a.txt"] }} />);
    fireEvent.click(screen.getByText(/list_files/));
    expect(screen.getByText(/"files"/)).toBeInTheDocument();
  });
});

describe("McpAppCard", () => {
  beforeEach(() => {
    vi.mocked(api.mcp.callTool).mockReset();
  });

  it("renders choice cards and submits the selected choice", async () => {
    vi.mocked(api.mcp.callTool).mockResolvedValueOnce({ ok: true });
    render(
      <McpAppCard
        serverId="srv-1"
        serverName="test-server"
        content={{
          type: "choice_cards",
          title: "Pick one",
          choices: [{ id: "a", label: "Option A" }, { id: "b", label: "Option B" }],
        }}
      />
    );

    fireEvent.click(screen.getByText("Option A"));

    await waitFor(() => expect(api.mcp.callTool).toHaveBeenCalledWith("srv-1", "Pick one_select", { choice_id: "a" }));
    expect(await screen.findByText("Choice submitted")).toBeInTheDocument();
  });

  it("renders a form and submits its values to the given submit_url", async () => {
    vi.mocked(api.mcp.callTool).mockResolvedValueOnce({ ok: true });
    render(
      <McpAppCard
        serverId="srv-1"
        serverName="test-server"
        content={{
          type: "form",
          submit_url: "create_ticket",
          fields: [{ id: "title", label: "Title", type: "text", required: true }],
        }}
      />
    );

    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Bug report" } });
    fireEvent.click(screen.getByText("Submit"));

    await waitFor(() =>
      expect(api.mcp.callTool).toHaveBeenCalledWith("srv-1", "create_ticket", { title: "Bug report" })
    );
    expect(await screen.findByText("Form submitted successfully")).toBeInTheDocument();
  });

  it("collapses and expands on header click", () => {
    render(
      <McpAppCard
        serverId="srv-1"
        serverName="test-server"
        content={{ type: "markdown", body: "hello" }}
      />
    );
    const header = screen.getByText(/MCP App: test-server/);

    fireEvent.click(header);
    fireEvent.click(header);

    expect(header).toBeInTheDocument();
  });
});
