import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  timeout: 30_000,
  // Each spec starts its own isolated Herdr server, hided and Chromium; running
  // them in parallel makes them contend for the runner's cores, and a timing
  // assertion that fails under load reads as a product failure. One worker
  // keeps a runner deterministic; CI deals the tests out across runners
  // (`--shard`), which is where the suite's parallelism lives.
  workers: 1,
  // Every test is its own unit: each starts and stops its own stack, so none
  // depends on the one before it. Saying so lets `--shard` deal out tests
  // instead of whole files, which is what keeps the CI shards even
  // (s3 alone is a third of the suite).
  fullyParallel: true,
  // The list reporter names every test with its duration, so a CI log says
  // where the minutes went; the dot reporter CI would default to does not.
  reporter: "list",
  use: {
    baseURL: process.env.HIDE_E2E_ORIGIN ?? "http://127.0.0.1:4173",
    headless: true,
  },
  webServer: undefined,
});
