// The one-time move of an older layout into ~/.hide on this Mac (PRD
// hide-home-layout B1-B4, B9-B14), driven through the app's own `hide
// connect` with nothing relocated: no HIDE_STATE_DIR, XDG_STATE_HOME or
// HCOORD_HOME, so every path is the default one under a private HOME.
//
// The app is a bundle staged from this checkout's build (`Hide.app` with
// `Contents/Resources` and a Node behind `Contents/MacOS`), because only a
// daemon inside a bundle runs the kit. launchd domains are per account, not
// per HOME: hcoord gives the plain `com.hcoord.daemon` label only to the
// account's own `~/.hide/hcoord` as the user database names it, so the
// daemon here gets a label of its own, which the test asserts before
// anything runs and unloads at the end. The operator's label is only read.

import { expect } from "@playwright/test";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { bootoutTestLabel, hcoordLabel, launchdPid, OPERATOR_HCOORD_LABEL, stageBuild } from "./device-home";
import { HIDE_CLI, isolate, relaunch, screenshot, shellPage, test } from "./fixture";
import { herdrBinary, startHerdr } from "../../web/e2e/herdr-fixture";
import { enterWorkspace } from "../../web/e2e/wire";

test.describe.configure({ timeout: 240_000 });
test.skip(process.platform !== "darwin", "hcoord's daemon runs through launchd on macOS only");

/** A bundle around the staged build: the kit reads its executable from Info.plist and runs hcoord on it in Node mode. */
function stageBundle(root: string): string {
  const build = stageBuild(root);
  const contents = path.join(root, "Hide.app", "Contents");
  fs.mkdirSync(path.join(contents, "MacOS"), { recursive: true });
  fs.renameSync(build, path.join(contents, "Resources"));
  fs.symlinkSync(herdrBinary(), path.join(contents, "Resources", "herdr"));
  fs.writeFileSync(path.join(contents, "Info.plist"), "<plist><dict><key>CFBundleExecutable</key><string>hide</string></dict></plist>\n");
  fs.writeFileSync(path.join(contents, "MacOS", "hide"), `#!/bin/sh\nexec ${JSON.stringify(process.execPath)} "$@"\n`, { mode: 0o755 });
  return path.join(contents, "Resources");
}

function json(command: string, args: string[], env: Record<string, string>): Record<string, unknown> {
  const run = spawnSync(command, args, { env, encoding: "utf8", timeout: 60_000 });
  const line = run.stdout.trim().split("\n").at(-1) ?? "";
  try { return JSON.parse(line) as Record<string, unknown>; } catch { throw new Error(`${command} ${args.join(" ")}: exit ${run.status}\n${run.stdout}\n${run.stderr}`); }
}

const alive = (pid: number) => { try { process.kill(pid, 0); return true; } catch { return false; } };

