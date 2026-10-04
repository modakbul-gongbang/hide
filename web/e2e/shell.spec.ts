import { expect, test, type Page } from "@playwright/test";
import { type ChildProcess } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { herdrBinary, linkFixtureTranscripts, startHerdr, type HerdrFixture } from "./herdr-fixture";
import { enterWorkspace } from "./wire";
import { spawnFixtureProcess, fixtureProcessFailure, fixtureExecutable, fixtureHomeEnv, fixtureOpenCommand, fixtureToolPath, inheritedFixtureEnv, stopFixtureProcess } from "./platform-fixture";
import { cleanupAfterFailure, ownUntilWorkerExit } from "./worker-owned";

type Daemon = {
  origin: string;
  token: string;
  /** Kills the daemon and removes its state directory. */
  stop: () => void;
};

/** `herdr` lends the daemon its transcripts and its `claude` label provider. */
async function startHided(extra: Record<string, string> = {}, herdr?: HerdrFixture): Promise<Daemon> {
  const dir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "hide-e2e-")));
  let child: ChildProcess | undefined;
  let spawnAttempted = false;
  let spawnFailed: Error | null = null;
  let cleaned = false;
  const incomplete = () => new Error(`hided fixture cleanup incomplete; preserve ${dir}: child exit unconfirmed (PID: ${child?.pid ?? "unavailable"}); wait for the recorded child's exit/close and owned cleanup, or confirm its exit before removing this retained root after worker loss`);
  const owned = ownUntilWorkerExit(() => {
    if (cleaned) return;
    if (spawnAttempted && !child) throw incomplete();
    if (child && !(spawnFailed && child.pid === undefined)) stopFixtureProcess(child);
    fs.rmSync(dir, { recursive: true, force: true });
    cleaned = true;
  });
  const stop = owned.stop;
  try {
    if (herdr) linkFixtureTranscripts(herdr, dir);
    const bin = path.resolve("..", "target", "debug", fixtureExecutable("hided"));
    const env = {
      ...inheritedFixtureEnv(),
      ...fixtureHomeEnv(dir),
      HIDE_STATE_DIR: path.join(dir, "hide"),
      HIDE_KEEP_ALIVE: "1",
      HIDE_PORT: "0",
      HIDED_UI_DIR: path.resolve("dist"),
      // Resolve the fixture binary even when this test creates no server.
      // Keep HERDR_SOCKET_PATH absent in those missing-socket/auth cases.
      HERDR_BIN_PATH: herdrBinary(),
      PATH: herdr?.fixturePath ?? fixtureToolPath(path.join(dir, "bin")),
      HIDE_OPEN_COMMAND: fixtureOpenCommand(herdr?.root, dir),
      ...extra,
    };
    spawnAttempted = true;
    child = spawnFixtureProcess(bin, [], dir, { env, stdio: ["ignore", "pipe", "pipe"] });
    child.once("error", (error) => { spawnFailed = error; });
    let closeLog = () => {};
    child.once("close", () => closeLog());
    // Two pipes share the evidence file; neither may end it before child close.
    const logDir = process.env.HIDE_E2E_SCREENSHOT_DIR;
    if (logDir) {
      const log = fs.createWriteStream(path.join(logDir, `hided-${path.basename(dir)}.log`), { flags: "a" });
      closeLog = () => log.end();
      child.stdout?.pipe(log, { end: false });
      child.stderr?.pipe(log, { end: false });
    }
    for (let i = 0; i < 50; i += 1) {
      fixtureProcessFailure(child);
      if (spawnFailed) throw new Error(`hided did not start from ${bin}: ${(spawnFailed as Error).message}`, { cause: spawnFailed });
      const statePath = path.join(dir, "hide", "hided.json");
      if (fs.existsSync(statePath)) {
        try {
          // The file may be mid-write on the first read; the next tick reads it whole.
          const state = JSON.parse(fs.readFileSync(statePath, "utf8")) as {
            port: number;
            token: string;
          };
          const origin = `http://127.0.0.1:${state.port}`;
          const health = await fetch(`${origin}/health`);
          if (health.ok) {
            return { origin, token: state.token, stop };
          }
        } catch {
          /* still starting */
        }
      }
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    throw new Error("hided did not write a state file");
  } catch (error) {
    cleanupAfterFailure(error, stop);
  }
}

test("sidebar shows a missing Herdr socket and the badge goes live", async ({ page }) => {
  const daemon = await startHided();
  try {
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await expect(page.getByText("Herdr 소켓 없음")).toBeVisible();
    await expect(page.locator("[data-connection]")).toHaveCount(0);
  } finally {
    daemon.stop();
  }
});

test("a bad token shows the refused connection state", async ({ page }) => {
  const daemon = await startHided();
  try {
    await page.goto(`${daemon.origin}/#token=${"aa".repeat(32)}`);
    await expect(page.getByText("연결 거부")).toBeVisible();
  } finally {
    daemon.stop();
  }
});

async function typedTextEchoes(page: Page, marker: string): Promise<void> {
  await page.locator('[data-pane-view][data-focused="true"] .xterm-helper-textarea').focus();
  await page.keyboard.type(marker);
  await expect
    .poll(() => page.evaluate(() => window.__hideProbe?.screenText() ?? ""), { timeout: 10_000 })
    .toContain(marker);
}

// @platform: Real PTY input and echo through the platform's Herdr and its shell.
test("a sidebar row click switches the pane and typed text echoes there", { tag: "@platform" }, async ({ page }) => {
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    daemon = await startHided({ HERDR_SOCKET_PATH: herdr.socket, HERDR_BIN_PATH: herdr.bin }, herdr);
    const [first, second] = herdr.panes;
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await enterWorkspace(page);
    // The sidebar opens on Projects; the rows this test clicks are the Agents list's.
    await page.locator('[data-sidebar-mode="agents"]').click();
    await expect(page.locator(`[data-pane="${first}"]`)).toContainText("Agent one");
    await expect(page.locator(`[data-pane="${second}"]`)).toContainText("Agent two");
    await expect(page.locator('[data-pane-view][data-focused="true"]')).toHaveAttribute("data-pane-view", first);

    await page.locator(`[data-pane="${second}"]`).click();
    await expect(page.locator('[data-pane-view][data-focused="true"]')).toHaveAttribute("data-pane-view", second);
    await expect
      .poll(() => page.evaluate(() => window.__hideProbe?.screenText() ?? ""), { timeout: 10_000 })
      .toContain("claude");
    await typedTextEchoes(page, "echo-two-9f3a");

    await page.locator(`[data-pane="${first}"]`).click();
    await expect(page.locator('[data-pane-view][data-focused="true"]')).toHaveAttribute("data-pane-view", first);
    await expect
      .poll(() => page.evaluate(() => window.__hideProbe?.screenText() ?? ""), { timeout: 10_000 })
      .toContain("claude");
    await typedTextEchoes(page, "echo-one-7c1d 한글");
    // Run evidence for a reviewer; CI sets no directory and takes none.
    const screenshots = process.env.HIDE_E2E_SCREENSHOT_DIR;
    if (screenshots) {
      await page.screenshot({ path: path.join(screenshots, "s1-click-flow.png") });
    }
    await expect(page.evaluate(() => window.__hideProbe?.screenText() ?? "")).resolves.not.toContain(
      "echo-two-9f3a",
    );
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
