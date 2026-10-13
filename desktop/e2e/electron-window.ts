// Captures only the renderer of the running candidate owned by Playwright.
// OS window chrome, compositor presentation and native input remain separate checks.
import { expect, type ElectronApplication } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { animationsFinished, compositorPresents } from "../../web/e2e/wait";

export async function captureElectronWindow(app: ElectronApplication, name: string, facts: Record<string, unknown>): Promise<void> {
  const evidence = process.env.HIDE_E2E_SCREENSHOT_DIR;
  if (!evidence) return;
  const page = await app.firstWindow();
  await animationsFinished(page);
  await compositorPresents(page);
  const candidate = await app.evaluate(({ BrowserWindow }) => {
    const windows = BrowserWindow.getAllWindows();
    const window = windows[0]!;
    return { pid: process.pid, executable: process.execPath, windows: windows.length, windowId: window.id, visible: window.isVisible(), minimized: window.isMinimized(), bounds: window.getBounds() };
  });
  expect(candidate.windows).toBe(1);
  fs.mkdirSync(evidence, { recursive: true });
  await page.screenshot({ path: path.join(evidence, `${name}.png`) });
  fs.writeFileSync(path.join(evidence, `${name}.json`), JSON.stringify({ ...facts, candidate, method: "Playwright Electron window renderer", limits: "OS window chrome, compositor presentation and native input were not captured" }, null, 2));
}
