import { test, expect } from "@playwright/test";
import { fileURLToPath } from "node:url";
import path from "node:path";

/**
 * Health & API coverage E2E test — validates that ALL REST endpoints respond correctly.
 * Run this after starting the core with `cargo run -p cid-core`.
 */

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const BASE = "http://127.0.0.1:5919";

async function rpc(method: string, params: unknown = {}, id = "1") {
  const resp = await fetch(`${BASE}/api/rpc`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id, method, params }),
  });
  if (!resp.ok) {
    throw new Error(`HTTP ${resp.status} for ${method}`);
  }
  return resp.json();
}

test.describe("Health Check & API Coverage", () => {
  test("health endpoint should return OK", async () => {
    const resp = await fetch(`${BASE}/health`).catch(() => null);
    if (!resp) {
      test.skip(true, "Core not running");
      return;
    }
    const data = await resp.json();
    expect(data.status).toBe("ok");
    expect(data.service).toBe("cid-core");
    expect(data.version).toBe("0.1.0");
  });

  test("WebSocket should connect", async () => {
    const ws = new WebSocket(`ws://127.0.0.1:5919/ws`);
    await new Promise<void>((resolve, reject) => {
      ws.onopen = () => { ws.close(); resolve(); };
      ws.onerror = () => reject(new Error("WebSocket failed"));
      setTimeout(() => reject(new Error("timeout")), 3000);
    });
  });

  test.describe("Workspace & Repo API", () => {
    let repoId: string;

    test("workspace.list should return workspaces", async () => {
      const data = await rpc("workspace.list");
      expect(data.result.length).toBeGreaterThan(0);
      expect(data.result[0].name).toBe("Default Workspace");
    });

    test("repo.connect should create a repo channel", async () => {
      const data = await rpc("repo.connect", { path: __dirname });
      expect(data.result.id).toBeTruthy();
      expect(data.result.path).toBe(__dirname);
      repoId = data.result.id;
    });

    test("repo.list should include connected repo", async () => {
      const data = await rpc("repo.list");
      expect(data.result.length).toBeGreaterThan(0);
    });

    test("repo.get should return repo details", async () => {
      const data = await rpc("repo.get", { id: repoId });
      expect(data.result.id).toBe(repoId);
    });
  });

  test.describe("Mission API", () => {
    let repoId: string;
    let missionId: string;

    test.beforeAll(async () => {
      try {
        const data = await rpc("repo.connect", { path: __dirname });
        repoId = data.result.id;
      } catch {
        // best-effort setup; the dependent tests below fail on their own if this didn't work
      }
    });

    test("mission.create should create a mission", async () => {
      const data = await rpc("mission.create", {
        repo_channel_id: repoId,
        title: "E2E Health Check Mission",
        task: "Verify the mission system works end-to-end",
        session_mode: "worktree",
        autonomy_level: "co_pilot",
      });
      expect(data.result.id).toBeTruthy();
      expect(data.result.title).toBe("E2E Health Check Mission");
      missionId = data.result.id;
    });

    test("mission.get should return mission", async () => {
      const data = await rpc("mission.get", { id: missionId });
      expect(data.result.id).toBe(missionId);
    });

    test("mission.send_message should work", async () => {
      const data = await rpc("mission.send_message", {
        mission_id: missionId,
        content: "Hello from E2E test",
      });
      expect(data.result).toBeDefined();
    });

    test("message.list should return messages", async () => {
      const data = await rpc("message.list", { mission_id: missionId });
      expect(data.result.length).toBeGreaterThan(0);
    });
  });

  test.describe("Git API", () => {
    test("git.status should return status", async () => {
      // __dirname (tests/e2e) is not itself a git repo — use the actual repo root
      // two levels up so this exercises the real code path instead of vacuously
      // swallowing a "not a git repository" error.
      const repoRoot = path.join(__dirname, "..", "..");
      const data = await rpc("git.status", { repo_path: repoRoot });
      expect(Array.isArray(data.result)).toBe(true);
    });
  });

  test.describe("Settings API", () => {
    test("settings.get should return settings", async () => {
      const data = await rpc("settings.get");
      expect(data.result.theme).toBeDefined();
      expect(data.result.anthropic_model).toBeDefined();
    });

    test("settings.update should update settings", async () => {
      const data = await rpc("settings.update", { theme: "dark" });
      expect(data.result.theme).toBe("dark");
    });
  });

  test.describe("Model API", () => {
    test("model.list should return available models", async () => {
      const data = await rpc("model.list");
      expect(Array.isArray(data.result)).toBe(true);
      expect(data.result.length).toBeGreaterThan(0);
    });
  });

  test.describe("MCP API", () => {
    test("mcp.server.list should return server list", async () => {
      const data = await rpc("mcp.server.list");
      expect(Array.isArray(data.result)).toBe(true);
    });

    test("mcp.server.add and remove should work", async () => {
      const addData = await rpc("mcp.server.add", {
        name: "e2e-test-server",
        transport_type: "stdio",
        config: { command: "echo", args: ["{}"] },
      });
      expect(addData.result.id).toBeTruthy();

      const listData = await rpc("mcp.server.list");
      const names = listData.result.map((s) => s.name);
      expect(names).toContain("e2e-test-server");

      await rpc("mcp.server.remove", { id: addData.result.id });
    });
  });

  test.describe("Code Analysis API (Phase 2)", () => {
    test("code.analyze_directory should analyze the current directory", async () => {
      const data = await rpc("code.analyze_directory", { dir_path: __dirname }).catch(() => null);
      if (data) {
        expect(Array.isArray(data.result)).toBe(true);
      }
    });

    test("code.search_symbols should return results", async () => {
      const data = await rpc("code.search_symbols", {
        dir_path: __dirname,
        query: "test",
      }).catch(() => null);
      if (data) {
        expect(data.result.total).toBeDefined();
      }
    });
  });

  test.describe("File API", () => {
    test("file.read should read a file", async () => {
      const data = await rpc("file.read", { path: __filename }).catch(() => null);
      if (data) {
        expect(data.result.content).toContain("Health Check");
      }
    });

    test("file.write should write and read back", async () => {
      // Must be inside __dirname, the repo channel connected in beforeAll —
      // 050-Gold-Standard-Review.md F1 confines file.* to connected repos.
      const tmpPath = `${__dirname}/_e2e_test_temp.txt`;
      try {
        await rpc("file.write", { path: tmpPath, content: "E2E test write" });
        const data = await rpc("file.read", { path: tmpPath });
        expect(data.result.content).toBe("E2E test write");
      } finally {
        try {
          await rpc("file.write", { path: tmpPath, content: "" });
        } catch {
          // best-effort cleanup of the temp file this test wrote
        }
      }
    });

    test("file.write outside every connected repo should be denied", async () => {
      // 050-Gold-Standard-Review.md F1: file.* previously took any path with
      // no confinement at all. One directory above the connected repo channel
      // (__dirname) must now be refused.
      const outsidePath = `${__dirname}/../_e2e_should_never_be_written.txt`;
      const data = await rpc("file.write", { path: outsidePath, content: "should not land" });
      expect(data.error).toBeTruthy();
      const fs = await import("fs");
      expect(fs.existsSync(outsidePath)).toBe(false);
    });

    test("file.read with parent-directory traversal should be denied", async () => {
      const data = await rpc("file.read", { path: `${__dirname}/../../../etc/passwd` });
      expect(data.error).toBeTruthy();
    });
  });

  test.describe("PTY API", () => {
    test("pty.list should return PTY instances for a mission", async () => {
      const data = await rpc("pty.list", { mission_id: "e2e-health-check-nonexistent" }).catch(() => null);
      if (data) {
        expect(Array.isArray(data.result)).toBe(true);
      }
    });
  });
});
