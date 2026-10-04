import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  reporter: [["list"], ["../scripts/ci-reporter.ts"]],
  timeout: 90_000,
  // Each spec starts its own isolated Herdr server, hided and Electron app;
  // one worker keeps them from contending for the machine.
  workers: 1,
  // A `@needs-focus` test brings its app to the front and then quits it. On a
  // machine with no other app in front, a CI runner, macOS then activates the
  // next app that opens, and every background test after it would fail the
  // focus guard. The background tests run first, and the focus tests after.
  projects: [
    { name: "background", grepInvert: /@needs-focus/ },
    { name: "needs-focus", grep: /@needs-focus/, dependencies: ["background"] },
  ],
});
