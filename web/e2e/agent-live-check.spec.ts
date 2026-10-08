// The local measurement CLI uses the existing compiled providers on the real
// pinned Herdr. This is tool plumbing, never authenticated model acceptance.
import { test, expect } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { herdrBinary } from "./herdr-fixture";
import { copyFixtureShim } from "./shims/build";
import { runPython, startPython } from "./live-check-process";

const originalConfig = '{"fixture":"before-run"}\n';
// Fault injection stays in the test runner. Production has no hidden route to
// mutate operator configuration or fake successful native delivery.
const toolRunner = `
import os,sys
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0,str(Path('../scripts').resolve()))
from agent_live_check.cli import main
from agent_live_check.runtime import Runtime
original=Runtime.__init__
def initialize(self,*args,**kwargs):
 original(self,*args,**kwargs)
 file=self.home/'.claude.json'
 file.write_bytes(${JSON.stringify(originalConfig)}.encode())
 file.chmod(0o640)
 peer=self.home/'.claude/settings.local.json'
 peer.parent.mkdir(mode=0o700,parents=True,exist_ok=True)
 peer.write_bytes(${JSON.stringify(originalConfig)}.encode())
def fail(self,*args,**kwargs):
 original_workspace(self,*args,**kwargs)
 if os.environ.get('LIVE_CHECK_FIXTURE_CASE')=='configuration':
  (self.home/'.claude/settings.local.json').write_bytes(b'owned temporary settings')
  os.link(self.home/'.claude.json',self.home/'.claude/unexpected-link')
 raise RuntimeError('injected_after_registered_private_workspace')
original_workspace=Runtime.new_workspace
with patch.object(Runtime,'__init__',initialize):
 if os.environ.get('LIVE_CHECK_FIXTURE_CASE') in ('failure','configuration'):
  with patch.object(Runtime,'new_workspace',fail): raise SystemExit(main(sys.argv[1:]))
 raise SystemExit(main(sys.argv[1:]))
`;

function fixtureRoot() {
  const runs = path.resolve("..", "agents", "runs");
  fs.mkdirSync(runs, { recursive: true });
  const root = fs.mkdtempSync(path.join(runs, "live-check-e2e-"));
  fs.chmodSync(root, 0o700);
  const bin = path.join(root, "bin");
  fs.mkdirSync(bin, { mode: 0o700 });
  for (const name of ["claude", "codex"]) {
    const program = path.join(bin, name);
    copyFixtureShim("claude-shim", program);
    fs.chmodSync(program, 0o700);
  }
  const run = path.join(root, "measurement");
  return { root, run, args: ["-c", toolRunner, "--agents", "claude-code,codex",
    "--fixture-bin", bin, "--herdr-bin", herdrBinary(), "--run-dir", run, "--scene-seconds", "1"] };
}

function assertEnded(pid: number) {
  expect(() => process.kill(pid, 0), `owned process ${pid} survived cleanup`).toThrow();
}

function assertClean(run: string, configurationFailure = false) {
  const report = JSON.parse(fs.readFileSync(path.join(run, "report.json"), "utf8"));
  if (configurationFailure) {
    expect(report.configuration.failures).toContainEqual({ path: path.join(run, "daemon-home", ".claude.json"),
      reason: "config_not_private_regular_file" });
    expect(report.configuration.inventory_checked).toBe(false);
    expect(report.configuration.directory_changes).toBeNull();
    expect(report.configuration.restored).toContainEqual({ path: path.join(run, "daemon-home", ".claude", "settings.local.json"),
      result: "restored" });
  } else expect(report.configuration.failures).toEqual([]);
  expect(report.cleanup).toMatchObject({ confirmed: true, probe_removed: true, socket_removed: true });
  expect(fs.existsSync(path.join(run, "probe"))).toBe(false);
  expect(fs.readFileSync(path.join(run, "daemon-home", ".claude.json"), "utf8")).toBe(originalConfig);
  expect(fs.statSync(path.join(run, "daemon-home", ".claude.json")).mode & 0o777).toBe(0o640);
  expect(fs.readFileSync(path.join(run, "daemon-home", ".claude", "settings.local.json"), "utf8")).toBe(originalConfig);
  return report;
}

test("live check guards preserve bytes, refuse aliases, and end detached children", async () => {
  test.skip(process.platform === "win32", "The local measurement runs on Unix hosts");
  const result = await runPython(["-m", "unittest", "discover", "-s", "../scripts/tests", "-p", "test_agent_live_*.py"],
    { timeout: 30_000 });
  expect(result.error, result.stderr).toBeUndefined();
  expect(result.status, result.stdout + result.stderr).toBe(0);
});

