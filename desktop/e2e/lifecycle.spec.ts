// Real launchd lifecycle checks, with only labels derived from this fixture's private HOME.
import { expect } from "@playwright/test";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { herdrBinary } from "../../web/e2e/herdr-fixture";
import { ownUntilWorkerExit } from "../../web/e2e/worker-owned";
import { bootoutTestLabel, hcoordLabel, launchdPid, OPERATOR_HCOORD_LABEL } from "./device-home";
import { DESKTOP_DIR, isolate, REPO, test, type Isolated } from "./fixture";

test.skip(process.platform !== "darwin", "the per-home login service uses macOS launchd");
const cli = path.join(REPO, "plugins", "hcoord", "dist", "hcoord", "cli.js");
const daemon = (run: Isolated, action: string) => spawnSync(process.execPath, [cli, "daemon", action, "--json"], { env: run.env, encoding: "utf8", timeout: 45_000 });
const privateRun = () => isolate({ bin: herdrBinary(), socket: "/tmp/hide-lifecycle-unused.sock" }, "lc");

for (const stopped of [false, true]) {
  test(`daemon uninstall removes its ${stopped ? "stopped" : "running"} service while preserving another home's daemon`, async () => {
    const run = privateRun();
    const other = privateRun();
    const label = hcoordLabel(run.env.HCOORD_HOME!);
    const otherLabel = hcoordLabel(other.env.HCOORD_HOME!);
    try {
      for (const owned of [run, other]) {
        const started = daemon(owned, "start");
        expect(started.status, started.stdout + started.stderr).toBe(0);
        await expect.poll(() => JSON.parse(daemon(owned, "status").stdout).value.stale === true).toBe(false);
      }
      const otherPid = launchdPid(otherLabel);
      expect(otherPid).toMatch(/^\d+$/);
      if (stopped) expect(daemon(run, "stop").status).toBe(0);
      const removed = daemon(run, "uninstall");
      expect(removed.status, removed.stdout + removed.stderr).toBe(0);
      expect(JSON.parse(removed.stdout).value.label).toBe(label);
      expect(launchdPid(label)).toBeNull();
      expect(fs.existsSync(path.join(run.env.HOME!, "Library", "LaunchAgents", `${label}.plist`))).toBe(false);
      expect(fs.existsSync(run.env.HCOORD_HOME!)).toBe(true);
      expect(launchdPid(otherLabel)).toBe(otherPid);
      expect(JSON.parse(daemon(other, "status").stdout).value.stale).not.toBe(true);
      expect(daemon(run, "uninstall").status).toBe(0);
    } finally {
      run.cleanup();
      other.cleanup();
    }
  });

  test(`fixture cleanup unloads its ${stopped ? "manually stopped" : "running"} coordinator before removing HOME`, async () => {
    const run = privateRun();
    const label = hcoordLabel(run.env.HCOORD_HOME!);
    const operator = launchdPid(OPERATOR_HCOORD_LABEL);
    try {
      const started = daemon(run, "start");
      expect(started.status, started.stdout + started.stderr).toBe(0);
      await expect.poll(() => launchdPid(label)).not.toBeNull();
      if (stopped) {
        await expect.poll(() => JSON.parse(daemon(run, "status").stdout).value.stale === true).toBe(false);
        const stop = daemon(run, "stop");
        expect(stop.status, stop.stdout + stop.stderr).toBe(0);
        expect(launchdPid(label)).not.toBeNull();
      }
      run.cleanup();
      expect(launchdPid(label), "removing HOME must not leave a loaded job whose executable is gone").toBeNull();
      expect(fs.existsSync(run.root)).toBe(false);
      expect(launchdPid(OPERATOR_HCOORD_LABEL)).toBe(operator);
      run.cleanup();
    } finally {
      // Also clean the pre-fix regression run if its assertion failed.
      bootoutTestLabel(label);
      run.cleanup();
    }
  });
}

