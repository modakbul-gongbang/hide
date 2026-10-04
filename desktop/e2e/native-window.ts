// Exact-window evidence for the candidate the fixture owns. Other platforms
// keep renderer captures; native window capture here uses the macOS boundary.
import { expect, type ElectronApplication } from "@playwright/test";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

export async function captureNativeWindow(app: ElectronApplication, name: string, facts: Record<string, unknown>): Promise<void> {
  const evidence = process.env.HIDE_E2E_SCREENSHOT_DIR;
  if (!evidence || process.platform !== "darwin") return;
  const candidate = await app.evaluate(({ BrowserWindow }) => {
    const windows = BrowserWindow.getAllWindows();
    return { pid: process.pid, executable: process.execPath, windows: windows.length, source: windows[0]!.getMediaSourceId() };
  });
  expect(candidate.windows).toBe(1);
  const shot = spawnSync("/usr/sbin/screencapture", ["-x", "-o", "-l", candidate.source.split(":")[1]!, path.join(evidence, `${name}.png`)], { encoding: "utf8", timeout: 10_000 });
  expect(shot.status, shot.stderr).toBe(0);
  fs.writeFileSync(path.join(evidence, `${name}.json`), JSON.stringify({ candidate, ...facts }, null, 2));
}
