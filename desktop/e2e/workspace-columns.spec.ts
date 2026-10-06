// Three-column acceptance in a real, privately owned desktop window.
// Native captures are optional run artifacts; the behavioral assertions run in CI.
import { expect, type ElectronApplication, type Page } from "@playwright/test";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { performance } from "node:perf_hooks";
import { startHerdr, type HerdrFixture } from "../../web/e2e/herdr-fixture";
import { chord } from "../../web/e2e/chords";
import { countSent, enterWorkspace, keyboardFocus, rest, sendEvent, showExplorer } from "../../web/e2e/wire";
import { deviceHome, proveDeviceHome, resetDeviceHome, stageBuild, writeSshConfig } from "./device-home";
import { isolate, launchShell, test } from "./fixture";

type Probe = {
  arm: (marker: string) => void;
  waitArmed: () => Promise<{ arrival_ms: number; write_ms: number }>;
  paneText: (pane: string) => string;
  attachedPanes: () => string[];
  liveTerminals: () => string[];
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
    theme: document.documentElement.className,
    columns: Array.from(document.querySelectorAll<HTMLElement>("[data-column]")).map((column) => ({ name: column.dataset.column, left: column.getBoundingClientRect().left, width: column.clientWidth, mounted: true, visible: column.checkVisibility({ visibilityProperty: true }) })),
    controls: Array.from(document.querySelectorAll<HTMLElement>("[data-column-toggles] button")).map((control) => ({ name: control.getAttribute("aria-label"), pressed: control.getAttribute("aria-pressed"), badge: control.querySelector("[data-column-badge]")?.textContent ?? null, focused: control.matches(":focus-visible") })),
    tooltip: document.querySelector<HTMLElement>('[data-slot="tooltip-content"]')?.textContent ?? null,
  }));
  const captured = spawnSync("/usr/sbin/screencapture", ["-x", "-o", "-l", source.window.split(":")[1]!, path.join(dir, `${name}.png`)], { encoding: "utf8" });
  expect(captured.status, captured.stderr).toBe(0);
  fs.writeFileSync(path.join(dir, `${name}.json`), JSON.stringify({ method: "exact candidate window screencapture without activation", collectedAt: new Date().toISOString(), source, geometry }, null, 2));
}

