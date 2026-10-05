// The e2e entry point for both packages: build the fixtures' C programs once,
// before any test starts, so a test never compiles one.
import { buildFixtureShims } from "./shims/build";

export default function globalSetup(): void {
  buildFixtureShims();
}
