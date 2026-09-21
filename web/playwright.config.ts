import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  timeout: 30_000,
  use: {
    baseURL: process.env.HIDE_E2E_ORIGIN ?? "http://127.0.0.1:4173",
    headless: true,
  },
  webServer: undefined,
});
