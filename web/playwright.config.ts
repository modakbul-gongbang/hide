import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  timeout: 30_000,
  // Each spec starts its own isolated Herdr server, hided and Chromium; running
  // them in parallel makes them contend for the runner's cores, and a timing
  // assertion that fails under load reads as a product failure. One worker
  // keeps the lane deterministic; the whole suite is under two minutes.
  workers: 1,
  use: {
    baseURL: process.env.HIDE_E2E_ORIGIN ?? "http://127.0.0.1:4173",
    headless: true,
  },
  webServer: undefined,
});
