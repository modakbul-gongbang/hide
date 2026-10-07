// The local measurement CLI uses the existing compiled providers on the real
// pinned Herdr. This is tool plumbing, never authenticated model acceptance.
import { test, expect } from "@playwright/test";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { herdrBinary } from "./herdr-fixture";
import { copyFixtureShim } from "./shims/build";

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
  const result = spawnSync("python3", ["../scripts/agent-live-check.py", "--agents", "claude-code,codex",
    "--fixture-bin", bin, "--herdr-bin", herdrBinary(), "--run-dir", run, "--scene-seconds", "1"],
  { encoding: "utf8", timeout: 220_000, maxBuffer: 1024 * 1024 });
  try {
    expect(result.error, result.stderr).toBeUndefined();
    expect(result.status, result.stdout + result.stderr).toBe(1);
    const report = JSON.parse(fs.readFileSync(path.join(run, "report.json"), "utf8"));
    expect(report.fixture).toBe(true);
    expect(report.herdr.manifests.length).toBeGreaterThan(0);
    expect(report.configuration.failures).toEqual([]);
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
