// The local measurement CLI uses the existing compiled providers on the real
// pinned Herdr. This is tool plumbing, never authenticated model acceptance.
import { test, expect } from "@playwright/test";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { herdrBinary } from "./herdr-fixture";
import { copyFixtureShim } from "./shims/build";

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
def fail(self,*args,**kwargs):
 original_workspace(self,*args,**kwargs)
 raise RuntimeError('injected_after_registered_private_workspace')
original_workspace=Runtime.new_workspace
with patch.object(Runtime,'__init__',initialize):
 if os.environ.get('LIVE_CHECK_FIXTURE_CASE')=='failure':
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

function assertClean(run: string) {
  const report = JSON.parse(fs.readFileSync(path.join(run, "report.json"), "utf8"));
  expect(report.configuration.failures).toEqual([]);
  expect(report.cleanup).toMatchObject({ confirmed: true, probe_removed: true, socket_removed: true });
  expect(fs.existsSync(path.join(run, "probe"))).toBe(false);
  expect(fs.readFileSync(path.join(run, "daemon-home", ".claude.json"), "utf8")).toBe(originalConfig);
  expect(fs.statSync(path.join(run, "daemon-home", ".claude.json")).mode & 0o777).toBe(0o640);
  return report;
}

test("live check guards preserve bytes, refuse aliases, and end detached children", () => {
  test.skip(process.platform === "win32", "The local measurement runs on Unix hosts");
  const result = spawnSync("python3", ["-m", "unittest", "discover", "-s", "../scripts/tests", "-p", "test_agent_live_*.py"],
    { encoding: "utf8", timeout: 30_000, maxBuffer: 1024 * 1024 });
  expect(result.error, result.stderr).toBeUndefined();
  expect(result.status, result.stdout + result.stderr).toBe(0);
});

test("live check retains the full scene matrix and rejects unsafe picker or plan input", async () => {
  test.skip(process.platform === "win32", "Native measurement's process guardian supports Unix hosts");
  test.setTimeout(240_000);
  const { root, run, args } = fixtureRoot();
  const result = spawnSync("python3", args,
  { encoding: "utf8", timeout: 220_000, maxBuffer: 1024 * 1024 });
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
  } catch (error) {
    // Keep the private tool report and its screen reads beside other CI logs.
    const evidence = process.env.HIDE_E2E_SCREENSHOT_DIR;
    if (evidence && fs.existsSync(run)) fs.cpSync(run, path.join(evidence, path.basename(root)), { recursive: true });
    throw error;
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test("live check refuses operator socket and state before a server starts", () => {
  test.skip(process.platform === "win32", "Unix local tool");
  const { root, run, args } = fixtureRoot();
  try {
    for (const [flag, value] of [["--socket", path.join(process.env.HOME!, ".config", "herdr", "herdr.sock")],
      ["--state-dir", path.join(process.env.HOME!, ".hide", "state")]]) {
      const destination = run + flag;
      const modified = [...args];
      modified[modified.indexOf("--run-dir") + 1] = destination;
      const result = spawnSync("python3", [...modified, flag, value], { encoding: "utf8", timeout: 10_000 });
      expect(result.status, result.stdout + result.stderr).toBe(2);
      const report = JSON.parse(fs.readFileSync(path.join(destination, "report.json"), "utf8"));
      expect(report.herdr).toEqual({});
      expect(report.agents).toEqual([]);
      expect(report.failures).toHaveLength(1);
      expect(fs.existsSync(path.join(destination, "state", "hided.json"))).toBe(false);
      expect(fs.existsSync(path.join(destination, "probe"))).toBe(false);
    }
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

test("live check tears down a started runtime on failure and Ctrl-C", async () => {
  test.skip(process.platform === "win32", "Unix signal and private socket contract");
  test.setTimeout(120_000);
  for (const scenario of ["failure", "hold"]) {
    const { root, run, args } = fixtureRoot();
    let child: ReturnType<typeof spawn> | undefined;
    try {
      const env = { ...process.env, LIVE_CHECK_FIXTURE_CASE: scenario };
      if (scenario === "failure") {
        const result = spawnSync("python3", args, { encoding: "utf8", env, timeout: 45_000 });
        expect(result.status, result.stdout + result.stderr).toBe(2);
        const report = assertClean(run);
        expect(report.herdr.version).toContain("herdr");
        expect(report.failures[0].reason).toBe("injected_after_registered_private_workspace");
      } else {
        args[args.indexOf("--scene-seconds") + 1] = "120";
        child = spawn("python3", args, { env, stdio: ["ignore", "pipe", "pipe"] });
        let output = "";
        child.stdout!.on("data", chunk => { if (output.length < 1024 * 1024) output += chunk; });
        child.stderr!.on("data", chunk => { if (output.length < 1024 * 1024) output += chunk; });
        const completed = new Promise<number | null>((resolve, reject) => {
          child!.once("error", reject);
          child!.once("exit", resolve);
        });
        const identity = path.join(run, "daemon-home", ".live-check-child.json");
        await expect.poll(() => fs.existsSync(identity), { timeout: 40_000 }).toBe(true);
        const fixture = JSON.parse(fs.readFileSync(identity, "utf8"));
        const state = path.join(run, "state", "hided.json");
        expect(fs.existsSync(state)).toBe(true);
        const daemon = JSON.parse(fs.readFileSync(state, "utf8"));
        child.kill("SIGINT");
        expect(await completed, output).toBe(2);
        assertClean(run);
        assertEnded(fixture.pid);
        assertEnded(daemon.pid);
        expect(fixture.socket.length).toBeGreaterThan(0);
        expect(fs.existsSync(fixture.socket)).toBe(false);
      }
    } finally {
      if (child && child.exitCode === null && child.signalCode === null) {
        const running = child;
        await new Promise<void>(resolve => {
          const timer = setTimeout(() => running.kill("SIGKILL"), 10_000);
          running.once("exit", () => { clearTimeout(timer); resolve(); });
          running.kill("SIGTERM");
        });
      }
      fs.rmSync(root, { recursive: true, force: true });
    }
  }
});