test("an unload failure reports incomplete cleanup and retains the owned home", () => {
  const run = privateRun();
  const label = hcoordLabel(run.env.HCOORD_HOME!);
  const started = daemon(run, "start");
  expect(started.status, started.stdout + started.stderr).toBe(0);
  const fakeBin = path.join(run.root, "refused-launchctl");
  fs.mkdirSync(fakeBin);
  fs.writeFileSync(path.join(fakeBin, "launchctl"), "#!/bin/sh\nexit 77\n", { mode: 0o755 });
  const originalPath = process.env.PATH;
  try {
    process.env.PATH = fakeBin;
    expect(() => run.cleanup()).toThrow(/fixture cleanup incomplete/);
    expect(fs.existsSync(run.root)).toBe(true);
    expect(fs.existsSync(path.join(run.env.HOME!, "Library", "LaunchAgents", `${label}.plist`))).toBe(true);
  } finally {
    process.env.PATH = originalPath;
    run.cleanup();
  }
});

test("a worker exit releases its coordinator without reaching the test finally block", async () => {
  test.setTimeout(60_000);
  const owner = privateRun();
  const marker = path.join(owner.root, "child-home.json");
  const probe = path.join(owner.root, "worker.spec.ts");
  const config = path.join(owner.root, "playwright.config.ts");
  fs.writeFileSync(config, `export default { testDir: ${JSON.stringify(owner.root)}, workers: 1, timeout: 15000, reporter: "line" };\n`);
  fs.writeFileSync(probe, `
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import { isolate, test } from ${JSON.stringify(path.join(__dirname, "fixture"))};
test("exit before test teardown", () => {
  const run = isolate({bin: "/usr/bin/true", socket: "/tmp/hide-lifecycle-unused.sock"}, "wx");
  fs.writeFileSync(${JSON.stringify(marker)}, JSON.stringify({root:run.root, home:run.env.HOME, data:run.env.HCOORD_HOME}));
  const started = spawnSync(${JSON.stringify(process.execPath)}, [${JSON.stringify(cli)}, "daemon", "start", "--json"], {env:run.env,encoding:"utf8",timeout:10000});
  if(started.status!==0) throw new Error(started.stdout + started.stderr);
  process.exit(23);
});\n`);
  let output = "";
  const child = spawn(process.execPath, [path.join(DESKTOP_DIR, "node_modules", "@playwright", "test", "cli.js"), "test", "--config", config], { cwd: DESKTOP_DIR, env: { ...process.env, HIDE_CLI_PATH: undefined } });
  const ownerOfChild = ownUntilWorkerExit(() => { if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL"); });
  const collect = (chunk: Buffer) => {
    output += String(chunk);
    if (output.length > 32_768) { output = output.slice(0, 32_768) + "\nworker probe output exceeded its 32 KiB cap"; ownerOfChild.stop(); }
  };
  child.stdout.on("data", collect);
  child.stderr.on("data", collect);
  const deadline = setTimeout(() => child.kill("SIGKILL"), 40_000);
  try {
    const exit = await new Promise<number | null>((resolve, reject) => { child.once("error", reject); child.once("exit", resolve); });
    expect(exit, output).toBe(1);
    expect(output).toContain("worker process exited unexpectedly");
    const owned = JSON.parse(fs.readFileSync(marker, "utf8")) as { root: string; data: string };
    expect(launchdPid(hcoordLabel(owned.data))).toBeNull();
    expect(fs.existsSync(owned.root)).toBe(false);
  } finally {
    clearTimeout(deadline);
    ownerOfChild.stop();
    // A failing regression still releases only the exact home the child reported.
    if (fs.existsSync(marker)) {
      const owned = JSON.parse(fs.readFileSync(marker, "utf8")) as { root: string; data: string };
      bootoutTestLabel(hcoordLabel(owned.data));
      fs.rmSync(owned.root, { recursive: true, force: true });
    }
    owner.cleanup();
  }
});

test.describe("automatic ownership", () => {
  test.describe.configure({ mode: "serial" });
  let abandoned: Isolated;
  let label: string;
  test("a failed test leaves its resources to the fixture owner", () => {
    abandoned = privateRun();
    label = hcoordLabel(abandoned.env.HCOORD_HOME!);
    const started = daemon(abandoned, "start");
    expect(started.status, started.stdout + started.stderr).toBe(0);
    test.fail();
    throw new Error("intentional failure after starting an owned coordinator");
  });
  test("the next test sees the failed test's label and home removed", () => {
    try {
      expect(launchdPid(label)).toBeNull();
      expect(fs.existsSync(abandoned.root)).toBe(false);
    } finally {
      bootoutTestLabel(label);
      abandoned.cleanup();
    }
  });
});
