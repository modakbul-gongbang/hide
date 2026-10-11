// The packaged app as an operator unpacks it, with its window: its own
// Electron host, the `hide` CLI and daemon in its resources, and the web
// shell that daemon embeds, against a private HOME, state and Herdr. The
// package lane runs it on Windows, where `smoke-package.mjs` starts the same
// package without a window and no other job draws the packaged one.
// HIDE_E2E_PACKAGED_APP names the packaged executable (`hide.exe` at the top
// of the unpacked folder on Windows, `hide.app/Contents/MacOS/hide` on macOS),
// never the operator's installed copy.

import { expect, type ElectronApplication } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "../../web/e2e/herdr-fixture";
import { fixtureExecutable } from "../../web/e2e/platform-fixture";
import { enterWorkspace } from "../../web/e2e/wire";
import { hostLog, isolate, launch, screenshot, shellPage, test } from "./fixture";

const executable = process.env.HIDE_E2E_PACKAGED_APP;
test.describe.configure({ timeout: 180_000 });
test.skip(!executable, "a packaged app is required");

/** The packaged resources folder: `Contents/Resources` on macOS, `resources` beside the executable elsewhere. */
function packagedResources(file: string): string {
  return process.platform === "darwin" ? path.resolve(file, "..", "..", "Resources") : path.join(path.dirname(file), "resources");
}

test("the packaged app opens its window on the shell its own daemon serves", async () => {
  const file = fs.realpathSync.native(executable!);
  if (file.startsWith("/Applications/")) throw new Error("HIDE_E2E_PACKAGED_APP must name a build, never the operator's installed app");
  const resources = packagedResources(file);
  const herdr = await startHerdr({ agents: false });
  const run = isolate(herdr, "packaged");
  let app: ElectronApplication | null = null;
  try {
    // The host finds the package's own CLI, whose `hide connect` starts the
    // hided beside it; with no UI folder named, that daemon serves the shell it embeds.
    run.env.HIDE_CLI_PATH = path.join(resources, fixtureExecutable("hide"));
    delete run.env.HIDED_UI_DIR;
    ({ app } = await launch(run.env, { executablePath: file }));
    const page = await shellPage(app);
    expect(fs.realpathSync.native(await app.evaluate(() => process.resourcesPath))).toBe(fs.realpathSync.native(resources));
    expect(run.daemonPid()).not.toBeNull();
    await enterWorkspace(page, "fixture");
    await expect(page.locator("[data-pane-view] .xterm").first()).toBeVisible();
    await screenshot(page, "packaged-window");
  } catch (error) {
    console.log(hostLog(run.env).map((line) => JSON.stringify(line)).join("\n"));
    throw error;
  } finally {
    await app?.close().catch(() => undefined);
    run.cleanup();
    herdr.stop();
  }
});
