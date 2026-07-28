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
      command: "npm run dev:core",
      url: "http://127.0.0.1:5919/health",
      reuseExistingServer: true,
      timeout: 120000,
    },
  ],
});
