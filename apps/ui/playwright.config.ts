/**
 * Playwright config (spec 001c-2): the e2e checkout flow runs the real
 * desktop binary (global setup spawns it with the `e2e-shim` +
 * `dev-session` features) and drives the e2e UI build in Chromium
 * via `vite preview`. One worker: a single till, serial sales.
 */
import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  globalSetup: "./e2e/global-setup.ts",
  globalTeardown: "./e2e/global-teardown.ts",
  timeout: 60_000,
  workers: 1,
  reporter: "list",
  use: {
    trace: "retain-on-failure",
  },
});
