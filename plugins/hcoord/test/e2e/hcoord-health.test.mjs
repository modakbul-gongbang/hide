import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";
import test from "node:test";
import { createFakeRemote } from "../helpers/fake-remote.mjs";
import { hq } from "../helpers/hcoord-hq.mjs";
import { daemonPlist } from "../../dist/hcoord/platform.js";

const CLI = path.resolve(import.meta.dirname, "../../dist/hcoord/cli.js");
const minutesAgo = (minutes) => new Date(Date.now() - minutes * 60_000).toISOString();

test("the LaunchAgent restarts only unsuccessful exits", () => {
  const plist = daemonPlist("/Users/example", ["/usr/local/bin/node", "/opt/hcoord/cli.js", "daemon", "run"]);
  assert.match(plist, /<key>KeepAlive<\/key><dict><key>SuccessfulExit<\/key><false\/><\/dict>/);
  assert.match(plist, /<key>RunAtLoad<\/key><true\/>/);
});

test("three unexpected exits in ten minutes raise one Herdr notification and a warning on every command", async (t) => {
  const fake = createFakeRemote(CLI);
  t.after(() => fake.cleanup());
  const coordinator = hq(t, fake);
  for (let crash = 0; crash < 3; crash += 1) { await coordinator.start(); await coordinator.stop("SIGKILL"); }
  await coordinator.start();
  await coordinator.until(() => fake.notifications().length > 0, "the restarted daemon sends a Herdr notification");
  const notices = fake.notifications();
  assert.equal(notices.length, 1, "the restarted daemon notifies once");
  assert.match(notices[0][0], /hcoord daemon unstable: 3 unexpected daemon exits within 10 minutes; see .*daemon\.err\.log/);
  const status = coordinator.run("status");
  assert.equal(status.status, 0);
  assert.match(status.stderr.split("\n")[0], /^hcoord warning: coordinator daemon unstable since .*: 3 unexpected daemon exits within 10 minutes; logs: .*daemon\.log, .*daemon\.err\.log$/);
  assert.equal(JSON.parse(status.stdout).ok, true, "JSON stdout stays parseable");
  coordinator.run("inbox");
  assert.equal(fake.notifications().length, 1, "later commands repeat the warning without another notification");
});

test("clean stops never count as instability", async (t) => {
  const fake = createFakeRemote(CLI);
  t.after(() => fake.cleanup());
  const coordinator = hq(t, fake);
  for (let restart = 0; restart < 4; restart += 1) { await coordinator.start(); await coordinator.stop("SIGTERM"); }
  await coordinator.start();
  assert.equal(coordinator.run("status").stderr, "");
  assert.equal(fake.notifications().length, 0);
});

// A stop signal that lands between the start record and the socket being ready
// once killed the daemon without its clean mark, so a clean stop read as a crash.
test("a stop signal during startup still ends with a clean mark", async (t) => {
  const fake = createFakeRemote(CLI);
  t.after(() => fake.cleanup());
  const coordinator = hq(t, fake);
  const healthFile = path.join(coordinator.dir, "health.json");
  const starts = () => { try { return JSON.parse(fs.readFileSync(healthFile, "utf8")).starts; } catch { return []; } };
  for (let attempt = 0; attempt < 5; attempt += 1) {
    const before = starts().length;
    const daemon = spawn(process.execPath, [CLI, "daemon", "run"], { env: coordinator.env, stdio: "ignore" });
    const exited = new Promise((resolve) => daemon.once("exit", (code, signal) => resolve({ code, signal })));
    while (starts().length === before && daemon.exitCode === null) await new Promise((resolve) => setImmediate(resolve));
    daemon.kill("SIGTERM");
    assert.deepEqual(await exited, { code: 0, signal: null }, `attempt ${attempt} exits through the stop path`);
  }
  assert.deepEqual(starts().filter((entry) => entry.cleanAt === null), [], "every recorded start ended cleanly");
});

test("a start that never answers for a minute warns even without a request, and ten ready minutes clear it", async (t) => {
  const fake = createFakeRemote(CLI);
  t.after(() => fake.cleanup());
  const coordinator = hq(t, fake);
  fs.mkdirSync(coordinator.dir, { recursive: true });
  fs.writeFileSync(path.join(coordinator.dir, "health.json"), JSON.stringify({ starts: [{ pid: 999999, at: minutesAgo(2), readyAt: null, cleanAt: null }] }));
  const down = coordinator.run("inbox");
  assert.match(down.stderr.split("\n")[0], /did not answer on its socket within 1 minute of starting/);
  assert.equal(fake.notifications().length, 1);

  await coordinator.start();
  const health = JSON.parse(fs.readFileSync(path.join(coordinator.dir, "health.json"), "utf8"));
  assert.equal(coordinator.run("status").stderr === "", false, "a fresh start does not clear the warning at once");
  health.starts.at(-1).at = minutesAgo(11);
  health.starts.at(-1).readyAt = minutesAgo(11);
  fs.writeFileSync(path.join(coordinator.dir, "health.json"), JSON.stringify(health));
  assert.equal(coordinator.run("status").stderr, "", "ten answering minutes without a restart clear the warning");
  assert.equal(fs.existsSync(path.join(coordinator.dir, "alert.json")), false);
});

test("a corrupt health record is reported, not read as a healthy daemon", async (t) => {
  const fake = createFakeRemote(CLI);
  t.after(() => fake.cleanup());
  const coordinator = hq(t, fake);
  fs.mkdirSync(coordinator.dir, { recursive: true });
  fs.writeFileSync(path.join(coordinator.dir, "alert.json"), "{not json");
  assert.match(coordinator.run("inbox").stderr, /^hcoord warning: daemon health is unknown: .*alert\.json is unreadable/);
});
