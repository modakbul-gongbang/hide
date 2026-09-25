import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  timeout: 90_000,
  // Each spec starts its own isolated Herdr server, hided and Electron app;
  // one worker keeps them from contending for the machine.
  workers: 1,
});
