import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { adoptLegacyHome } from "../../dist/hcoord/home.js";
import { daemonLabel, DEFAULT_LABEL } from "../../dist/hcoord/platform.js";
import { dataDir } from "../../dist/hcoord/store.js";

// No test here reaches launchd: every launchctl call goes to a recorder, and
// the label is one this file makes up. launchd domains are per account, so a
// real call with the account's label would stop the operator's coordinator.
const LABEL = "dev.hide.test.hcoord-home";

function withoutRelocation(t) {
  const previous = process.env.HCOORD_HOME;
  delete process.env.HCOORD_HOME;
  t.after(() => { if (previous === undefined) delete process.env.HCOORD_HOME; else process.env.HCOORD_HOME = previous; });
}

/** A fake launchctl: `loaded` says whether the label is up; bootout/bootstrap flip it; `fail` names verbs that refuse. */
function launchd({ loaded = true, fail = [] } = {}) {
  const state = { loaded, calls: [] };
  state.environment = {
    uid: 501,
    launchctl(args) {
      state.calls.push(args.join(" "));
      const verb = args[0];
      if (fail.includes(verb)) return { status: 5, stdout: "", stderr: `${verb} refused` };
      if (verb === "print") return state.loaded ? { status: 0, stdout: "loaded", stderr: "" } : { status: 113, stdout: "", stderr: "not found" };
      if (verb === "bootout") { state.loaded = false; return { status: 0, stdout: "", stderr: "" }; }
      if (verb === "bootstrap") { state.loaded = true; return { status: 0, stdout: "", stderr: "" }; }
      return { status: 0, stdout: "", stderr: "" };
    },
  };
  return state;
}

function legacyHome(t) {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "hcoord-home-"));
  t.after(() => fs.rmSync(home, { recursive: true, force: true }));
  const old = path.join(home, ".hcoord");
  fs.mkdirSync(path.join(old, "bin"), { recursive: true, mode: 0o700 });
  fs.mkdirSync(path.join(old, "outbox"), { mode: 0o700 });
  fs.mkdirSync(path.join(old, "api.sock.lock.recovery"), { mode: 0o700 });
  fs.writeFileSync(path.join(old, "ledger.json"), '{"schema":"kept"}');
  fs.writeFileSync(path.join(old, "manual-stop"), "2026-10-02T00:00:00Z\n");
  fs.writeFileSync(path.join(old, "outbox", "letter.json"), "{}");
  fs.writeFileSync(path.join(old, "bin", "hcoord"), "#!/bin/sh\n");
  fs.writeFileSync(path.join(old, "api.sock"), "");
  fs.writeFileSync(path.join(old, "api.sock.lock"), "99999999\n");
  fs.writeFileSync(path.join(old, "api.sock.lock.recovery", "owner"), "99999999\n");
  fs.writeFileSync(path.join(old, ".ledger.json.28834.0676d56c.tmp"), "");
  fs.writeFileSync(path.join(old, "health.json"), '{"starts":[{"pid":99999999,"at":"2026-10-02T00:00:00Z","readyAt":null,"cleanAt":null}]}');
  return { home, old, target: path.join(home, ".hide", "hcoord") };
}

test("the default home is inside hide's folder and HCOORD_HOME still relocates it", (t) => {
  withoutRelocation(t);
  assert.equal(dataDir("/Users/example"), "/Users/example/.hide/hcoord");
  process.env.HCOORD_HOME = "/iso/coordinator";
  assert.equal(dataDir("/Users/example"), "/iso/coordinator");
});

test("only the account's default home keeps the plain label, however it is named", (t) => {
  withoutRelocation(t);
  const account = os.userInfo().homedir;
  assert.equal(daemonLabel(account), DEFAULT_LABEL);
  process.env.HCOORD_HOME = path.join(account, ".hide", "hcoord");
  assert.equal(daemonLabel(account), DEFAULT_LABEL, "an explicit HCOORD_HOME equal to the default is the same coordinator");
  delete process.env.HCOORD_HOME;
  // A test that moved only HOME used to take the account's label; it has its own now.
  const isolated = daemonLabel(path.join(os.tmpdir(), "isolated-home"));
  assert.notEqual(isolated, DEFAULT_LABEL);
  assert.match(isolated, /^com\.hcoord\.daemon\.[0-9a-f]{12}$/);
  process.env.HCOORD_HOME = path.join(os.tmpdir(), "isolated-home", ".hcoord");
  assert.notEqual(daemonLabel(account), DEFAULT_LABEL);
});

test("adopt stops the old daemon, moves the whole home and drops only the junk", (t) => {
  withoutRelocation(t);
  const { home, old, target } = legacyHome(t);
  const fake = launchd();
  const adopted = adoptLegacyHome({ home, launchd: fake.environment, legacyLabel: LABEL });
  assert.equal(adopted.moved, true);
  assert.equal(adopted.stoppedLabel, LABEL);
  assert.deepEqual(adopted.dropped, [".ledger.json.28834.0676d56c.tmp", "api.sock", "api.sock.lock", "api.sock.lock.recovery", "health.json"]);
  assert.equal(fs.existsSync(old), false, "no copy stays behind");
  assert.equal(fs.readFileSync(path.join(target, "ledger.json"), "utf8"), '{"schema":"kept"}');
  assert.equal(fs.existsSync(path.join(target, "manual-stop")), true, "a manual stop moves with the home");
  assert.equal(fs.existsSync(path.join(target, "outbox", "letter.json")), true);
  assert.equal(fs.existsSync(path.join(target, "bin", "hcoord")), true);
  assert.equal(fs.statSync(path.join(home, ".hide")).mode & 0o777, 0o700);
  assert.deepEqual(fake.calls.filter((call) => !call.startsWith("print")), [`bootout gui/501/${LABEL}`]);

  const again = adoptLegacyHome({ home, launchd: fake.environment, legacyLabel: LABEL });
  assert.equal(again.moved, false, "a second run finds nothing to move");
});

