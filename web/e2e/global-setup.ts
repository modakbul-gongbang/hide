// The e2e entry point for both packages: build the fixtures' C programs once,
// before any test starts, so a test never compiles one.
import path from "node:path";
import type { FullConfig } from "@playwright/test";
import { buildFixtureShims } from "./shims/build";

export default function globalSetup(config: FullConfig): void {
  buildFixtureShims(path.dirname(config.configFile!));
}
