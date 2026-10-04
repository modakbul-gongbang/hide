import { defineConfig } from "vitest/config";

export default defineConfig({
  test: { include: ["src/**/*.test.ts", "e2e/fixture-cleanup.unit.ts", "e2e/platform-cleanup.unit.ts"] },
});
