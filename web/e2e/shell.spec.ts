import { expect, test } from "@playwright/test";
import { spawn, type ChildProcess } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

async function startHided(): Promise<{
  process: ChildProcess;
  origin: string;
  token: string;
  dir: string;
}> {
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
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  for (let i = 0; i < 50; i += 1) {
    const statePath = path.join(dir, "hide", "hided.json");
    if (fs.existsSync(statePath)) {
      const state = JSON.parse(fs.readFileSync(statePath, "utf8")) as {
        port: number;
        token: string;
      };
      const origin = `http://127.0.0.1:${state.port}`;
      try {
        const health = await fetch(`${origin}/health`);
        if (health.ok) {
          return { process: child, origin, token: state.token, dir };
        }
      } catch {
        /* still starting */
      }
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  child.kill();
  throw new Error("hided did not write a state file");
}

test("sidebar shows a missing Herdr socket and the badge goes live", async ({ page }) => {
  const daemon = await startHided();
  try {
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await expect(page.getByText("Herdr 소켓 없음")).toBeVisible();
    await expect(page.locator("[data-connection]")).toHaveCount(0);
  } finally {
    daemon.process.kill();
  }
});

test("a bad token shows the refused connection state", async ({ page }) => {
  const daemon = await startHided();
  try {
    await page.goto(`${daemon.origin}/#token=${"aa".repeat(32)}`);
    await expect(page.locator("[data-connection]")).toHaveAttribute("data-connection", /reconnecting|gone/);
  } finally {
    daemon.process.kill();
  }
});
