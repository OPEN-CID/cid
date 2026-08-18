import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/e2e",
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  workers: 1,
  reporter: "list",
  use: {
    baseURL: "http://localhost:1420",
    trace: "on-first-retry",
    video: "on-first-retry",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  // 050-Gold-Standard-Review.md F9: CI already starts cid-core itself before
  // running the suite (.github/workflows/ci.yml's "Start core in background"
  // step) — only vite was ever started here, so `npx playwright test` on a
  // clean checkout hit every RPC assertion against a dead socket (16 of 30
  // health-check.spec.ts cases). `reuseExistingServer: true` on both entries
  // means a manually-started Core (the documented workaround for the
  // dev:all/EBUSY issue in CLAUDE.md) is still honored, not double-started.
  webServer: [
    {
      command: "npm run dev",
      url: "http://localhost:1420",
      reuseExistingServer: true,
      timeout: 60000,
    },
    {
      // `dev:core:e2e`, not `dev:core`: with no `--db`, Core falls back to the
      // OS data dir (`%APPDATA%/cid/cid.db` on Windows) — the developer's real
      // database. Every past E2E run therefore left `test-repo` channels,
      // missions and git worktrees behind in it; 13 such rows were found in a
      // real install, cluttering the repo list of the actual app. The suite
      // now writes to a disposable `.cid-e2e/` database instead.
      //
      // Caveat, stated rather than papered over: `reuseExistingServer: true`
      // is kept so the manually-started Core documented in CLAUDE.md is still
      // honored — which means that if you already have your *real* Core
      // running on 5919, the suite reuses it and this isolation does not
      // apply. Stop it first for a fully isolated run.
      command: "npm run dev:core:e2e",
      url: "http://127.0.0.1:5919/health",
      reuseExistingServer: true,
      timeout: 120000,
    },
  ],
});
