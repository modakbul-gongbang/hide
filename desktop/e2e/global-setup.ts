// Builds the web fixtures' C programs once, before any test (web/e2e/shims/build.ts).
// This file lives in the desktop package so Playwright loads it, and what it
// imports, the way it loads the desktop specs.
import path from "node:path";
import type { FullConfig } from "@playwright/test";
import { buildFixtureShims } from "../../web/e2e/shims/build";

export default function globalSetup(config: FullConfig): void {
  buildFixtureShims(path.dirname(config.configFile!));
}
