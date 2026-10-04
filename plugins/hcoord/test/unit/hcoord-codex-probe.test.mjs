import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { createFakeRemote } from "../helpers/fake-remote.mjs";

const CLI = path.resolve(import.meta.dirname, "../../dist/hcoord/cli.js");
const HERDR = path.resolve(import.meta.dirname, "../../dist/hcoord/herdr.js");
const REMOTE = path.resolve(import.meta.dirname, "../../dist/hcoord/remote.js");
const start = `
const {startSpawnedAgent}=require(process.argv[1]);
try { startSpawnedAgent(JSON.parse(process.argv[2])); console.log(JSON.stringify({ok:true})); }
catch (error) { console.log(JSON.stringify({ok:false,code:error.code,detail:error.detail})); }
`;

function fixture(t) {
  const fake = createFakeRemote(CLI);
  t.after(() => fake.cleanup());
  fake.addMachine("target", "target-ssh");
  fake.installHcoord("target");
  for (const machine of ["local", "target"]) {
    fake.addAgent(machine, "probe-pane", { name: "existing", kind: "codex", session: "old", instance: "terminal" });
  }
  const features = (machine, text) => fs.writeFileSync(path.join(fake.root, "h", machine, "codex-features"), text);
  const launch = (machine, nativeArgs = ["--model", "chosen"]) => {
    const record = { key: "probe", name: "probed", kind: "codex", machine, hostScope: "default", pane: "probe-pane", nativeArgs };
    const result = spawnSync(process.execPath, ["-e", start, HERDR, JSON.stringify(record)], { env: fake.env(), encoding: "utf8", timeout: 15_000 });
    assert.equal(result.status, 0, result.stderr);
    return JSON.parse(result.stdout.trim());
  };
  return { fake, features, launch };
}

test("the target feature list controls local and remote Codex start arguments", (t) => {
  const { fake, features, launch } = fixture(t);
  features("local", "apps stable true\n");
  features("target", "daemon_auto_start stable true\n");
  assert.equal(launch("target").ok, true);
  const remoteStart = fake.calls("target").find((args) => args[1] === "start");
  assert.deepEqual(remoteStart.slice(8), ["--no-daemon", "--model", "chosen"]);
  assert.equal(fs.existsSync(path.join(fake.root, "h", "local", "codex-calls.jsonl")), false, "the HQ Codex was not probed for a remote start");
  assert.equal(launch("local").ok, true);
  const localStart = fake.calls("local").find((args) => args[1] === "start");
  assert.deepEqual(localStart.slice(8), ["--model", "chosen"]);
});

test("a feature probe failure refuses agent start with target provenance", (t) => {
  const { fake, launch } = fixture(t);
  fake.flag("target", "codex-fails");
  const result = launch("target");
  assert.equal(result.code, "codex_probe_failed");
  assert.equal(result.detail.machine, "target");
  assert.equal(result.detail.exitStatus, 7);
  assert.equal(result.detail.unfinishedStep, "codex_probe");
  assert.equal(fake.calls("target").some((args) => args[1] === "start"), false);
});

test("an explicit no-daemon flag is preserved once without another probe", (t) => {
  const { fake, launch } = fixture(t);
  assert.equal(launch("target", ["--no-daemon", "--model", "chosen"]).ok, true);
  assert.equal(fs.existsSync(path.join(fake.root, "h", "target", "codex-calls.jsonl")), false);
  const args = fake.calls("target").find((args) => args[1] === "start");
  assert.equal(args.filter((arg) => arg === "--no-daemon").length, 1);
});

test("a successful old-version probe is not cached after a target upgrade", (t) => {
  const { fake } = fixture(t);
  const file = path.join(fake.root, "h", "target", "codex-features");
  fs.writeFileSync(file, "apps stable true\n");
  const code = `const fs=require("node:fs"), {codexHasDaemon}=require(process.argv[1]);
    const before=codexHasDaemon("target");
    fs.writeFileSync(process.argv[2],"daemon_auto_start stable false\\n");
    console.log(JSON.stringify([before,codexHasDaemon("target")]));`;
  const result = spawnSync(process.execPath, ["-e", code, REMOTE, file], { env: fake.env(), encoding: "utf8", timeout: 15_000 });
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout.trim()), [false, true]);
});

test("the target Codex process has its own deadline and output bound", (t) => {
  const { fake, launch } = fixture(t);
  fake.flag("target", "codex-hangs");
  const started = Date.now();
  const timeout = launch("target");
  assert.equal(timeout.code, "codex_probe_timeout");
  assert.ok(Date.now() - started < 10_000);
  fake.flag("target", "codex-hangs", false);
  fake.flag("target", "codex-floods");
  assert.equal(launch("target").code, "codex_probe_capacity");
  assert.equal(fake.calls("target").some((args) => args[1] === "start"), false);
});

test("authentication failure is retryable and unreachable backoff stops repeated SSH", (t) => {
  const { fake, features } = fixture(t);
  features("target", "daemon_auto_start stable true\n");
  const flagFile = path.join(fake.root, "h", "target", "auth-denied");
  const downFile = path.join(fake.root, "h", "target", "down");
  fs.writeFileSync(flagFile, "1");
  const code = `const fs=require("node:fs"), {codexHasDaemon}=require(process.argv[1]);
    const failure=()=>{try {codexHasDaemon("target");return null;}catch(e){return e.code;}};
    const auth=failure(); fs.unlinkSync(process.argv[2]); const recovered=codexHasDaemon("target");
    fs.writeFileSync(process.argv[3],"1"); const down=failure(); const deferred=failure();
    console.log(JSON.stringify({auth,recovered,down,deferred}));`;
  const result = spawnSync(process.execPath, ["-e", code, REMOTE, flagFile, downFile], { env: fake.env(), encoding: "utf8", timeout: 15_000 });
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout.trim()), { auth: "auth_failed", recovered: true, down: "machine_unreachable", deferred: "machine_unreachable" });
  assert.equal(fake.sshCommands("target").length, 3, "the deferred retry starts no SSH process");
});
