// `hcoord daemon start` against a fake launchctl in an isolated HOME: no test
// bootstraps a label into the real launchd domain. The label follows
// HCOORD_HOME, so each test's daemon has a label of its own.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

import { installFakeLaunchctl } from "../helpers/fake-launchctl.mjs";

const CLI = path.resolve(import.meta.dirname, "../../dist/hcoord/cli.js");
// `hcoord daemon start` is macOS-only by design; elsewhere it refuses with unsupported_platform.
const MACOS_ONLY = { skip: process.platform !== "darwin" ? "hcoord daemon start is macOS-only" : false };

function isolated(t, extra = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hcoord-launchd-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const launchctl = installFakeLaunchctl(root);
  const home = path.join(root, "home");
  const data = path.join(root, "data");
  fs.mkdirSync(home);
  const env = { ...process.env, HOME: home, HCOORD_HOME: data, PATH: launchctl.env.PATH, LAUNCHCTL_FAKE_LOG: launchctl.log, LAUNCHCTL_FAKE_STATE: launchctl.stateFile, ...extra };
  delete env.HERDR_SOCKET_PATH;
  const daemon = (action) => spawnSync(process.execPath, [CLI, "daemon", action, "--json"], { env, encoding: "utf8", timeout: 45_000 });
  return { home, data, launchctl, start: () => daemon("start"), daemon, env };
}

test("daemon start after a manual stop kickstarts the still-loaded label instead of failing its bootstrap", MACOS_ONLY, (t) => {
  const { data, launchctl, start } = isolated(t);
  const first = start();
  assert.equal(first.status, 0, first.stdout + first.stderr);
  // `hcoord daemon stop` leaves the label loaded (KeepAlive keeps a clean exit down) and writes the marker.
  fs.writeFileSync(path.join(data, "manual-stop"), "stopped\n");
  const again = start();
  assert.equal(again.status, 0, again.stdout + again.stderr);
  assert.equal(JSON.parse(again.stdout).ok, true);
  const asked = launchctl.argv().map((args) => args[0]);
  assert.deepEqual(asked.filter((verb) => verb === "bootstrap").length, 1, "a loaded label is never bootstrapped a second time");
  assert.equal(launchctl.state().kicked, 2, "each start asks launchd to run the loaded label");
  assert.equal(fs.existsSync(path.join(data, "manual-stop")), false, "start clears the manual stop");
});

test("daemon uninstall unloads only its own label, removes its plist, and preserves conversation data", MACOS_ONLY, (t) => {
  const { data, launchctl, start, daemon } = isolated(t, { LAUNCHCTL_FAKE_BOOTOUT_SETTLE_PRINTS: "3" });
  const started = start();
  assert.equal(started.status, 0, started.stdout + started.stderr);
  const { label, path: plist } = JSON.parse(started.stdout).value;
  const ledger = path.join(data, "ledger.json");
  fs.writeFileSync(ledger, "conversation kept\n");
  fs.writeFileSync(path.join(data, "manual-stop"), "stopped\n");
  fs.writeFileSync(launchctl.stateFile, JSON.stringify({ loaded: { [label]: plist, "com.hcoord.daemon": "/operator/service.plist" } }));
  const removed = daemon("uninstall");
  assert.equal(removed.status, 0, removed.stdout + removed.stderr);
  assert.equal(JSON.parse(removed.stdout).value.label, label);
  assert.deepEqual(launchctl.state().loaded, { "com.hcoord.daemon": "/operator/service.plist" });
  assert.equal(fs.existsSync(plist), false);
  assert.equal(fs.readFileSync(ledger, "utf8"), "conversation kept\n");
  assert.equal(daemon("uninstall").status, 0, "removal is idempotent when the daemon is already down");
  assert.equal(launchctl.argv().filter(([verb]) => verb === "bootout").length, 1);
});

test("daemon start that reloads a changed plist waits for the bootout to settle before bootstrapping", MACOS_ONLY, (t) => {
  const { home, launchctl, start } = isolated(t, { LAUNCHCTL_FAKE_BOOTOUT_SETTLE_PRINTS: "3" });
  const first = start();
  assert.equal(first.status, 0, first.stdout + first.stderr);
  const { label } = JSON.parse(first.stdout).value;
  const plist = path.join(home, "Library", "LaunchAgents", `${label}.plist`);
  // The live shape: the loaded label still runs another checkout's daemon.
  fs.writeFileSync(plist, "<plist><string>/elsewhere/dist/hcoord/cli.js</string></plist>\n");
  fs.writeFileSync(launchctl.stateFile, JSON.stringify({ loaded: { [label]: plist } }));
  fs.rmSync(launchctl.log);
  const started = start();
  assert.equal(started.status, 0, started.stdout + started.stderr);
  assert.equal(JSON.parse(started.stdout).ok, true);
  assert.deepEqual(launchctl.argv().map((args) => args[0]).filter((verb) => verb !== "print"), ["bootout", "bootstrap", "kickstart"]);
  assert.equal(path.basename(launchctl.state().loaded[label]), `${label}.plist`);
  assert.match(fs.readFileSync(plist, "utf8"), new RegExp(CLI.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")), "the reloaded definition runs this build");
});