test("live check retains the full scene matrix and rejects unsafe picker or plan input", async () => {
  test.skip(process.platform === "win32", "Native measurement's process guardian supports Unix hosts");
  test.setTimeout(240_000);
  const { root, run, args } = fixtureRoot();
  let clean = false;
  const result = await runPython(args, { timeout: 220_000 });
  try {
    expect(result.error, result.stderr).toBeUndefined();
    expect(result.status, result.stdout + result.stderr).toBe(1);
    const report = assertClean(run);
    expect(report.fixture).toBe(true);
    expect(report.herdr.manifests.length).toBeGreaterThan(0);
    expect(report.configuration.failures).toEqual([]);
    expect(report.configuration.restored).toContainEqual({ path: path.join(run, "daemon-home", ".claude.json"), result: "restored" });
    expect(report.cleanup).toMatchObject({ confirmed: true, probe_removed: true, socket_removed: true });
    expect(fs.existsSync(path.join(run, "probe"))).toBe(false);
    expect(report.agents).toHaveLength(2);
    for (const agent of report.agents) {
      expect(agent.scenes.map((row: { scene: string }) => row.scene).sort()).toEqual([
        "rest", "working", "shell_approval", "file_approval", "question", "plan_approval",
        "model_picker", "resume_picker", "mcp_approval", "startup"].sort());
      expect(agent.scenes.every((row: { arrival: string }) => row.arrival === "reached")).toBe(true);
      expect(agent.verdict).toBe("unsafe");
      expect(agent.scenes.some((row: { scene: string; effect: string }) =>
        ["model_picker", "resume_picker", "plan_approval"].includes(row.scene)
        && ["selection", "approval", "resumed_session"].includes(row.effect))).toBe(true);
    }
    expect(fs.readFileSync(path.join(run, "report.md"), "utf8")).toContain("unsafe");
    clean = true;
  } catch (error) {
    // Keep the private tool report and its screen reads beside other CI logs.
    const evidence = process.env.HIDE_E2E_SCREENSHOT_DIR;
    if (evidence && fs.existsSync(run)) fs.cpSync(run, path.join(evidence, path.basename(root)), { recursive: true });
    throw error;
  } finally {
    if (clean) fs.rmSync(root, { recursive: true, force: true });
  }
});

test("live check refuses operator socket and state before a server starts", async () => {
  test.skip(process.platform === "win32", "Unix local tool");
  const { root, run, args } = fixtureRoot();
  let clean = false;
  try {
    for (const [flag, value] of [["--socket", path.join(process.env.HOME!, ".config", "herdr", "herdr.sock")],
      ["--state-dir", path.join(process.env.HOME!, ".hide", "state")]]) {
      const destination = run + flag;
      const modified = [...args];
      modified[modified.indexOf("--run-dir") + 1] = destination;
      const result = await runPython([...modified, flag, value], { timeout: 10_000 });
      expect(result.error, result.stderr).toBeUndefined();
      expect(result.status, result.stdout + result.stderr).toBe(2);
      const report = JSON.parse(fs.readFileSync(path.join(destination, "report.json"), "utf8"));
      expect(report.herdr).toEqual({});
      expect(report.agents).toEqual([]);
      expect(report.failures).toHaveLength(1);
      expect(fs.existsSync(path.join(destination, "state", "hided.json"))).toBe(false);
      expect(fs.existsSync(path.join(destination, "probe"))).toBe(false);
    }
    clean = true;
  } finally { if (clean) fs.rmSync(root, { recursive: true, force: true }); }
});

test("live check tears down a started runtime on failure and Ctrl-C", async () => {
  test.skip(process.platform === "win32", "Unix signal and private socket contract");
  test.setTimeout(120_000);
  for (const scenario of ["failure", "configuration", "hold"]) {
    const { root, run, args } = fixtureRoot();
    let running: ReturnType<typeof startPython> | undefined;
    let clean = false;
    try {
      const env = { ...process.env, LIVE_CHECK_FIXTURE_CASE: scenario };
      if (scenario !== "hold") {
        const result = await runPython(args, { env, timeout: 45_000 });
        expect(result.error, result.stderr).toBeUndefined();
        expect(result.status, result.stdout + result.stderr).toBe(2);
        const report = assertClean(run, scenario === "configuration");
        expect(report.herdr.version).toContain("herdr");
        expect(report.failures[0].reason).toBe("injected_after_registered_private_workspace");
        clean = true;
      } else {
        args[args.indexOf("--scene-seconds") + 1] = "120";
        running = startPython(args, { env, timeout: 110_000 });
        const identity = path.join(run, "daemon-home", ".live-check-child.json");
        await expect.poll(() => fs.existsSync(identity), { timeout: 40_000 }).toBe(true);
        const fixture = JSON.parse(fs.readFileSync(identity, "utf8"));
        const state = path.join(run, "state", "hided.json");
        expect(fs.existsSync(state)).toBe(true);
        const daemon = JSON.parse(fs.readFileSync(state, "utf8"));
        running.child.kill("SIGINT");
        const result = await running.completed;
        expect(result.error, result.stderr).toBeUndefined();
        expect(result.status, result.stdout + result.stderr).toBe(2);
        assertClean(run);
        assertEnded(fixture.pid);
        assertEnded(daemon.pid);
        expect(fixture.socket.length).toBeGreaterThan(0);
        expect(fs.existsSync(fixture.socket)).toBe(false);
        clean = true;
      }
    } finally {
      if (running) await running.stop();
      if (clean) fs.rmSync(root, { recursive: true, force: true });
    }
  }
});