test("the first connect of a new app moves the old layout into ~/.hide once", async () => {
  const herdr = await startHerdr({ agents: false });
  // A short label keeps hcoord's socket under the private HOME within the Unix path limit.
  const run = isolate(herdr, "hl");
  const home = run.env.HOME!;
  const env: Record<string, string> = { ...run.env };
  for (const key of ["HIDE_STATE_DIR", "XDG_STATE_HOME", "HCOORD_HOME"]) delete env[key];
  const resources = stageBundle(run.root);
  const appHide = path.join(resources, "hide");
  const newHcoord = path.join(home, ".hide", "hcoord");
  const label = hcoordLabel(newHcoord);
  expect(label).not.toBe(OPERATOR_HCOORD_LABEL);
  const operatorDaemon = launchdPid(OPERATOR_HCOORD_LABEL);
  const legacyState = path.join(home, ".local", "state", "hide");
  const legacyHcoord = path.join(home, ".hcoord");
  let oldPid = 0;
  let newPid = 0;
  try {
    // An older build's daemon, running from the legacy folder with the operator's state beside it. It
    // runs outside a bundle, so it runs no kit pass and leaves hcoord's old home to the new app.
    const old = json(HIDE_CLI, ["connect"], { ...env, HIDE_STATE_DIR: legacyState });
    oldPid = Number(old.pid);
    fs.writeFileSync(path.join(legacyState, "operator-file.txt"), "kept");
    const hostId = fs.readFileSync(path.join(legacyState, "host-id"), "utf8");
    // hcoord's old home: a ledger written by hcoord itself, the old shim, and the junk a stopped daemon leaves.
    const store = path.join(resources, "hcoord", "dist", "hcoord");
    const seeded = spawnSync(process.execPath, ["-e", `require(${JSON.stringify(path.join(store, "store.js"))}).saveLedger(require(${JSON.stringify(path.join(store, "model.js"))}).emptyLedger("2026-10-02T00:00:00.000Z"))`], { env: { ...env, HCOORD_HOME: legacyHcoord }, encoding: "utf8" });
    expect(seeded.status, seeded.stderr).toBe(0);
    const ledger = fs.readFileSync(path.join(legacyHcoord, "ledger.json"), "utf8");
    fs.mkdirSync(path.join(legacyHcoord, "bin"), { recursive: true });
    fs.writeFileSync(path.join(legacyHcoord, "bin", "hcoord"), `#!/bin/sh\nexec ${JSON.stringify(process.execPath)} /old/cli.js "$@"\n`, { mode: 0o700 });
    fs.writeFileSync(path.join(legacyHcoord, "api.sock.lock"), "99999999\n");
    fs.writeFileSync(path.join(legacyHcoord, ".ledger.json.1.x.tmp"), "");
    // The labels era's leftovers on this Mac, and the shared parents that stay.
    fs.mkdirSync(path.join(home, ".local", "state", "hide-plugin-upgrade", "20260920T110346Z"), { recursive: true });
    fs.mkdirSync(path.join(home, ".local", "share", "hide", "agent-context-labels"), { recursive: true });
    fs.writeFileSync(path.join(home, ".local", "share", "hide", "agent-context-labels", "watcher.out"), "x");

    const connected = json(appHide, ["connect"], env);
    expect(connected.ok, JSON.stringify(connected)).toBe(true);
    newPid = Number(connected.pid);
    const moved = path.join(home, ".hide", "state");

    // B2, B3: one folder, the old daemon gone, one new daemon from ~/.hide/state.
    expect(newPid).not.toBe(oldPid);
    await expect.poll(() => alive(oldPid), { timeout: 10_000 }).toBe(false);
    expect(fs.existsSync(legacyState)).toBe(false);
    expect(fs.readFileSync(path.join(moved, "operator-file.txt"), "utf8")).toBe("kept");
    expect(fs.readFileSync(path.join(moved, "host-id"), "utf8")).toBe(hostId);
    expect(json(appHide, ["status", "--json"], env).pid).toBe(newPid);

    // B10, B11, B14: the kit moved hcoord whole and runs it from the new home under its own label.
    const shim = path.join(newHcoord, "bin", "hcoord");
    await expect.poll(() => fs.existsSync(shim) && fs.readFileSync(shim, "utf8").includes(".hide/kit/hcoord"), { timeout: 120_000 }).toBe(true);
    expect(fs.existsSync(legacyHcoord)).toBe(false);
    expect(fs.readFileSync(path.join(newHcoord, "ledger.json"), "utf8")).toBe(ledger);
    expect(fs.existsSync(path.join(newHcoord, ".ledger.json.1.x.tmp"))).toBe(false);
    expect(fs.readlinkSync(path.join(home, ".local", "bin", "hcoord"))).toBe(shim);
    await expect.poll(() => launchdPid(label), { timeout: 30_000 }).not.toBeNull();
    const status = json(path.join(home, ".local", "bin", "hcoord"), ["daemon", "status", "--json"], env);
    expect(status.ok, JSON.stringify(status)).toBe(true);
    expect(launchdPid(OPERATOR_HCOORD_LABEL)).toBe(operatorDaemon);

    // B9: the labels era's folders are gone; ~/.local/state and ~/.local/share stay.
    await expect.poll(() => fs.existsSync(path.join(home, ".local", "share", "hide")), { timeout: 30_000 }).toBe(false);
    expect(fs.existsSync(path.join(home, ".local", "state", "hide-plugin-upgrade"))).toBe(false);
    expect(fs.statSync(path.join(home, ".local", "state")).isDirectory()).toBe(true);
    expect(fs.statSync(path.join(home, ".local", "share")).isDirectory()).toBe(true);

    // B4: a second connect attaches to the same daemon and moves nothing.
    expect(json(appHide, ["connect"], env).pid).toBe(newPid);
    expect(fs.existsSync(legacyState) || fs.existsSync(legacyHcoord)).toBe(false);

    // B26: This Mac's row in Settings > Devices, in this checkout's window attached to that daemon.
    const app = await relaunch({ ...run.env, HIDE_STATE_DIR: moved, HIDE_CLI_PATH: appHide });
    try {
      const page = await shellPage(app);
      await enterWorkspace(page, "projects").catch(() => undefined);
      await page.locator("[data-open-settings]").click();
      await page.locator('[data-settings-tab="devices"]').click();
      await expect(page.locator('[data-kit-part="local:hcoord:installed"]')).toBeVisible();
      await screenshot(page, "home-layout-this-mac-kit");
    } finally {
      await app.close().catch(() => undefined);
    }
  } finally {
    spawnSync(appHide, ["stop"], { env, timeout: 20_000 });
    if (oldPid && alive(oldPid)) spawnSync(HIDE_CLI, ["stop"], { env: { ...env, HIDE_STATE_DIR: legacyState }, timeout: 20_000 });
    bootoutTestLabel(label);
    run.cleanup();
    herdr.stop();
  }
});
