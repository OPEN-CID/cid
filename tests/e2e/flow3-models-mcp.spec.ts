import { test, expect } from "@playwright/test";

test.describe("Flow 3 - Local Models & MCP Integration (Phase 3)", () => {
  test("should list available models via API", async () => {
    const resp = await fetch("http://127.0.0.1:5919/api/rpc", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: "1",
        method: "model.list",
        params: {},
      }),
    }).catch(() => null);

    if (!resp) {
      test.skip(true, "Core not running at http://127.0.0.1:5919");
      return;
    }

    const data = await resp.json();
    expect(data.result).toBeDefined();
    expect(data.result.length).toBeGreaterThanOrEqual(1);
    console.log(`[E2E] Available models: ${data.result.length}`);
  });

  test("should detect local runtimes", async () => {
    const resp = await fetch("http://127.0.0.1:5919/api/rpc", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: "2",
        method: "local.runtime.detect",
        params: { force_refresh: true },
      }),
    }).catch(() => null);

    if (!resp) {
      test.skip(true, "Core not running");
      return;
    }

    const data = await resp.json();
    expect(data.result).toBeDefined();
    // Local runtimes may not be running in CI, but the API should return a list
    console.log(`[E2E] Detected ${data.result.length} local runtimes`);
  });

  test("should add and remove MCP server", async () => {
    // Add a test MCP server
    const addResp = await fetch("http://127.0.0.1:5919/api/rpc", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: "3",
        method: "mcp.server.add",
        params: {
          name: "test-mcp-server",
          transport_type: "stdio",
          config: { command: "echo", args: ["{}"] },
        },
      }),
    }).catch(() => null);

    if (!addResp) {
      test.skip(true, "Core not running");
      return;
    }

    const addData = await addResp.json();
    expect(addData.result).toBeDefined();
    const serverId = addData.result.id;
    expect(serverId).toBeTruthy();
    console.log(`[E2E] Added MCP server: ${serverId}`);

    // List MCP servers
    const listResp = await fetch("http://127.0.0.1:5919/api/rpc", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: "4",
        method: "mcp.server.list",
        params: {},
      }),
    }).catch(() => null);

    if (listResp) {
      const listData = await listResp.json();
      const serverNames = (listData.result || []).map((s) => s.name);
      expect(serverNames).toContain("test-mcp-server");
      console.log(`[E2E] MCP servers: ${JSON.stringify(serverNames)}`);
    }

    // Remove MCP server
    const removeResp = await fetch("http://127.0.0.1:5919/api/rpc", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: "5",
        method: "mcp.server.remove",
        params: { id: serverId },
      }),
    }).catch(() => null);

    if (removeResp) {
      const removeData = await removeResp.json();
      expect(removeData.result).toBeDefined();
      console.log(`[E2E] Removed MCP server: ${serverId}`);
    }
  });
});
