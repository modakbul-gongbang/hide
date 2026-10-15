import { expect, type ElectronApplication } from "@playwright/test";
import { startHerdr } from "../../web/e2e/herdr-fixture";
import { enterWorkspace } from "../../web/e2e/wire";
import { isolate, launchShell, test } from "./fixture";
import { captureElectronWindow } from "./electron-window";
import { captureNativeWindow } from "./native-window";

test("sidebar tab segments use the candidate window's real tab", { tag: "@platform" }, async () => {
  // A private desktop host, local stream and state file on each system.
  const herdr = await startHerdr();
  const run = isolate(herdr, "sidebar-tab-groups");
  let app: ElectronApplication | undefined;
  try {
    const launched = await launchShell(run.env);
    app = launched.app;
    const page = launched.page;
    await page.reload();
    await enterWorkspace(page, "fixture");
    const glyph = page.locator(`[data-sidebar-tab="${herdr.tab}"]`);
    const rows = page.locator(`[data-sidebar-tab-group="${herdr.tab}"] [data-agent-open]`);
    await expect(glyph).toHaveAccessibleName("Tab 1, 2 agents");
    await expect(rows).toHaveCount(2);
    await expect(page.locator(`[data-sidebar-tab-guide="${herdr.tab}"]`)).toHaveCount(1);
    await captureElectronWindow(app, "sidebar-native-segments", { build: "worktree debug candidate", daemonPid: run.daemonPid(), privateSocket: herdr.socket });
    await captureNativeWindow(app, "sidebar-compositor-segments", { method: "screencapture exact candidate window without activation", build: "worktree debug candidate", daemonPid: run.daemonPid(), privateSocket: herdr.socket, limits: "native keyboard and IME not exercised" });
    await glyph.click();
    await expect(page.locator(`[data-tab="${herdr.tab}"]`).first()).toHaveAttribute("aria-selected", "true");
    await expect(rows).toHaveCount(2);
  } finally {
    if (app) await app.close();
    run.cleanup();
    herdr.stop();
  }
});
