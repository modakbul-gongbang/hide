// Three-column acceptance in a real, privately owned desktop window.
// Native captures are optional run artifacts; the behavioral assertions run in CI.
import { expect, type ElectronApplication, type Page } from "@playwright/test";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { performance } from "node:perf_hooks";
import { startHerdr, type HerdrFixture } from "../../web/e2e/herdr-fixture";
import { countSent, enterWorkspace, showExplorer } from "../../web/e2e/wire";
import { isolate, launchShell, test } from "./fixture";

type Probe = {
  arm: (marker: string) => void;
  waitArmed: () => Promise<{ arrival_ms: number; write_ms: number }>;
  paneText: (pane: string) => string;
};

async function paint(page: Page): Promise<void> {
  await page.evaluate(() => new Promise<void>((done) => requestAnimationFrame(() => requestAnimationFrame(() => done()))));
}

async function nativeCapture(app: ElectronApplication, page: Page, name: string): Promise<void> {
  const dir = process.env.HIDE_E2E_SCREENSHOT_DIR;
  if (!dir || process.platform !== "darwin") return;
  await paint(page);
  const source = await app.evaluate(({ BrowserWindow }) => {
    const windows = BrowserWindow.getAllWindows();
    if (windows.length !== 1) throw new Error("expected exactly one candidate window");
    const window = windows[0]!;
    return { pid: process.pid, window: window.getMediaSourceId(), bounds: window.getBounds(), zoom: window.webContents.getZoomFactor(), focused: window.isFocused() };
  });
  const geometry = await page.evaluate(() => ({
    body: document.querySelector<HTMLElement>("[data-column-row=true]")!.clientWidth,
    columns: Array.from(document.querySelectorAll<HTMLElement>("[data-column]")).map((column) => ({ name: column.dataset.column, width: column.clientWidth, visible: column.checkVisibility() })),
  }));
  const captured = spawnSync("/usr/sbin/screencapture", ["-x", "-o", "-l", source.window.split(":")[1]!, path.join(dir, `${name}.png`)], { encoding: "utf8" });
  expect(captured.status, captured.stderr).toBe(0);
  fs.writeFileSync(path.join(dir, `${name}.json`), JSON.stringify({ method: "exact candidate window screencapture without activation", collectedAt: new Date().toISOString(), source, geometry }, null, 2));
}

async function bodyWidth(app: ElectronApplication, page: Page, width: number): Promise<void> {
  const inset = await page.evaluate(() => innerWidth - document.querySelector<HTMLElement>("[data-column-row=true]")!.clientWidth);
  await app.evaluate(({ BrowserWindow }, width) => {
    const window = BrowserWindow.getAllWindows()[0]!;
    window.setSize(width, window.getSize()[1]!);
    // macOS can clamp to a small CI screen. Preserve the requested CSS body
    // width with the granted content size, and record zoom in native evidence.
    window.webContents.setZoomFactor(window.getContentBounds().width / width);
  }, width + inset);
  await expect.poll(() => page.locator("[data-column-row=true]").evaluate((element) => element.clientWidth)).toBe(width);
}

async function stableCount(count: () => number): Promise<number> {
  let previous = -1;
  await expect.poll(async () => {
    await new Promise((done) => setTimeout(done, 150));
    const current = count();
    const same = current === previous;
    previous = current;
    return same;
  }).toBe(true);
  return previous;
}

function sendText(herdr: HerdrFixture, pane: string, text: string): void {
  const sent = spawnSync(herdr.bin, ["pane", "send-text", pane, text], { env: herdr.env, encoding: "utf8", timeout: 3000 });
  expect(sent.status, sent.stderr).toBe(0);
}

async function echo(page: Page, herdr: HerdrFixture, pane: string, phase: string): Promise<unknown> {
  const samples: { marker: string; cli_return_ms: number; arrival_ms: number; write_ms: number; latency_ms: number }[] = [];
  for (let index = 0; index < 20; index += 1) {
    const marker = `columns_${phase}_${String(index).padStart(3, "0")}`;
    await page.evaluate((marker) => (window as unknown as { __hideProbe: Probe }).__hideProbe.arm(marker), marker);
    sendText(herdr, pane, `${marker}\n`);
    const cli_return_ms = performance.timeOrigin + performance.now();
    const sample = await page.evaluate(() => (window as unknown as { __hideProbe: Probe }).__hideProbe.waitArmed());
    samples.push({ marker, cli_return_ms, ...sample, latency_ms: sample.write_ms - cli_return_ms });
    await new Promise((done) => setTimeout(done, 80));
  }
  const values = samples.map((sample) => sample.latency_ms).sort((a, b) => a - b);
  return { method: "send-text CLI return to xterm write callback, 20 native-window samples, nearest-rank p95; candidate observation, no baseline comparison", samples, p95_ms: values[Math.ceil(values.length * 0.95) - 1] };
}

