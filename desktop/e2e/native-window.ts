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
    const window = windows[0]!;
    return { pid: process.pid, executable: process.execPath, windows: windows.length, source: window.getMediaSourceId(), visible: window.isVisible(), minimized: window.isMinimized(), bounds: window.getBounds() };
  });
  expect(candidate.windows).toBe(1);
  const listed = spawnSync("osascript", ["-l", "JavaScript", "-e", `
    ObjC.import("CoreGraphics");
    JSON.stringify(ObjC.deepUnwrap(ObjC.castRefToObject($.CGWindowListCopyWindowInfo($.kCGWindowListOptionOnScreenOnly | $.kCGWindowListExcludeDesktopElements, 0)))
      .filter(w => w.kCGWindowLayer === 0 && w.kCGWindowOwnerPID === ${candidate.pid}));
  `], { encoding: "utf8", timeout: 10_000 });
  expect(listed.status, listed.stderr).toBe(0);
  const nativeWindows = JSON.parse(listed.stdout) as { kCGWindowNumber: number }[];
  // Retain the exact candidate identity even if the compositor refuses capture.
  fs.writeFileSync(path.join(evidence, `${name}.json`), JSON.stringify({ candidate, nativeWindows, ...facts }, null, 2));
  expect(nativeWindows, "candidate must have exactly one on-screen native window").toHaveLength(1);
  const shot = spawnSync("/usr/sbin/screencapture", ["-x", "-o", "-l", String(nativeWindows[0]!.kCGWindowNumber), path.join(evidence, `${name}.png`)], { encoding: "utf8", timeout: 10_000 });
  expect(shot.status, shot.stderr).toBe(0);
}
