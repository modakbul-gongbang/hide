import { expect, test, type Page } from "@playwright/test";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";

type Daemon = {
  origin: string;
  token: string;
  /** Kills the daemon and removes its state directory. */
  stop: () => void;
};

async function startHided(extra: Record<string, string> = {}): Promise<Daemon> {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hide-e2e-"));
  const bin = path.resolve("..", "target", "debug", "hided");
  const uiDir = path.resolve("dist");
  const env = { ...process.env };
  delete env.HERDR_SOCKET_PATH;
  delete env.HERDR_PANE_ID;
  delete env.HERDR_TAB_ID;
  delete env.HERDR_WORKSPACE_ID;
  delete env.HERDR_ENV;
  const child = spawn(bin, [], {
    env: {
      ...env,
      HOME: dir,
      HIDE_STATE_DIR: path.join(dir, "hide"),
      HIDE_KEEP_ALIVE: "1",
      HIDE_PORT: "0",
      HIDED_UI_DIR: uiDir,
      ...extra,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  // The daemon's structured log is run evidence when a directory is given.
  const logDir = process.env.HIDE_E2E_SCREENSHOT_DIR;
  if (logDir) {
    const log = fs.createWriteStream(path.join(logDir, `hided-${path.basename(dir)}.log`), { flags: "a" });
    child.stdout?.pipe(log);
    child.stderr?.pipe(log);
  }
  const stop = () => {
    child.kill();
    // The daemon may still be writing its state file as it dies; removing the
    // directory under it fails with ENOTEMPTY.
    for (let attempt = 0; attempt < 40; attempt += 1) {
      try {
        fs.rmSync(dir, { recursive: true, force: true });
        return;
      } catch {
        spawnSync("/bin/sleep", ["0.05"]);
      }
    }
    fs.rmSync(dir, { recursive: true, force: true });
  };
  for (let i = 0; i < 50; i += 1) {
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
  stop();
  throw new Error("hided did not write a state file");
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

test("a sidebar row click switches the pane and typed text echoes there", async ({ page }) => {
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    daemon = await startHided({ HERDR_SOCKET_PATH: herdr.socket, HERDR_BIN_PATH: herdr.bin });
    const [first, second] = herdr.panes;
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
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