test("Workspace columns preserve geometry, dock once on release and show native idle and driven echo", async () => {
  const herdr = await startHerdr({ agents: false });
  const run = isolate(herdr, "workspace-columns");
  let app: ElectronApplication | null = null;
  try {
    const checkout = path.join(fs.realpathSync(herdr.root), "fixture");
    fs.writeFileSync(path.join(checkout, "notes.md"), "# 작업 기록\n\n한글과 English를 나란히 읽습니다.\n");
    const longName = "한글과 English 작업 기록 - 긴 파일 제목과 경로 확인.md";
    fs.writeFileSync(path.join(checkout, longName), "# 긴 제목의 작업 기록\n\n한글과 English가 함께 있는 파일을 읽습니다.\n");
    ({ app } = await launchShell(run.env));
    const page = await app.firstWindow();
    const sent = countSent(page);
    const url = new URL(page.url());
    url.searchParams.set("probe", "1");
    await page.goto(url.href);
    await expect.poll(() => page.evaluate(() => typeof (window as unknown as { __hideProbe?: Probe }).__hideProbe)).toBe("object");
    await enterWorkspace(page, "fixture");
    await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.setSize(1440, 900));
    const workspace = page.locator("[data-workspace-screen]");
    await expect(workspace).toHaveAttribute("data-file-views", "off");
    await expect(workspace).toHaveAttribute("data-tools", "off");
    await expect(page.locator("[data-column=agents]")).toBeVisible();
    const identity = await app.evaluate(({ BrowserWindow }) => ({ pid: process.pid, executable: process.execPath, window: BrowserWindow.getAllWindows()[0]!.getMediaSourceId(), screen: BrowserWindow.getAllWindows()[0]!.getBounds() }));
    const dir = process.env.HIDE_E2E_SCREENSHOT_DIR;
    if (dir) fs.writeFileSync(path.join(dir, "columns-identity.json"), JSON.stringify({ ...identity, daemonPid: run.daemonPid(), socket: herdr.socket, state: run.env.HIDE_STATE_DIR, profile: run.env.HIDE_DESKTOP_USER_DATA_DIR, home: run.env.HOME }, null, 2));
    await nativeCapture(app, page, "columns-agents-only");
    await showExplorer(page);
    await page.locator('[data-explorer-row$="/notes.md"]').click();
    await expect(page.locator("[data-editor-body] .cm-content")).toBeVisible();
    // A Tools call in a wide body must not override File Views-first
    // fallback later. Only a call made in the two-column body replaces it.
    await bodyWidth(app, page, 1600);
    await page.locator('[data-tool-tab="explorer"]').click();
    await bodyWidth(app, page, 1100);
    await expect(workspace).toHaveAttribute("data-file-views", "shown");
    await expect(workspace).toHaveAttribute("data-tools", "hidden");
    await nativeCapture(app, page, "columns-wide-tools-call-to-mid");
    await page.locator('[data-column-toggle="tools"]').click();
    await expect(workspace).toHaveAttribute("data-file-views", "hidden");
    await expect(workspace).toHaveAttribute("data-tools", "shown");
    await bodyWidth(app, page, 1600);
    await bodyWidth(app, page, 1100);
    await expect(workspace).toHaveAttribute("data-file-views", "shown");
    await expect(workspace).toHaveAttribute("data-tools", "hidden");
    await bodyWidth(app, page, 1116);
    await expect(workspace).toHaveAttribute("data-workspace-body", "wide");
    const widths = async () => Promise.all(["agents", "views", "tools"].map((column) => page.locator(`[data-column=${column}]`).evaluate((element) => element.getBoundingClientRect().width)));
    await expect.poll(widths).toEqual([480, 360, 260]);
    await nativeCapture(app, page, "columns-body-1116");
    await bodyWidth(app, page, 1115);
    await expect(workspace).toHaveAttribute("data-workspace-body", "mid");
    await expect(workspace).toHaveAttribute("data-tools", "hidden");
    await bodyWidth(app, page, 1100);
    expect(await page.evaluate(() => innerWidth)).toBe(1440);
    await nativeCapture(app, page, "columns-1440-sidebar-open-fallback");
    await page.locator('[data-column-toggle="tools"]').click();
    await expect(workspace).toHaveAttribute("data-file-views", "hidden");
    await expect(workspace).toHaveAttribute("data-tools", "shown");
    await page.locator('[data-column-toggle="views"]').click();
    await bodyWidth(app, page, 848);
    await expect(workspace).toHaveAttribute("data-workspace-body", "mid");
    await expect.poll(() => page.locator("[data-column=agents]").evaluate((element) => element.clientWidth)).toBe(480);
    await nativeCapture(app, page, "columns-body-848");
    await bodyWidth(app, page, 847);
    await expect(workspace).toHaveAttribute("data-workspace-body", "narrow");
    await expect(workspace).toHaveAttribute("data-file-views", "hidden");
    await page.locator('[data-column-toggle="views"]').click();
    await expect(workspace).toHaveAttribute("data-file-views", "shown");
    await nativeCapture(app, page, "columns-body-847-called-views");
    await bodyWidth(app, page, 1116);
    await expect.poll(widths).toEqual([480, 360, 260]);
    // Use spare Agent width: at the exact minima, hiding Tools can give
    // its width to File Views without changing the terminal grid.
    await bodyWidth(app, page, 1600);
    const resizeCount = () => sent.get("terminal_resize") ?? 0;
    const panesShown = await page.locator("[data-pane-view]").count();
    const toggles: { column: string; state: string; resizes: number }[] = [];
    for (const [column, state] of [["tools", "off"], ["views", "off"], ["views", "shown"]]) {
      const before = await stableCount(resizeCount);
      await page.locator(`[data-column-toggle="${column}"]`).click();
      await expect(workspace).toHaveAttribute(column === "views" ? "data-file-views" : "data-tools", state!);
      await expect.poll(resizeCount).toBeGreaterThan(before);
      const resizes = await stableCount(resizeCount) - before;
      expect(resizes).toBe(panesShown);
      toggles.push({ column: column!, state: state!, resizes });
    }
    await bodyWidth(app, page, 1116);
    await stableCount(resizeCount);
    const divider = page.locator('[data-column-divider="views"]');
    const box = (await divider.boundingBox())!;
    const before = resizeCount();
    await page.mouse.move(box.x + box.width / 2, box.y + 100);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2 + 32, box.y + 100, { steps: 6 });
    await expect(page.locator("[data-column-guide=true]")).toBeVisible();
    expect(resizeCount()).toBe(before);
    await nativeCapture(app, page, "columns-divider-guide");
    await page.mouse.up();
    await expect(page.locator("[data-column-guide=true]")).toHaveCount(0);
    await expect.poll(resizeCount).toBeGreaterThan(before);
    const resizeAfterRelease = await stableCount(resizeCount) - before;
    expect(resizeAfterRelease).toBe(panesShown);
    const saved = () => JSON.parse(fs.readFileSync(path.join(run.env.HIDE_STATE_DIR!, "workspace-views.json"), "utf8")) as { workspaces: { path: string; views_width: number }[] };
    await expect.poll(() => saved().workspaces.find((entry) => entry.path === checkout)?.views_width).toBe(596);
    await page.reload();
    await expect(page.locator('[data-column-divider="views"]')).toHaveAttribute("aria-valuenow", "596");
    await nativeCapture(app, page, "columns-restored-width");
    const pane = herdr.panes[0];
    sendText(herdr, pane, "stty -echo -icanon; echo COLUMNS_IDLE_READY; cat\n");
    await expect.poll(() => page.evaluate((id) => (window as unknown as { __hideProbe: Probe }).__hideProbe.paneText(id), pane)).toContain("COLUMNS_IDLE_READY");
    const idle = await echo(page, herdr, pane, "idle");
    sendText(herdr, pane, "\u0003");
    const driver = path.join(herdr.root, "columns-driven.sh");
    fs.writeFileSync(driver, '#!/bin/bash\nstty -echo -icanon\n(while true; do printf "columns driven output\\n"; sleep 0.008; done) &\ndriver=$!\ntrap \'kill "$driver" 2>/dev/null; wait "$driver" 2>/dev/null\' EXIT HUP TERM\necho COLUMNS_DRIVEN_READY\ncat\n');
    sendText(herdr, pane, `bash '${driver}'\n`);
    await expect.poll(() => page.evaluate((id) => (window as unknown as { __hideProbe: Probe }).__hideProbe.paneText(id), pane)).toContain("COLUMNS_DRIVEN_READY");
    const driven = await echo(page, herdr, pane, "driven");
    await nativeCapture(app, page, "columns-driven-terminal");
    if (dir) fs.writeFileSync(path.join(dir, "columns-echo.json"), JSON.stringify({ identity, toggles, divider: { resizeDuringDrag: 0, resizeAfterRelease, panesShown, restoredViewsWidth: 596 }, load: spawnSync("uptime", { encoding: "utf8" }).stdout.trim(), workload: "two visible shell panes; idle cat and driven line every 8ms in measured pane; 20 markers per phase; no baseline or speed claim", idle, driven }, null, 2));
    if (dir) {
      await showExplorer(page);
      await page.locator(`[data-explorer-row$="/${longName}"]`).click();
      await expect(page.locator("[data-view-tab-bar]")).toContainText(longName);
      for (const theme of ["dark", "light"] as const) {
        await page.locator("[data-open-settings]").click();
        await page.locator('[data-settings-tab="appearance"]').click();
        await page.locator(`[data-theme-option="${theme}"]`).click();
        await expect(page.locator("html")).toHaveClass(new RegExp(`\\b${theme}\\b`));
        await page.keyboard.press("Escape");
        await expect(page.locator('[data-settings="true"]')).toHaveCount(0);
        await nativeCapture(app, page, `columns-${theme}-long-title`);
        await page.locator('[data-column-toggle="views"]').click();
        await expect(workspace).toHaveAttribute("data-file-views", "off");
        await nativeCapture(app, page, `columns-${theme}-tools-only`);
        await page.locator('[data-column-toggle="views"]').click();
        await expect(workspace).toHaveAttribute("data-file-views", "shown");
      }
    }
  } finally {
    await app?.close().catch(() => undefined);
    run.cleanup();
    herdr.stop();
  }
});

