// Exact-window evidence for the candidate the fixture owns. Other platforms
// keep renderer captures; native window capture here uses the macOS boundary.
import { expect, type ElectronApplication } from "@playwright/test";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { animationsFinished, compositorPresents } from "../../web/e2e/wait";

export async function captureNativeWindow(app: ElectronApplication, name: string, facts: Record<string, unknown>): Promise<void> {
  const evidence = process.env.HIDE_E2E_SCREENSHOT_DIR;
  if (!evidence || process.platform !== "darwin") return;
  const page = await app.firstWindow();
  await animationsFinished(page);
  await compositorPresents(page);
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
  const nativeWindows = JSON.parse(listed.stdout) as { kCGWindowNumber: number; kCGWindowOwnerName: string }[];
  // Retain the exact candidate identity even if the compositor refuses capture.
  fs.writeFileSync(path.join(evidence, `${name}.json`), JSON.stringify({ candidate, nativeWindows, ...facts }, null, 2));
  expect(nativeWindows, "candidate must have exactly one on-screen native window").toHaveLength(1);
  const windowId = String(nativeWindows[0]!.kCGWindowNumber);
  const file = path.join(evidence, `${name}.png`);
  const shot = spawnSync("/usr/sbin/screencapture", ["-x", "-o", "-l", windowId, file], { encoding: "utf8", timeout: 10_000 });
  let method = "screencapture";
  if (shot.status !== 0) {
    // ScreenCaptureKit owns modern macOS capture. The exact window and PID
    // are checked again there; this never substitutes a renderer screenshot.
    const helper = path.join(evidence, "native-window-capture");
    if (!fs.existsSync(helper)) {
      const built = spawnSync("/usr/bin/swiftc", ["-parse-as-library", path.join(__dirname, "native-window.swift"), "-o", helper], { encoding: "utf8", timeout: 60_000 });
      expect(built.status, built.stderr).toBe(0);
    }
    const captured = spawnSync(helper, [windowId, String(candidate.pid), file], { encoding: "utf8", timeout: 20_000 });
    if (captured.status === 0) method = captured.stdout.trim();
    else {
      // The permission-owning native service can observe a covered window.
      // Its name-only target is safe only if every matching app is this PID.
      const ownerName = nativeWindows[0]!.kCGWindowOwnerName;
      const ownerQuery = `ObjC.import("AppKit"); const apps = $.NSWorkspace.sharedWorkspace.runningApplications; const pids = []; for (let i=0; i<Number(apps.count); i++) { const a=apps.objectAtIndex(i); if (ObjC.unwrap(a.localizedName) === ${JSON.stringify(ownerName)}) pids.push(Number(a.processIdentifier)); } JSON.stringify(pids);`;
      const owners = spawnSync("osascript", ["-l", "JavaScript", "-e", ownerQuery], { encoding: "utf8", timeout: 10_000 });
      expect(owners.status, owners.stderr).toBe(0);
      expect(JSON.parse(owners.stdout)).toEqual([candidate.pid]);
      const observed = spawnSync("ocu", ["call", "get_app_state", "--args", JSON.stringify({ app: ownerName })], { encoding: "utf8", timeout: 30_000, maxBuffer: 32 * 1024 * 1024 });
      fs.writeFileSync(path.join(evidence, `${name}-native-state.json`), observed.stdout);
      expect(observed.status, observed.stderr).toBe(0);
      const after = spawnSync("osascript", ["-l", "JavaScript", "-e", ownerQuery], { encoding: "utf8", timeout: 10_000 });
      expect(after.status, after.stderr).toBe(0);
      expect(JSON.parse(after.stdout)).toEqual([candidate.pid]);
      const result = JSON.parse(observed.stdout) as { isError?: boolean; content: { type: string; data?: string; mimeType?: string }[] };
      expect(result.isError, captured.stderr).not.toBe(true);
      const images = result.content.filter(item => item.type === "image");
      expect(images).toHaveLength(1);
      expect(images[0]!.mimeType).toBe("image/png");
      fs.writeFileSync(file, Buffer.from(images[0]!.data!, "base64"));
      method = "native service exact sole named candidate";
    }
  }
  fs.writeFileSync(path.join(evidence, `${name}.json`), JSON.stringify({ candidate, nativeWindows, method, legacyFailure: shot.status === 0 ? undefined : shot.stderr.trim(), ...facts }, null, 2));
}