async function bodyWidth(app: ElectronApplication, page: Page, width: number): Promise<void> {
  const inset = await page.evaluate(() => innerWidth - document.querySelector<HTMLElement>("[data-column-row=true]")!.clientWidth);
  await app.evaluate(({ BrowserWindow }, width) => {
    const window = BrowserWindow.getAllWindows()[0]!;
    // eslint-disable-next-line hide-e2e/window-size-through-fixture -- these bodies are wider than a CI screen and their columns are asserted in whole CSS pixels, which a zoomed page does not give; nothing here orders the window out and in, so macOS keeps the size.
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

async function echo(page: Page, herdr: HerdrFixture, pane: string, phase: string, afterSend?: (index: number) => Promise<void>) {
  const samples: { marker: string; cli_return_ms: number; arrival_ms: number; write_ms: number; latency_ms: number }[] = [];
  for (let index = 0; index < 20; index += 1) {
    const marker = `columns_${phase}_${String(index).padStart(3, "0")}`;
    await page.evaluate((marker) => (window as unknown as { __hideProbe: Probe }).__hideProbe.arm(marker), marker);
    sendText(herdr, pane, `${marker}\n`);
    const cli_return_ms = performance.timeOrigin + performance.now();
    // Keep marker observation running while the real column action lands.
    // Both promises are owned here, including an action or echo failure.
    const [sample] = await Promise.all([
      page.evaluate(() => (window as unknown as { __hideProbe: Probe }).__hideProbe.waitArmed()),
      afterSend?.(index),
    ]);
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
    const init = spawnSync("git", ["-C", checkout, "init", "-q", "--initial-branch=main"], { env: herdr.env, encoding: "utf8" });
    expect(init.status, init.stderr).toBe(0);
    fs.writeFileSync(path.join(checkout, "notes.md"), "# 작업 기록\n\n한글과 English를 나란히 읽습니다.\n");
    const longName = "한글과 English 작업 기록 - 긴 파일 제목과 경로 확인.md";
    fs.writeFileSync(path.join(checkout, longName), "# 긴 제목의 작업 기록\n\n한글과 English가 함께 있는 파일을 읽습니다.\n");
    ({ app } = await launchShell(run.env));
    const page = await app.firstWindow();
    const sent = countSent(page);
    let newViews = 0;
    page.on("websocket", (socket) => socket.on("framesent", (frame) => {
      const event = JSON.parse(String(frame.payload)) as { kind?: string; payload?: { new_view?: boolean } };
      if (event.kind === "terminal_viewport" && event.payload?.new_view === true) newViews += 1;
    }));
    const url = new URL(page.url());
    url.searchParams.set("probe", "1");
    await page.goto(url.href);
    await expect.poll(() => page.evaluate(() => typeof (window as unknown as { __hideProbe?: Probe }).__hideProbe)).toBe("object");
    await enterWorkspace(page, "fixture");
    await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.setSize(1440, 900)); // eslint-disable-line hide-e2e/window-size-through-fixture -- the first Tools call is made in a wide body, wider than a CI screen; nothing here orders the window out and in.
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
    // Tools trades with File Views here, whose 640px leaves 280px above
    // its minimum; the accessible maximum must describe that same range.
    await expect(page.locator('[data-column-divider="tools"]')).toHaveAttribute("aria-valuemax", "635");
    const toolsDivider = page.locator('[data-column-divider="tools"]');
    const beforeToolKeys = await stableCount(() => sent.get("terminal_resize") ?? 0);
    await toolsDivider.press("ArrowLeft");
    await expect(toolsDivider).toHaveAttribute("aria-valuenow", "387");
    await expect.poll(() => page.locator('[data-column="views"]').evaluate((element) => element.clientWidth)).toBe(608);
    await toolsDivider.press("ArrowRight");
    await expect(toolsDivider).toHaveAttribute("aria-valuenow", "355");
    await expect.poll(() => page.locator('[data-column="views"]').evaluate((element) => element.clientWidth)).toBe(640);
    expect(await stableCount(() => sent.get("terminal_resize") ?? 0)).toBe(beforeToolKeys);
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
    // B18: the output and input continue THROUGH toggles and a held drag,
    // rather than only measuring echo after all geometry has settled.
    await bodyWidth(app, page, 1600);
    await stableCount(resizeCount);
    const session = () => page.evaluate(() => {
      const probe = (window as unknown as { __hideProbe: Probe }).__hideProbe;
      return { attached: probe.attachedPanes().sort(), live: probe.liveTerminals().sort() };
    });
    const originalSession = await session();
    const originalElements = await page.evaluateHandle(() => Array.from(document.querySelectorAll("[data-column=agents] .xterm")));
    const beforeNewViews = newViews;
    const transitionActions: { action: string; start_ms: number; end_ms: number; resizes: number }[] = [];
    let drivenToggles;
    let drivenDrag;
    const retained = async () => {
      expect(await session()).toEqual(originalSession);
      expect(newViews).toBe(beforeNewViews);
      expect(await page.evaluate((original) => {
        const current = Array.from(document.querySelectorAll("[data-column=agents] .xterm"));
        return current.length === original.length && current.every((element, index) => element === original[index]);
      }, originalElements)).toBe(true);
      expect(await page.evaluate((id) => (window as unknown as { __hideProbe: Probe }).__hideProbe.paneText(id), pane)).toContain("columns driven output");
    };
    try {
      const calls = [["tools", "shown"], ["views", "off"], ["views", "shown"], ["tools", "off"]] as const;
      drivenToggles = await echo(page, herdr, pane, "toggle", async (index) => {
        if (index % 5 !== 0) return;
        const [column, state] = calls[index / 5]!;
        const before = await stableCount(resizeCount);
        const start_ms = Date.now();
        await page.locator('[data-column-toggle="' + column + '"]').click();
        await expect(workspace).toHaveAttribute(column === "views" ? "data-file-views" : "data-tools", state);
        await expect.poll(resizeCount).toBeGreaterThan(before);
        const resizes = await stableCount(resizeCount) - before;
        expect(resizes).toBe(panesShown);
        await retained();
        transitionActions.push({ action: column + ":" + state, start_ms, end_ms: Date.now(), resizes });
        await nativeCapture(app!, page, "columns-driven-" + column + "-" + state);
      });
      const dragBox = (await page.locator('[data-column-divider="views"]').boundingBox())!;
      const beforeDrag = await stableCount(resizeCount);
      const start_ms = Date.now();
      drivenDrag = await echo(page, herdr, pane, "drag", async (index) => {
        if (index === 0) {
          await page.mouse.move(dragBox.x + dragBox.width / 2, dragBox.y + 100);
          await page.mouse.down();
          await page.mouse.move(dragBox.x + dragBox.width / 2 - 32, dragBox.y + 100, { steps: 6 });
          await expect(page.locator("[data-column-guide=true]")).toBeVisible();
          expect(resizeCount()).toBe(beforeDrag);
          await nativeCapture(app!, page, "columns-driven-drag-held");
        } else if (index === 3) {
          await page.mouse.move(dragBox.x + dragBox.width / 2 - 64, dragBox.y + 100, { steps: 6 });
          expect(resizeCount()).toBe(beforeDrag);
        } else if (index === 5) {
          await page.mouse.up();
          await expect(page.locator("[data-column-guide=true]")).toHaveCount(0);
          await expect.poll(resizeCount).toBeGreaterThan(beforeDrag);
          const resizes = await stableCount(resizeCount) - beforeDrag;
          expect(resizes).toBe(panesShown);
          transitionActions.push({ action: "drag:release", start_ms, end_ms: Date.now(), resizes });
          await nativeCapture(app!, page, "columns-driven-drag-released");
        }
        await retained();
      });
      expect(drivenToggles.samples).toHaveLength(20);
      expect(drivenDrag.samples).toHaveLength(20);
    } finally {
      await page.mouse.up();
      await originalElements.dispose();
    }
    if (dir) fs.writeFileSync(path.join(dir, "columns-echo.json"), JSON.stringify({ identity, toggles, divider: { resizeDuringDrag: 0, resizeAfterRelease, panesShown, restoredViewsWidth: 596 }, transitions: { actions: transitionActions, originalSession, newViewRequests: newViews - beforeNewViews, terminalElementsRetained: true, drivenToggles, drivenDrag }, load: spawnSync("uptime", { encoding: "utf8" }).stdout.trim(), workload: "two visible shell panes; idle cat and driven line every 8ms in measured pane; 20 markers per idle, driven, driven-toggle and driven-drag phase; native captures at action states, no continuous blank-frame recording, baseline or speed claim", idle, driven }, null, 2));
    if (dir) {
      await showExplorer(page);
      await bodyWidth(app, page, 1116);
      await page.locator(`[data-explorer-row$="/${longName}"]`).click();
      await expect(page.locator("[data-view-tab-bar]")).toContainText(longName);
      for (const theme of ["dark", "light"] as const) {
        await page.locator("[data-open-settings]").click();
        await page.locator('[data-settings-tab="general"]').click();
        await page.locator(`[data-theme-option="${theme}"]`).click();
        await expect(page.locator("html")).toHaveClass(new RegExp(`\\b${theme}\\b`));
        await page.keyboard.press("Escape");
        await expect(page.locator('[data-settings="true"]')).toHaveCount(0);
        await nativeCapture(app, page, `columns-${theme}-long-title`);
        await rest(page);
        await nativeCapture(app, page, "columns-" + theme + "-divider-rest");
        const divider = page.locator('[data-column-divider="views"]');
        await divider.hover();
        await expect(divider.locator("[data-column-grip]")).toHaveCSS("opacity", "1");
        await nativeCapture(app, page, "columns-" + theme + "-divider-hover");
        await rest(page);
        await keyboardFocus(page, divider);
        await expect(divider.locator("[data-column-grip]")).toHaveCSS("opacity", "1");
        await nativeCapture(app, page, "columns-" + theme + "-divider-focus");
        await rest(page);
        const gripBox = (await divider.boundingBox())!;
        await page.mouse.move(gripBox.x + gripBox.width / 2, gripBox.y + 100);
        await page.mouse.down();
        await page.mouse.move(gripBox.x + gripBox.width / 2 + 32, gripBox.y + 100);
        await expect(page.locator("[data-column-guide=true]")).toBeVisible();
        await nativeCapture(app, page, "columns-" + theme + "-divider-drag");
        // Return to the starting edge before release, preserving the state
        // used for both themes' design comparison.
        await page.mouse.move(gripBox.x + gripBox.width / 2, gripBox.y + 100);
        await page.mouse.up();
        await expect(page.locator("[data-column-guide=true]")).toHaveCount(0);
        for (const [selector, name, shortcut] of [['[data-open-server="true"]', "Open server", null], ['[data-column-toggle="views"]', "File Views", "⇧⌘B"], ['[data-column-toggle="tools"]', "Tools", "⌘E"]] as const) {
          const control = page.locator(selector);
          const capture = name === "Open server" ? "server" : name === "File Views" ? "views" : "tools";
          await control.hover();
          const tooltip = page.locator('[data-slot="tooltip-content"]');
          await expect(tooltip).toContainText(name);
          if (shortcut) await expect(tooltip).toContainText(shortcut);
          await nativeCapture(app, page, `columns-${theme}-${capture}-tooltip`);
          await rest(page);
          await keyboardFocus(page, control);
          await nativeCapture(app, page, `columns-${theme}-${capture}-focus`);
          await rest(page);
        }
        await page.locator('[data-view-tab-bar] [data-display]').filter({ hasText: longName }).hover();
        await expect(page.locator('[data-slot="tooltip-content"]')).toContainText(path.join(checkout, longName));
        await nativeCapture(app, page, `columns-${theme}-full-file-path-tooltip`);
        await rest(page);
        await bodyWidth(app, page, 1100);
        await expect(workspace).toHaveAttribute("data-file-views", "shown");
        await expect(workspace).toHaveAttribute("data-tools", "hidden");
        await nativeCapture(app, page, `columns-${theme}-body-1100-views`);
        await bodyWidth(app, page, 848);
        await expect(workspace).toHaveAttribute("data-file-views", "shown");
        await nativeCapture(app, page, `columns-${theme}-body-848-views`);
        await bodyWidth(app, page, 847);
        await expect(page.locator('[data-column="agents"]')).toBeVisible();
        await page.locator('[data-column-toggle="views"]').click();
        await expect(workspace).toHaveAttribute("data-file-views", "shown");
        await expect(page.locator('[data-column="agents"]')).toBeHidden();
        await nativeCapture(app, page, `columns-${theme}-body-847-called-views`);
        await bodyWidth(app, page, 1100);
        await page.locator('[data-column-toggle="tools"]').click();
        await expect(workspace).toHaveAttribute("data-tools", "shown");
        await expect(page.locator('[data-column="tools"]')).toHaveJSProperty("clientWidth", 355);
        await expect(page.locator('[data-column="agents"]')).toHaveJSProperty("clientWidth", 737);
        await nativeCapture(app, page, `columns-${theme}-body-1100-called-tools`);
        await bodyWidth(app, page, 848);
        await expect(page.locator('[data-column="tools"]')).toHaveJSProperty("clientWidth", 355);
        await expect(page.locator('[data-column="agents"]')).toHaveJSProperty("clientWidth", 485);
        await nativeCapture(app, page, `columns-${theme}-body-848-called-tools`);
        await bodyWidth(app, page, 847);
        await expect(page.locator('[data-column="agents"]')).toBeVisible();
        await expect(workspace).toHaveAttribute("data-tools", "hidden");
        await nativeCapture(app, page, `columns-${theme}-body-847-agents`);
        await page.locator('[data-column-toggle="tools"]').click();
        await expect(workspace).toHaveAttribute("data-tools", "shown");
        await expect(page.locator('[data-column="agents"]')).toBeHidden();
        await expect(page.locator('[data-column="tools"]')).toHaveJSProperty("clientWidth", 847);
        await nativeCapture(app, page, `columns-${theme}-body-847-called-tools`);
        await bodyWidth(app, page, 1116);
        await page.locator('[data-column-toggle="views"]').click();
        await expect(workspace).toHaveAttribute("data-file-views", "off");
        await expect(page.locator('[data-column-toggle="views"] [data-column-badge]')).toBeVisible();
        await nativeCapture(app, page, `columns-${theme}-tools-only`);
        await page.locator('[data-column-toggle="views"]').click();
        await expect(workspace).toHaveAttribute("data-file-views", "shown");
        for (let remaining = await page.locator('[data-view-tab-bar] [data-display]').count(); remaining > 0; remaining -= 1) {
          await page.getByRole("button", { name: /^Close view / }).first().click();
          await expect(page.locator('[data-view-tab-bar] [data-display]')).toHaveCount(remaining - 1);
        }
        await expect(workspace).toHaveAttribute("data-file-views", "off");
        await expect(page.locator('[data-column-toggle="views"] [data-column-badge]')).toHaveCount(0);
        await expect(page.locator('[data-column="views"]')).toHaveCount(0);
        await expect(page.locator('[data-column="tools"]')).toHaveJSProperty("clientWidth", 355);
        await nativeCapture(app, page, `columns-${theme}-zero-views-tools`);
        await page.locator(`[data-explorer-row$="/${longName}"]`).click();
        await expect(workspace).toHaveAttribute("data-file-views", "shown");
        // The saved geometry reference is a two-column body, with Tools
        // off: Agents512 + divider8 + File Views596. Restore it in both
        // themes rather than pairing a dark capture to a light board.
        await page.locator('[data-column-toggle="tools"]').click();
        await expect(workspace).toHaveAttribute("data-tools", "off");
        const restored = page.locator('[data-column-divider="views"]');
        await expect(restored).toHaveAttribute("aria-valuenow", "628");
        await restored.press("ArrowRight");
        await expect(restored).toHaveAttribute("aria-valuenow", "596");
        await page.reload();
        await expect(restored).toHaveAttribute("aria-valuenow", "596");
        await expect(page.locator('[data-column="agents"]')).toHaveJSProperty("clientWidth", 512);
        await nativeCapture(app, page, `columns-${theme}-restored-width`);
        await page.locator('[data-column-toggle="tools"]').click();
        await expect(workspace).toHaveAttribute("data-tools", "shown");
        await bodyWidth(app, page, 1600);
        await restored.press("ArrowLeft");
        await restored.press("ArrowLeft");
        await expect(restored).toHaveAttribute("aria-valuenow", "660");
        await bodyWidth(app, page, 1116);
      }
    }
  } finally {
    await app?.close().catch(() => undefined);
    run.cleanup();
    herdr.stop();
  }
});

// B20 and UI_BEHAVIOR: every arrow key moves one 32px step, even when
// more keys arrive before the daemon publishes the previous width.
test("column dividers retain every rapid arrow step before a snapshot arrives", async () => {
  const herdr = await startHerdr({ agents: false });
  const run = isolate(herdr, "column-key-burst");
  let app: ElectronApplication | null = null;
  try {
    ({ app } = await launchShell(run.env));
    const page = await app.firstWindow();
    await enterWorkspace(page, "fixture");
    await bodyWidth(app, page, 1600);
    await page.locator('[data-column-toggle="views"]').click();
    await expect(page.locator('[data-column="views"]')).toBeVisible();
    await showExplorer(page);
    const views = page.locator('[data-column-divider="views"]');
    const tools = page.locator('[data-column-divider="tools"]');
    await expect(views).toHaveAttribute("aria-valuenow", "640");
    await expect(tools).toHaveAttribute("aria-valuenow", "355");
    const burst = async (divider: typeof views, key: "ArrowLeft" | "ArrowRight") => {
      await divider.focus();
      // Deliver both keys in one browser turn, so the regression does not
      // depend on the machine winning a race against the next snapshot.
      await divider.evaluate((element, key) => {
        for (let count = 0; count < 2; count += 1) {
          element.dispatchEvent(new KeyboardEvent("keydown", { key, code: key, bubbles: true, cancelable: true }));
        }
      }, key);
    };
    await burst(views, "ArrowLeft");
    await expect(views).toHaveAttribute("aria-valuenow", "704");
    await expect(page.locator('[data-column="agents"]')).toHaveJSProperty("clientWidth", 525);
    await burst(tools, "ArrowLeft");
    await expect(tools).toHaveAttribute("aria-valuenow", "419");
    await expect(views).toHaveAttribute("aria-valuenow", "640");
    await expect(page.locator('[data-column="agents"]')).toHaveJSProperty("clientWidth", 525);
    await burst(tools, "ArrowRight");
    await expect(tools).toHaveAttribute("aria-valuenow", "355");
    await expect(views).toHaveAttribute("aria-valuenow", "704");
    await burst(views, "ArrowRight");
    await expect(views).toHaveAttribute("aria-valuenow", "640");
    await page.reload();
    await expect(views).toHaveAttribute("aria-valuenow", "640");
    await expect(tools).toHaveAttribute("aria-valuenow", "355");
    await nativeCapture(app, page, "columns-rapid-keys-restored");
  } finally {
    await app?.close().catch(() => undefined);
    run.cleanup();
    herdr.stop();
  }
});

test("SSH Workspace columns keep chords, fallback and saved widths separate from the same local path", async () => {
  test.skip(!process.env.HIDE_E2E_SSH_PORT, "requires the proved private SSH HOME in device-home.ts");
  test.setTimeout(300_000);
  const local = await startHerdr({ agents: false });
  let remote: HerdrFixture | null = null;
  let run: ReturnType<typeof isolate> | null = null;
  let app: ElectronApplication | null = null;
  let remoteHome: string | null = null;
  const errors: unknown[] = [];
  try {
    remote = await startHerdr({ agents: false });
    run = isolate(local, "remote-columns");
    run.env.HIDE_HOST_HELPER_ROOT = path.join(run.root, "device-helper");
    run.env.HIDE_HOST_CLI_DIR = path.join(run.root, "device-bin");
    writeSshConfig(run.env.HOME!, ["isolated-columns"]);
    remoteHome = deviceHome();
    proveDeviceHome(run.env, "isolated-columns", remoteHome);
    resetDeviceHome(remoteHome);
    // The host starts and owns the staged daemon through its existing CLI
    // boundary; no second resident-process launcher belongs to this spec.
    run.env.HIDE_CLI_PATH = path.join(stageBuild(run.root), "hide");
    const shared = path.join(fs.realpathSync(local.root), "shared-columns");
    const other = path.join(fs.realpathSync(remote.root), "remote-only");
    for (const folder of [shared, other]) {
      fs.mkdirSync(folder);
      fs.writeFileSync(path.join(folder, "한글-notes.md"), "# Remote column notes\n");
      const initialized = spawnSync("git", ["-C", folder, "init", "-q"], { env: local.env, encoding: "utf8", timeout: 10_000 });
      expect(initialized.status, initialized.stderr).toBe(0);
    }
    for (const [server, folder, label] of [[local, shared, "shared-columns"], [remote, shared, "shared-columns"], [remote, other, "remote-only"]] as const) {
      server.run(["workspace", "create", "--cwd", folder, "--label", label, "--env", `PATH=${server.fixturePath}`, "--no-focus"]);
    }
    ({ app } = await launchShell(run.env));
    const page = await app.firstWindow();
    const dir = process.env.HIDE_E2E_SCREENSHOT_DIR;
    if (dir) {
      const candidate = await app.evaluate(({ BrowserWindow }) => ({ pid: process.pid, window: BrowserWindow.getAllWindows()[0]!.getMediaSourceId(), executable: process.execPath }));
      fs.writeFileSync(path.join(dir, "remote-columns-identity.json"), JSON.stringify({ ...candidate, daemonPid: run.daemonPid(), localSocket: local.socket, remoteSocket: remote.socket, home: run.env.HOME, deviceHome: remoteHome, state: run.env.HIDE_STATE_DIR, profile: run.env.HIDE_DESKTOP_USER_DATA_DIR, transport: "real loopback SSH to a separate private Herdr server" }, null, 2));
    }
    await enterWorkspace(page, "fixture");
    const state = JSON.parse(fs.readFileSync(path.join(run.env.HIDE_STATE_DIR!, "hided.json"), "utf8")) as { token: string };
    await sendEvent(page, { origin: new URL(page.url()).origin, token: state.token }, "register_device", {
      id: "columns-ssh", label: "SSH columns", ssh_alias: "isolated-columns", herdr_socket_path: remote.socket, host_consent: true,
    });
    const rail = (device: string) => page.locator(`[data-rail-tile="${device}"]`);
    await expect(rail("columns-ssh")).toHaveAttribute("data-rail-connected", "true", { timeout: 120_000 });
    await rail("columns-ssh").click();
    await page.locator('[data-sidebar-mode="projects"]').click();
    const row = (label: string) => page.locator("[data-project]", { hasText: label }).locator("[data-checkout]").first();
    await row("shared-columns").click();
    await expect(row("shared-columns")).toHaveAttribute("aria-current", "true");
    const workspace = page.locator("[data-workspace-screen]");
    await bodyWidth(app, page, 1600);
    await expect(workspace).toHaveAttribute("data-file-views", "off");
    await expect(workspace).toHaveAttribute("data-tools", "off");
    await page.keyboard.press(chord("toggle_explorer", "electron"));
    await expect(workspace).toHaveAttribute("data-tools", "shown");
    await expect(page.locator('[data-explorer-row$="/한글-notes.md"]')).toBeVisible({ timeout: 60_000 });
    await page.locator('[data-explorer-row$="/한글-notes.md"]').click();
    await expect(page.locator("[data-editor-body] .cm-content")).toContainText("Remote column notes");
    await page.keyboard.press(chord("toggle_right_panel", "electron"));
    await expect(workspace).toHaveAttribute("data-file-views", "off");
    await page.keyboard.press(chord("toggle_right_panel", "electron"));
    await expect(workspace).toHaveAttribute("data-file-views", "shown");
    // Save a width by the real release path, then give Tools a different
    // saved width through the separator's accessible keyboard interaction.
    const divider = page.locator('[data-column-divider="views"]');
    const box = (await divider.boundingBox())!;
    await page.mouse.move(box.x + box.width / 2, box.y + 100);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2 + 96, box.y + 100);
    await expect(page.locator("[data-column-guide=true]")).toBeVisible();
    await page.mouse.up();
    await expect(divider).toHaveAttribute("aria-valuenow", "544");
    await page.locator('[data-column-divider="tools"]').press("ArrowLeft");
    await expect(page.locator('[data-column-divider="tools"]')).toHaveAttribute("aria-valuenow", "387");
    const saved = () => JSON.parse(fs.readFileSync(path.join(run!.env.HIDE_STATE_DIR!, "workspace-views.json"), "utf8")) as {
      workspaces: { device_id: string; path: string; views: boolean; tools: boolean; views_width?: number; tools_width?: number }[];
    };
    await expect.poll(() => saved().workspaces.find((entry) => entry.device_id === "columns-ssh" && entry.path === shared)).toMatchObject({ views: true, tools: true, views_width: 512, tools_width: 387 });
    await nativeCapture(app, page, "remote-columns-wide-saved");
    await bodyWidth(app, page, 1116);
    await expect.poll(() => Promise.all(["agents", "views", "tools"].map((column) => page.locator(`[data-column="${column}"]`).evaluate((element) => element.clientWidth)))).toEqual([480, 360, 260]);
    await nativeCapture(app, page, "remote-columns-1116");
    await bodyWidth(app, page, 1100);
    await expect(workspace).toHaveAttribute("data-file-views", "shown");
    await expect(workspace).toHaveAttribute("data-tools", "hidden");
    await page.keyboard.press(chord("toggle_explorer", "electron"));
    await expect(workspace).toHaveAttribute("data-tools", "shown");
    await expect(workspace).toHaveAttribute("data-file-views", "hidden");
    await expect.poll(() => page.locator('[data-column="tools"]').evaluate((element) => element.clientWidth)).toBe(387);
    await nativeCapture(app, page, "remote-columns-mid-called-tools");
    await bodyWidth(app, page, 848);
    await expect(workspace).toHaveAttribute("data-workspace-body", "mid");
    await expect.poll(() => page.locator('[data-column="agents"]').evaluate((element) => element.clientWidth)).toBe(480);
    await bodyWidth(app, page, 847);
    await expect(workspace).toHaveAttribute("data-workspace-body", "narrow");
    await expect(page.locator('[data-column="agents"]')).toBeVisible();
    await page.keyboard.press(chord("toggle_right_panel", "electron"));
    await expect(workspace).toHaveAttribute("data-file-views", "shown");
    await expect(page.locator('[data-column="agents"]')).not.toBeVisible();
    await nativeCapture(app, page, "remote-columns-narrow-called-views");
    await bodyWidth(app, page, 1600);
    await rail("local").click();
    await row("shared-columns").click();
    await expect(row("shared-columns")).toHaveAttribute("aria-current", "true");
    await expect(workspace).toHaveAttribute("data-file-views", "off");
    await expect(workspace).toHaveAttribute("data-tools", "off");
    await showExplorer(page);
    await page.locator('[data-explorer-row$="/한글-notes.md"]').click();
    await expect(page.locator("[data-editor-body] .cm-content")).toContainText("Remote column notes");
    await expect(page.locator('[data-column-divider="views"]')).toHaveAttribute("aria-valuenow", "640");
    await expect(page.locator('[data-column-divider="tools"]')).toHaveAttribute("aria-valuenow", "355");
    await nativeCapture(app, page, "remote-columns-same-local-path");
    await rail("columns-ssh").click();
    await row("remote-only").click();
    await expect(workspace).toHaveAttribute("data-file-views", "off");
    await expect(workspace).toHaveAttribute("data-tools", "off");
    await row("shared-columns").click();
    await expect(page.locator('[data-column-divider="views"]')).toHaveAttribute("aria-valuenow", "512");
    await expect(page.locator('[data-column-divider="tools"]')).toHaveAttribute("aria-valuenow", "387");
    await page.reload();
    await expect(page.locator('[data-column-divider="views"]')).toHaveAttribute("aria-valuenow", "512");
    await expect(page.locator('[data-column-divider="tools"]')).toHaveAttribute("aria-valuenow", "387");
    await nativeCapture(app, page, "remote-columns-restored");
    if (dir) fs.writeFileSync(path.join(dir, "remote-columns-saved.json"), JSON.stringify(saved(), null, 2));
  } catch (error) {
    errors.push(error);
  } finally {
    try { await app?.close(); } catch (error) { errors.push(error); }
    for (const cleanup of [
      () => run?.cleanup(),
      () => remote?.stop(),
      () => local.stop(),
    ]) {
      try { cleanup(); } catch (error) { errors.push(error); }
    }
  }
  if (errors.length) throw new AggregateError(errors, "private SSH column fixture failed; inspect all test and cleanup errors before removing its declared homes");
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
    for (const theme of ["dark", "light"] as const) {
      await row("fixture").click();
      await expect(row("fixture")).toHaveAttribute("aria-current", "true");
      await openNotes();
      await page.locator("[data-open-settings]").click();
      await page.locator('[data-settings-tab="general"]').click();
      await page.locator(`[data-theme-option="${theme}"]`).click();
      await expect(page.locator("html")).toHaveClass(new RegExp(`\\b${theme}\\b`));
      await page.keyboard.press("Escape");
      await expect(page.locator('[data-settings="true"]')).toHaveCount(0);
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
      await expect(page.locator('[data-column="views"]')).toHaveJSProperty("clientWidth", 640);
      await expect(page.locator('[data-column="tools"]')).toHaveJSProperty("clientWidth", 355);
      await nativeCapture(app, page, `columns-${theme}-drag-switch-cancelled`);
    }
  } finally {
    await app?.close().catch(() => undefined);
    run.cleanup();
    herdr.stop();
  }
});