// Changing Workspaces while the pointer is captured cancels the old guide;
// releasing it must not land a width in the newly front Workspace.
test("a Workspace switch cancels its column drag without changing another Workspace", async () => {
  const herdr = await startHerdr({ agents: false });
  const run = isolate(herdr, "column-drag-switch");
  let app: ElectronApplication | null = null;
  try {
    const first = path.join(herdr.root, "fixture");
    const beta = path.join(herdr.root, "beta");
    fs.mkdirSync(beta);
    for (const folder of [first, beta]) {
      fs.writeFileSync(path.join(folder, "notes.md"), "# Workspace notes\n");
      const init = spawnSync("git", ["-C", folder, "init", "-q"], { encoding: "utf8" });
      expect(init.status, init.stderr).toBe(0);
    }
    herdr.run(["workspace", "create", "--cwd", beta, "--label", "beta", "--env", `PATH=${herdr.fixturePath}`, "--no-focus"]);
    ({ app } = await launchShell(run.env));
    const page = await app.firstWindow();
    const sent = countSent(page);
    await page.reload();
    await enterWorkspace(page, "fixture");
    await bodyWidth(app, page, 1600);
    await page.locator('[data-sidebar-mode="projects"]').click();
    const row = (label: string) => page.locator("[data-project]", { hasText: label }).locator("[data-checkout]").first();
    const openNotes = async () => {
      await showExplorer(page);
      await page.locator('[data-explorer-row$="/notes.md"]').click();
      await expect(page.locator("[data-editor-body] .cm-content")).toBeVisible();
    };
    await row("beta").click();
    await expect(row("beta")).toHaveAttribute("aria-current", "true");
    await openNotes();
    await row("fixture").click();
    await expect(row("fixture")).toHaveAttribute("aria-current", "true");
    await openNotes();
    const divider = page.locator('[data-column-divider="views"]');
    const box = (await divider.boundingBox())!;
    await page.mouse.move(box.x + box.width / 2, box.y + 100);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2 - 64, box.y + 100);
    await expect(page.locator("[data-column-guide=true]")).toBeVisible();
    await expect(page.locator("html")).toHaveAttribute("data-view-drag", "col-resize");
    const before = sent.get("workspace_view") ?? 0;
    // Invoke the candidate's Workspace control while capture is held, as a
    // keyboard Workspace switch can do without releasing the divider.
    await row("beta").evaluate((element: HTMLElement) => element.click());
    await expect(row("beta")).toHaveAttribute("aria-current", "true");
    await expect(page.locator("[data-column-guide=true]")).toHaveCount(0);
    await expect(page.locator("html")).not.toHaveAttribute("data-view-drag");
    await page.mouse.up();
    await paint(page);
    expect(sent.get("workspace_view") ?? 0).toBe(before);
    await nativeCapture(app, page, "columns-drag-switch-cancelled");
  } finally {
    await app?.close().catch(() => undefined);
    run.cleanup();
    herdr.stop();
  }
});