test("adopt with no old daemon loaded moves without asking launchd to stop anything", (t) => {
  withoutRelocation(t);
  const { home, old, target } = legacyHome(t);
  const fake = launchd({ loaded: false });
  const adopted = adoptLegacyHome({ home, launchd: fake.environment, legacyLabel: LABEL });
  assert.equal(adopted.moved, true);
  assert.equal(adopted.stoppedLabel, null);
  assert.equal(fs.existsSync(path.join(target, "ledger.json")), true);
  assert.deepEqual(fake.calls.filter((call) => !call.startsWith("print")), []);
});

test("an existing new home is never merged: the old home and its daemon stay", (t) => {
  withoutRelocation(t);
  const { home, old, target } = legacyHome(t);
  fs.mkdirSync(target, { recursive: true });
  const fake = launchd();
  assert.throws(() => adoptLegacyHome({ home, launchd: fake.environment, legacyLabel: LABEL }), (error) => error.code === "home_conflict" && error.message.includes(old) && error.message.includes(target));
  assert.equal(fs.readFileSync(path.join(old, "ledger.json"), "utf8"), '{"schema":"kept"}');
  assert.equal(fake.loaded, true);
  assert.deepEqual(fake.calls, []);
});

test("a failed rename starts the old daemon again from its untouched plist", (t) => {
  withoutRelocation(t);
  const { home, old } = legacyHome(t);
  // The move target's parent is a file, so the rename fails after the stop.
  fs.mkdirSync(path.join(home, ".hide"), { mode: 0o700 });
  const fake = launchd();
  const blocker = path.join(home, ".hide", "hcoord");
  fs.symlinkSync(path.join(home, "nowhere"), blocker);
  assert.throws(() => adoptLegacyHome({ home, launchd: fake.environment, legacyLabel: LABEL }), (error) => error.code === "home_conflict");
  fs.unlinkSync(blocker);
  // A daemon still holding the old home's lock outside launchd stops the move after the bootout.
  fs.writeFileSync(path.join(old, "api.sock.lock"), `${process.ppid}\n`);
  assert.throws(() => adoptLegacyHome({ home, launchd: fake.environment, legacyLabel: LABEL }), (error) => error.code === "adopt_failed" && /not run by launchd/.test(error.message) && /runs from the old home again/.test(error.message));
  assert.equal(fake.loaded, true, "the old daemon is loaded again");
  assert.deepEqual(fake.calls.filter((call) => !call.startsWith("print")), [`bootout gui/501/${LABEL}`, `bootstrap gui/501 ${path.join(home, "Library", "LaunchAgents", `${LABEL}.plist`)}`]);
  assert.equal(fs.existsSync(path.join(old, "ledger.json")), true);
});

test("a daemon that will not stop leaves the old home as it is", (t) => {
  withoutRelocation(t);
  const { home, old, target } = legacyHome(t);
  const fake = launchd({ fail: ["bootout"] });
  assert.throws(() => adoptLegacyHome({ home, launchd: fake.environment, legacyLabel: LABEL }), (error) => error.code === "adopt_failed" && /nothing was moved/.test(error.message));
  assert.equal(fs.existsSync(path.join(old, "ledger.json")), true);
  assert.equal(fs.existsSync(target), false);
});

test("a linked old home and a relocated hcoord are never moved", (t) => {
  withoutRelocation(t);
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "hcoord-home-"));
  t.after(() => fs.rmSync(home, { recursive: true, force: true }));
  fs.mkdirSync(path.join(home, "elsewhere"));
  fs.symlinkSync(path.join(home, "elsewhere"), path.join(home, ".hcoord"));
  assert.throws(() => adoptLegacyHome({ home, launchd: launchd().environment, legacyLabel: LABEL }), (error) => error.code === "unsafe_home");
  assert.equal(fs.existsSync(path.join(home, ".hide")), false);
  process.env.HCOORD_HOME = path.join(home, "relocated");
  assert.throws(() => adoptLegacyHome({ home }), (error) => error.code === "relocated");
});

test("an isolated HOME never names the account's label for the old daemon", (t) => {
  withoutRelocation(t);
  const { home, old } = legacyHome(t);
  const fake = launchd();
  const adopted = adoptLegacyHome({ home, launchd: fake.environment });
  assert.equal(adopted.moved, true);
  assert.equal(adopted.stoppedLabel, null);
  assert.deepEqual(fake.calls, [], "only the account's own ~/.hcoord ran under the plain label");
});

test("the command moves only its own HOME's ~/.hcoord: --from is refused and no other folder is touched", (t) => {
  withoutRelocation(t);
  const { home, old } = legacyHome(t);
  const documents = path.join(home, "Documents");
  fs.mkdirSync(documents);
  fs.writeFileSync(path.join(documents, ".draft.tmp"), "mine");
  const cli = path.join(path.dirname(new URL(import.meta.url).pathname), "../../dist/hcoord/cli.js");
  const run = spawnSync(process.execPath, [cli, "home", "adopt", "--from", documents, "--json"], { env: { ...process.env, HOME: home }, encoding: "utf8" });
  assert.notEqual(run.status, 0);
  assert.equal(JSON.parse(run.stdout.trim().split("\n").at(-1)).error.code, "invalid_argument");
  assert.equal(fs.readFileSync(path.join(documents, ".draft.tmp"), "utf8"), "mine");
  assert.equal(fs.existsSync(path.join(old, "ledger.json")), true);
  assert.equal(fs.existsSync(path.join(home, ".hide")), false);
});
