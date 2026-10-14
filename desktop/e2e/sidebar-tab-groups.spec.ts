import { expect, type ElectronApplication } from "@playwright/test";
import { startHerdr } from "../../web/e2e/herdr-fixture";
import { countSent, enterWorkspace } from "../../web/e2e/wire";
import { isolate, launchShell, test } from "./fixture";
import { captureElectronWindow } from "./electron-window";
import { captureNativeWindow } from "./native-window";

test("sidebar groups use the candidate window's real tab and saved disclosure", { tag: "@platform" }, async () => {
  // A private desktop host, local stream and state file on each system.
  const herdr = await startHerdr();
  const run = isolate(herdr, "sidebar-tab-groups");
  let app: ElectronApplication | undefined;
  try {
    const launched = await launchShell(run.env);
    app = launched.app;
    const page = launched.page;
    const sent = countSent(page);
    await page.reload();
    await enterWorkspace(page, "fixture");
    const header = page.locator(`[data-sidebar-tab="${herdr.tab}"]`);
    const fold = page.locator(`[data-sidebar-tab-toggle="${herdr.tab}"]`);
    const rows = page.locator(`[data-sidebar-tab-group="${herdr.tab}"] [data-agent-open]`);
    await expect(header).toHaveAccessibleName("Tab 1, 2 agents");
    await expect(rows).toHaveCount(2);
    await captureElectronWindow(app, "sidebar-native-open", { build: "worktree debug candidate", daemonPid: run.daemonPid(), privateSocket: herdr.socket });
    await captureNativeWindow(app, "sidebar-compositor-open", { method: "screencapture exact candidate window without activation", build: "worktree debug candidate", daemonPid: run.daemonPid(), privateSocket: herdr.socket, limits: "native keyboard and IME not exercised" });
    await fold.click();
    await expect(header).toHaveAttribute("aria-expanded", "false");
    await expect(rows).toHaveCount(0);
    await expect(header).toHaveAttribute("aria-current", "true");
    expect(sent.get("sidebar_tab_expanded")).toBe(1);
    await captureElectronWindow(app, "sidebar-native-folded", { build: "worktree debug candidate", daemonPid: run.daemonPid(), privateSocket: herdr.socket });
    await captureNativeWindow(app, "sidebar-compositor-folded", { method: "screencapture exact candidate window without activation", build: "worktree debug candidate", daemonPid: run.daemonPid(), privateSocket: herdr.socket, limits: "native keyboard and IME not exercised" });
    await app.close();
    app = undefined;
    const reopened = await launchShell(run.env);
    app = reopened.app;
    await enterWorkspace(reopened.page, "fixture");
    await expect(reopened.page.locator(`[data-sidebar-tab="${herdr.tab}"]`)).toHaveAttribute("aria-expanded", "false");
  } finally {
    if (app) await app.close();
    run.cleanup();
    herdr.stop();
  }
});
