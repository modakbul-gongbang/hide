// The S7 View areas on an isolated pinned Herdr and hided (PRD
// workspace-views-layout B1-B22): a click previews a file in the last used
// area and a double click, Keep open or the first edit pins it (B1-B3); Open
// to the side shows one document twice with one buffer (B4, B5, B20); tab
// drags reorder, move and split with a preview and land once (B6-B8);
// splits, dividers, menus and the palette work from the keyboard (B9, B10,
// B18, B20); the tab menu and the caps say what cannot be done (B11, B19); a
// narrow window changes only what is drawn (B12, B13); a restart brings the
// layout back and a broken or older file is handled (B14-B17); and S6's
// layout switch and tools keep working beside several areas (B22).

import { expect, test, type Locator, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, enterWorkspace, screenshot } from "./wire";

test.describe.configure({ timeout: 180_000 });
// A click or a key that cannot happen fails the flow in seconds, not at the test's end.
test.use({ actionTimeout: 15_000 });

type SentEvent = { kind: string; payload: Record<string, unknown> };

type Stack = {
  herdr: HerdrFixture;
  daemon: Daemon;
  /** The checkout root as the core spells it (a resolved path). */
  root: string;
  sent: Map<string, number>;
  last: Map<string, Record<string, unknown>>;
  /** Every event the page sent, in order. */
  events: SentEvent[];
  /** The `workspace_views.*` diagnostic kinds the page received in any snapshot frame. */
  diagnostics: Set<string>;
};

/** The daemon's state directory, where `workspace-views.json` lives. */
function stateDir(daemon: Daemon): string {
  return path.join(path.dirname(daemon.home), "hide");
}

function viewsFile(daemon: Daemon): string {
  return path.join(stateDir(daemon), "workspace-views.json");
}

/** Records every event the page sends and every Views-file diagnostic it receives. */
function recordWire(page: Page, events: SentEvent[], diagnostics: Set<string>): void {
  page.on("websocket", (ws) => {
    ws.on("framesent", (frame) => {
      try {
        const event = JSON.parse(String(frame.payload)) as { kind?: string; payload?: Record<string, unknown> };
        if (event.kind) events.push({ kind: event.kind, payload: event.payload ?? {} });
      } catch {
        /* the handshake is not an event */
      }
    });
    ws.on("framereceived", (frame) => {
      const text = String(frame.payload);
      if (!text.includes('"diagnostics"')) return;
      for (const match of text.matchAll(/"kind":"(workspace_views\.[a-z_]+)"/g)) {
        if (match[1]) diagnostics.add(match[1]);
      }
    });
  });
}

async function open(page: Page, daemon: Daemon): Promise<void> {
  await page.goto(`${daemon.origin}/#token=${daemon.token}`);
}

/**
 * The isolated stack with `files` written into the fixture checkout, and the
 * page on that Workspace (a first run goes through Main and the Overview).
 */
async function startStack(
  page: Page,
  label: string,
  files: Record<string, string>,
  options: { beforeOpen?: () => Promise<void>; prepare?: (checkout: string) => void } = {},
): Promise<Stack> {
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    for (const [name, contents] of Object.entries(files)) {
      fs.mkdirSync(path.dirname(path.join(herdr.root, "fixture", name)), { recursive: true });
      fs.writeFileSync(path.join(herdr.root, "fixture", name), contents);
    }
    options.prepare?.(path.join(herdr.root, "fixture"));
    daemon = await startHided(herdr, label);
    const last = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, last);
    const events: SentEvent[] = [];
    const diagnostics = new Set<string>();
    recordWire(page, events, diagnostics);
    await options.beforeOpen?.();
    await open(page, daemon);
    await enterWorkspace(page, "fixture");
    return { herdr, daemon, root: path.join(fs.realpathSync(herdr.root), "fixture"), sent, last, events, diagnostics };
  } catch (error) {
    daemon?.stop();
    herdr.stop();
    throw error;
  }
}

function stopStack(stack: Stack): void {
  stack.daemon.stop();
  stack.herdr.stop();
}

/**
 * What the View areas show, in tree order: each area's tabs in order, `>`
 * before the area's shown tab, `*` after a preview, and `@` before the area
 * the operator works in. `(b.txt >c.txt*) | @(>d.txt)` is two areas.
 */
async function shape(page: Page): Promise<string> {
  return page.evaluate(() =>
    [...document.querySelectorAll<HTMLElement>("[data-view-area-id]")]
      .map((area) => {
        const tabs = [...area.querySelectorAll<HTMLElement>('[data-view-tab-bar] [role="tab"][data-display]')].map((tab) => {
          const identity = tab.getAttribute("aria-label") ?? "";
          const name = identity.replace(/^[^:]+: /, "").replace(/ · .*$/, "").split("/").pop() ?? "";
          return `${tab.getAttribute("aria-selected") === "true" ? ">" : ""}${name}${tab.dataset.preview === "true" ? "*" : ""}`;
        });
        return `${area.dataset.activeArea === "true" ? "@" : ""}(${tabs.join(" ")})`;
      })
      .join(" | "),
  );
}

function area(page: Page, index: number): Locator {
  return page.locator("[data-view-area-id]").nth(index);
}

/** A View tab by its file name, in one area or anywhere. */
function tab(scope: Page | Locator, name: string): Locator {
  return scope.locator(`[data-view-tab-bar] [role="tab"][data-display][aria-label*="/${name}"]`);
}

/** The shown document's editable text in one area. */
function editor(page: Page, index: number): Locator {
  return area(page, index).locator("[data-editor-body] .cm-content");
}

function explorerRow(page: Page, stack: Stack, name: string): Locator {
  return page.locator(`[data-explorer-row="${path.join(stack.root, name)}"]`);
}

/** Opens a View tab's own menu (right-click) and picks one item. */
async function tabMenu(scope: Page | Locator, page: Page, name: string, item: string): Promise<void> {
  await tab(scope, name).click({ button: "right" });
  await page.locator(`[role="menu"] [data-menu-item="${item}"]`).click();
}

/** Every `view_layout` event sent so far. */
function viewEvents(stack: Stack): SentEvent[] {
  return stack.events.filter((event) => event.kind === "view_layout");
}

/** Every View action the page sent named the Workspace in front: this checkout on this machine (contract 4.1). */
function expectFrontWorkspaceOnEveryViewEvent(stack: Stack): void {
  for (const event of viewEvents(stack)) expect(event.payload.workspace).toEqual({ device_id: "local", path: stack.root });
}

test("a click previews in the last used area, a pin keeps a view, and a shown file is focused rather than opened again", async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  const names = ["a", "b", "c", "d", "e", "f"];
  const stack = await startStack(page, "s7-place", Object.fromEntries(names.map((name) => [`${name}.txt`, `${name} line\n`])));
  try {
    const row = (name: string) => explorerRow(page, stack, name);
    const workspace = page.locator("[data-workspace-screen]");

    // A single click previews in the one area, italic, and the next single
    // click replaces that preview in place (B1).
    await row("a.txt").click();
    await expect(workspace).toHaveAttribute("data-layout", "together");
    await expect.poll(() => shape(page)).toBe("@(>a.txt*)");
    await expect(tab(page, "a.txt").getByText("a.txt")).toHaveCSS("font-style", "italic");
    await expect(tab(page, "a.txt")).toHaveAttribute("aria-label", /· Preview$/);
    await row("b.txt").click();
    await expect.poll(() => shape(page)).toBe("@(>b.txt*)");

    // A double click on the tab pins it (B2).
    await tab(page, "b.txt").dblclick();
    await expect.poll(() => shape(page)).toBe("@(>b.txt)");
    await expect(tab(page, "b.txt").getByText("b.txt")).toHaveCSS("font-style", "normal");
    expect(stack.sent.get("view_layout.keep_open")).toBe(1);

    // Keep open from the tab's menu pins the next preview (B2).
    await row("c.txt").click();
    await expect.poll(() => shape(page)).toBe("@(b.txt >c.txt*)");
    await tabMenu(page, page, "c.txt", "keep_open");
    await expect.poll(() => shape(page)).toBe("@(b.txt >c.txt)");

    // A double click on the Explorer row opens it pinned (B2).
    await row("d.txt").dblclick();
    await expect.poll(() => shape(page)).toBe("@(b.txt c.txt >d.txt)");

    // A second area: the split view goes right and that area is now the one in use.
    await tabMenu(page, page, "c.txt", "split_right");
    await expect.poll(() => shape(page)).toBe("(b.txt >d.txt) | @(>c.txt)");

    // The next click previews in the area used last, and the other area's
    // pinned tabs stay (B1).
    await row("a.txt").click();
    await expect.poll(() => shape(page)).toBe("(b.txt >d.txt) | @(c.txt >a.txt*)");
    await tab(area(page, 0), "b.txt").click();
    await expect.poll(() => shape(page)).toBe("@(>b.txt d.txt) | (c.txt >a.txt*)");
    await row("e.txt").click();
    await expect.poll(() => shape(page)).toBe("@(b.txt d.txt >e.txt*) | (c.txt >a.txt*)");
    await screenshot(page, "s7-place-two-previews");

    // The first edit pins the preview: a dirty document is never a preview,
    // so the next click adds a preview beside it instead of replacing it (B2).
    const opensBefore = stack.sent.get("file_open") ?? 0;
    await editor(page, 0).click();
    await page.keyboard.press("Meta+ArrowUp");
    await page.keyboard.press("End");
    await page.keyboard.type(" edited");
    await expect.poll(() => shape(page)).toBe("@(b.txt d.txt >e.txt) | (c.txt >a.txt*)");
    await row("f.txt").click();
    await expect.poll(() => shape(page)).toBe("@(b.txt d.txt e.txt >f.txt*) | (c.txt >a.txt*)");
    expect(stack.sent.get("file_open")).toBe(opensBefore + 1);
    await expect.poll(() => fs.readFileSync(path.join(stack.root, "e.txt"), "utf8")).toBe("e line edited\n");

    // Opening a file another area shows focuses that view: no tab is added (B3).
    await row("c.txt").click();
    await expect.poll(() => shape(page)).toBe("(b.txt d.txt e.txt >f.txt*) | @(>c.txt a.txt*)");

    // With two views of one file, reopening it picks the one used last (B3).
    await tab(area(page, 0), "d.txt").click();
    await row("d.txt").click({ button: "right" });
    await page.locator('[data-explorer-menu] [data-menu-item="open-beside"]').click();
    await expect.poll(() => shape(page)).toBe("(b.txt >d.txt e.txt f.txt*) | @(c.txt a.txt* >d.txt)");
    await tab(area(page, 0), "d.txt").click();
    await tab(area(page, 1), "c.txt").click();
    await expect.poll(() => shape(page)).toBe("(b.txt >d.txt e.txt f.txt*) | @(>c.txt a.txt* d.txt)");
    await row("d.txt").click();
    await expect.poll(() => shape(page)).toBe("@(b.txt >d.txt e.txt f.txt*) | (>c.txt a.txt* d.txt)");
    await tab(area(page, 1), "d.txt").click();
    await tab(area(page, 0), "b.txt").click();
    await expect.poll(() => shape(page)).toBe("@(>b.txt d.txt e.txt f.txt*) | (c.txt a.txt* >d.txt)");
    await row("d.txt").click();
    await expect.poll(() => shape(page)).toBe("(>b.txt d.txt e.txt f.txt*) | @(c.txt a.txt* >d.txt)");
    await screenshot(page, "s7-place-reopen-last-focused");
    expectFrontWorkspaceOnEveryViewEvent(stack);
  } finally {
    stopStack(stack);
  }
});

/** Three hundred numbered lines, so a view can scroll and each line is findable. */
const LONG = Array.from({ length: 300 }, (_, index) => `line ${String(index + 1).padStart(3, "0")}`).join("\n") + "\n";

/** Composes Korean text through the browser's IME path, one syllable at a time, then commits it. */
async function composeKorean(page: Page, steps: string[][]): Promise<void> {
  const cdp = await page.context().newCDPSession(page);
  try {
    for (const syllable of steps) {
      for (const marked of syllable) await cdp.send("Input.imeSetComposition", { text: marked, selectionStart: marked.length, selectionEnd: marked.length });
      await cdp.send("Input.insertText", { text: syllable.at(-1) ?? "" });
    }
  } finally {
    await cdp.detach();
  }
}

test("Open to the side shows one document twice: edits and Korean input reach both, each keeps its place, and closing keeps unsaved text", async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  const stack = await startStack(page, "s7-beside", { "shared.txt": LONG, "other.txt": "other\n", "keep.txt": "keep on disk\n" });
  const shared = path.join(stack.root, "shared.txt");
  const keep = path.join(stack.root, "keep.txt");
  try {
    const row = (name: string) => explorerRow(page, stack, name);
    await row("shared.txt").click();
    await expect.poll(() => shape(page)).toBe("@(>shared.txt*)");

    // Open to the side makes a second, pinned view in a new area on the right (B4).
    await row("shared.txt").click({ button: "right" });
    await page.locator('[data-explorer-menu] [data-menu-item="open-beside"]').click();
    await expect.poll(() => shape(page)).toBe("(>shared.txt*) | @(>shared.txt)");
    expect(stack.last.get("file_open")).toMatchObject({ path: shared, beside: true, preview: false });
    const [left, right] = [editor(page, 0), editor(page, 1)];
    await expect(left).toContainText("line 001");
    await expect(right).toContainText("line 001");

    // An edit in one view shows in the other, and the first edit pins every
    // view of the document (B2, B4).
    await right.click();
    await page.keyboard.press("Meta+ArrowUp");
    await page.keyboard.press("End");
    await page.keyboard.type("-B");
    await expect(left).toContainText("line 001-B");
    await expect.poll(() => shape(page)).toBe("(>shared.txt) | @(>shared.txt)");

    // Korean text inserted in one view and composed in the other reads the
    // same in both, once, and reaches the file once (B20, D-13).
    await left.click();
    await page.keyboard.press("Meta+ArrowUp");
    await page.keyboard.press("End");
    await page.keyboard.insertText(" 한글");
    await expect(right).toContainText("line 001-B 한글");
    // The right view's cursor stayed where it was, before the text the left
    // view inserted; End takes it past that text.
    await right.focus();
    await page.keyboard.press("End");
    await composeKorean(page, [
      ["ㅇ", "아", "안"],
      ["ㄴ", "녀", "녕"],
    ]);
    await expect(right).toContainText("line 001-B 한글안녕");
    await expect(left).toContainText("line 001-B 한글안녕");
    await expect.poll(() => fs.readFileSync(shared, "utf8").split("\n")[0], { timeout: 10_000 }).toBe("line 001-B 한글안녕");
    await screenshot(page, "s7-beside-korean");

    // Each view keeps its own scroll and cursor: the left one scrolls to the
    // end and types there, the right one types at its own cursor (B4).
    const scrollers = [area(page, 0).locator(".cm-scroller"), area(page, 1).locator(".cm-scroller")];
    await scrollers[0]!.hover();
    await page.mouse.wheel(0, 20_000);
    await expect.poll(() => scrollers[0]!.evaluate((node) => node.scrollTop)).toBeGreaterThan(1000);
    await left.getByText("line 300").click();
    await page.keyboard.press("End");
    await page.keyboard.type("-A");
    await right.focus();
    await page.keyboard.type("-C");
    await expect(right).toContainText("line 001-B 한글안녕-C");
    await expect(left).toContainText("line 300-A");
    expect(await scrollers[1]!.evaluate((node) => node.scrollTop)).toBe(0);
    expect(await scrollers[0]!.evaluate((node) => node.scrollTop)).toBeGreaterThan(1000);
    await expect.poll(() => fs.readFileSync(shared, "utf8"), { timeout: 10_000 }).toBe(
      LONG.replace("line 001\n", "line 001-B 한글안녕-C\n").replace("line 300\n", "line 300-A\n"),
    );

    // A save that cannot land keeps the document unsaved in both views.
    // Closing one of them asks nothing and leaves the text and its unsaved
    // mark in the other (B5, D-03).
    await scrollers[0]!.hover();
    await page.mouse.wheel(0, -20_000);
    await expect.poll(() => scrollers[0]!.evaluate((node) => node.scrollTop)).toBe(0);
    fs.chmodSync(shared, 0o444);
    await right.focus();
    await page.keyboard.type("-D");
    await expect(left).toContainText("-C-D");
    await expect(area(page, 0).locator("[data-editor-save-state]")).toBeVisible({ timeout: 10_000 });
    await expect(tab(area(page, 0), "shared.txt")).toContainText("●");
    const closesBefore = stack.sent.get("view_layout.close") ?? 0;
    await tab(area(page, 1), "shared.txt").hover();
    await tab(area(page, 1), "shared.txt").getByRole("button", { name: /Close view/ }).click();
    await expect.poll(() => shape(page)).toBe("@(>shared.txt)");
    expect(stack.sent.get("view_layout.close")).toBe(closesBefore + 1);
    await expect(page.locator("[data-notice]")).toHaveCount(0);
    await expect(editor(page, 0)).toContainText("-C-D");
    await expect(tab(page, "shared.txt")).toContainText("●");
    await expect(area(page, 0).locator("[data-editor-dirty]")).toBeVisible();

    // Closing the last view carries the unsaved text as its save: while the
    // save cannot land the view stays, with its text, and is not called
    // saved; once it can, the save lands and the view closes (B5).
    await tab(page, "shared.txt").hover();
    await tab(page, "shared.txt").getByRole("button", { name: /Close view/ }).click();
    await expect.poll(() => (stack.last.get("view_layout.close")?.pending_save as { contents_utf8?: string } | undefined)?.contents_utf8?.split("\n")[0]).toBe(
      "line 001-B 한글안녕-C-D",
    );
    await page.waitForTimeout(1500);
    await expect.poll(() => shape(page)).toBe("@(>shared.txt)");
    await expect(editor(page, 0)).toContainText("-C-D");
    await expect(area(page, 0).locator("[data-editor-save-state]")).toBeVisible();
    expect(fs.readFileSync(shared, "utf8").split("\n")[0]).toBe("line 001-B 한글안녕-C");
    fs.chmodSync(shared, 0o644);
    await tab(page, "shared.txt").hover();
    await tab(page, "shared.txt").getByRole("button", { name: /Close view/ }).click();
    await expect(page.locator('[data-area-empty="no-view"]')).toBeVisible({ timeout: 10_000 });
    await expect.poll(() => fs.readFileSync(shared, "utf8").split("\n")[0]).toBe("line 001-B 한글안녕-C-D");

    // Unsaved text this page never loaded (a background view after a reload)
    // is closed only by the operator's explicit Don't save, which sends
    // `discard` and leaves the file as it was on disk (B5, contract 4.1).
    await row("keep.txt").dblclick();
    await expect.poll(() => shape(page)).toBe("@(>keep.txt)");
    fs.chmodSync(keep, 0o444);
    await editor(page, 0).click();
    await page.keyboard.press("Meta+ArrowUp");
    await page.keyboard.type("never saved ");
    await expect(area(page, 0).locator("[data-editor-save-state]")).toBeVisible({ timeout: 10_000 });
    await row("other.txt").dblclick();
    await expect.poll(() => shape(page)).toBe("@(keep.txt >other.txt)");
    // A fresh page, not a fragment navigation of this one.
    await page.goto("about:blank");
    await open(page, stack.daemon);
    await expect.poll(() => shape(page), { timeout: 20_000 }).toBe("@(keep.txt >other.txt)");
    await tab(page, "keep.txt").hover();
    await tab(page, "keep.txt").getByRole("button", { name: /Close view/ }).click();
    const dontSave = page.locator("[data-notice-dont-save]");
    await expect(dontSave).toBeVisible();
    await expect(page.locator("[data-notice]")).toContainText("keep.txt has unsaved changes");
    await expect.poll(() => shape(page)).toBe("@(keep.txt >other.txt)");
    await screenshot(page, "s7-beside-dont-save");
    await dontSave.click();
    await expect.poll(() => shape(page)).toBe("@(>other.txt)");
    expect(stack.last.get("view_layout.close")).toMatchObject({ discard: true });
    expect(fs.readFileSync(keep, "utf8")).toBe("keep on disk\n");

    expectFrontWorkspaceOnEveryViewEvent(stack);
  } finally {
    for (const file of [shared, keep]) if (fs.existsSync(file)) fs.chmodSync(file, 0o644);
    stopStack(stack);
  }
});

/** Whether the page hided serves (`web/dist`) was built with `text` in it. */
function builtPageHas(text: string): boolean {
  const assets = path.resolve("dist", "assets");
  return fs.readdirSync(assets).some((name) => name.endsWith(".js") && fs.readFileSync(path.join(assets, name), "utf8").includes(text));
}

const BESIDE_TOO_NARROW = "This view area is too narrow to open a second view beside it.";

test("Open to the side from the only area is refused with its reason until that area has room to split", async ({ page }) => {
  test.skip(!builtPageHas(BESIDE_TOO_NARROW), "needs a web/dist built with the Open to the side room rule");
  await page.setViewportSize({ width: 1280, height: 720 });
  const stack = await startStack(page, "s7-beside-room", { "a.txt": "a\n" });
  try {
    const row = explorerRow(page, stack, "a.txt");
    await row.click();
    await expect.poll(() => shape(page)).toBe("@(>a.txt*)");
    await expect.poll(async () => (await boxOf(area(page, 0))).width).toBeLessThan(450);

    // The one area cannot be halved, so Open to the side stays listed,
    // disabled with the reason, in the Explorer's menu and the palette (B4, B9, D-06).
    await row.click({ button: "right" });
    const item = page.locator('[data-explorer-menu] [data-menu-item="open-beside"]');
    await expect(item).toBeDisabled();
    await expect(item).toContainText(BESIDE_TOO_NARROW);
    await page.keyboard.press("Escape");
    await page.mouse.click(5, 5);
    await page.keyboard.press("Meta+KeyK");
    await page.keyboard.type("Open file to the side");
    const command = page.locator('[data-palette-row="command:open_beside"]');
    await expect(command).toHaveAttribute("aria-disabled", "true");
    await expect(command).toContainText(BESIDE_TOO_NARROW);
    await page.keyboard.press("Escape");
    expect(stack.events.filter((event) => event.kind === "file_open" && event.payload.beside === true)).toHaveLength(0);

    // With room (Views only), the same item opens the second view to the right.
    await page.locator('[data-layout-choice="views"]').click();
    await expect.poll(async () => (await boxOf(area(page, 0))).width).toBeGreaterThan(450);
    await row.click({ button: "right" });
    await expect(item).toBeEnabled();
    await item.click();
    await expect.poll(() => shape(page)).toBe("(>a.txt*) | @(>a.txt)");
  } finally {
    stopStack(stack);
  }
});

type Box = { x: number; y: number; width: number; height: number };

async function boxOf(locator: Locator): Promise<Box> {
  const box = await locator.boundingBox();
  if (!box) throw new Error("not on screen");
  return box;
}

/** Every View area's rectangle, to tell whether anything resized. */
async function areaBoxes(page: Page): Promise<Box[]> {
  return page.locator("[data-view-area-id]").evaluateAll((areas) =>
    areas.map((node) => {
      const rect = node.getBoundingClientRect();
      return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
    }),
  );
}

/**
 * Presses a View tab and drags it with real pointer events past the
 * activation distance to `to`; `during` runs with the pointer still down,
 * and `release: false` leaves the release (or an Escape) to the caller.
 */
async function dragTab(page: Page, from: Locator, to: { x: number; y: number }, during?: () => Promise<void>, release = true): Promise<void> {
  // A tab past the end of a narrow strip is scrolled to first, as the operator would.
  await from.scrollIntoViewIfNeeded();
  const start = await boxOf(from);
  const x = start.x + start.width / 2;
  const y = start.y + start.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x + 12, y + 2, { steps: 3 });
  await page.mouse.move(to.x, to.y, { steps: 10 });
  await during?.();
  if (release) await page.mouse.up();
}

test("dragging a tab reorders, moves or splits once on a valid drop and leaves everything as it was otherwise", async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  const stack = await startStack(page, "s7-drag", { "a.txt": "a\n", "b.txt": "b\n", "c.txt": "c\n" });
  try {
    for (const name of ["a.txt", "b.txt", "c.txt"]) await explorerRow(page, stack, name).dblclick();
    await expect.poll(() => shape(page)).toBe("@(a.txt b.txt >c.txt)");
    const moves = () => stack.sent.get("view_layout.move") ?? 0;
    const splits = () => stack.sent.get("view_layout.split") ?? 0;
    const resizes = () => stack.sent.get("terminal_resize") ?? 0;
    const drop = page.locator("[data-view-drop]");
    const floating = page.locator("[data-view-drag-tab]");

    // Within its own tab bar: an insertion line, the floating tab, nothing
    // resized while it lasts, then the order changes once with no copy (B6).
    const before = await areaBoxes(page);
    const terminalResizes = resizes();
    const [bTab, cTab] = [await boxOf(tab(page, "b.txt")), await boxOf(tab(page, "c.txt"))];
    await dragTab(page, tab(page, "a.txt"), { x: (bTab.x + bTab.width / 2 + cTab.x + cTab.width / 2) / 2, y: bTab.y + bTab.height / 2 }, async () => {
      await expect(page.locator('[data-view-drop="bar"]')).toBeVisible();
      await expect(floating).toBeVisible();
      expect(await areaBoxes(page)).toEqual(before);
      expect(moves()).toBe(0);
      await screenshot(page, "s7-drag-insertion-line");
    });
    await expect.poll(() => shape(page)).toBe("@(b.txt >a.txt c.txt)");
    expect(moves()).toBe(1);
    expect(stack.last.get("view_layout.move")).toMatchObject({ action: "move", index: 1 });
    await expect(drop).toHaveCount(0);
    await expect(floating).toHaveCount(0);

    // Onto the right edge of the content: the half the new area would take is
    // washed with one short label before the release; the drop splits once (B7).
    const body = await boxOf(area(page, 0).locator("[data-view-body]"));
    await dragTab(page, tab(page, "c.txt"), { x: body.x + body.width * 0.92, y: body.y + body.height / 2 }, async () => {
      const overlay = page.locator('[data-view-drop="right"]');
      await expect(overlay).toBeVisible();
      await expect(overlay).toHaveText("Split right");
      await expect(drop).toHaveCount(1);
      expect(await areaBoxes(page)).toEqual(before);
      expect(splits()).toBe(0);
      await screenshot(page, "s7-drag-split-overlay");
    });
    await expect.poll(() => shape(page)).toBe("(b.txt >a.txt) | @(>c.txt)");
    expect(splits()).toBe(1);
    expect(stack.last.get("view_layout.split")).toMatchObject({ action: "split", edge: "right" });

    // Into the other area's tab bar: a move into that area, no split, no copy (B6).
    const cRight = await boxOf(tab(area(page, 1), "c.txt"));
    await dragTab(page, tab(area(page, 0), "b.txt"), { x: cRight.x + cRight.width + 20, y: cRight.y + cRight.height / 2 }, async () => {
      await expect(page.locator('[data-view-drop="bar"]')).toBeVisible();
    });
    await expect.poll(() => shape(page)).toBe("(>a.txt) | @(c.txt >b.txt)");
    expect(moves()).toBe(2);
    expect(splits()).toBe(1);

    // Escape ends the drag with nothing sent, and the release after it changes nothing (B8).
    const settled = await areaBoxes(page);
    const aBox = await boxOf(tab(area(page, 0), "a.txt"));
    const bRight = await boxOf(tab(area(page, 1), "b.txt"));
    await dragTab(
      page,
      tab(area(page, 0), "a.txt"),
      { x: bRight.x + bRight.width + 20, y: bRight.y + bRight.height / 2 },
      async () => {
        await expect(page.locator('[data-view-drop="bar"]')).toBeVisible();
        await page.keyboard.press("Escape");
        await expect(drop).toHaveCount(0);
        await expect(floating).toHaveCount(0);
      },
      false,
    );
    await page.mouse.up();
    // A release outside the window lands nothing (B8).
    await dragTab(page, tab(area(page, 1), "c.txt"), { x: 1960, y: 500 }, async () => {
      await expect(floating).toBeVisible();
    });
    await expect(drop).toHaveCount(0);
    await expect(floating).toHaveCount(0);
    // A drop on something that is not a View area (the Explorer) lands nothing (B8).
    const explorer = await boxOf(page.locator('[data-tool="explorer"]'));
    await dragTab(page, tab(area(page, 1), "c.txt"), { x: explorer.x + explorer.width / 2, y: explorer.y + explorer.height / 2 }, async () => {
      await expect(drop).toHaveCount(0);
      await expect(floating).toBeVisible();
    });
    // The only view of an area cannot split its own area: no overlay, the
    // forbidden cursor and the reason on the floating tab (B8, B9).
    await dragTab(page, tab(area(page, 0), "a.txt"), { x: aBox.x + 300, y: body.y + body.height / 2 }, async () => {
      await expect(drop).toHaveCount(0);
      await expect(page.locator("html")).toHaveAttribute("data-view-drag", "forbidden");
      await expect(floating).toContainText("This is the only view in its area.");
      await screenshot(page, "s7-drag-forbidden");
    });
    await expect(page.locator("html")).not.toHaveAttribute("data-view-drag", /.*/);
    await expect.poll(() => shape(page)).toBe("(>a.txt) | @(c.txt >b.txt)");
    expect(await areaBoxes(page)).toEqual(settled);
    expect(moves()).toBe(2);
    expect(splits()).toBe(1);
    // No drag resized a terminal: the Agent area kept its size throughout (B7, B19).
    expect(resizes()).toBe(terminalResizes);
    expectFrontWorkspaceOnEveryViewEvent(stack);
  } finally {
    stopStack(stack);
  }
});

/** Runs one palette (⌘K) command by typing its name and committing the row with Enter. */
async function paletteCommand(page: Page, query: string, rowId: string): Promise<void> {
  await page.keyboard.press("Meta+KeyK");
  await expect(page.locator("[data-palette-input]")).toBeFocused();
  await page.keyboard.type(query);
  const row = page.locator(`[data-palette-row="${rowId}"]`);
  await expect(row).toBeVisible();
  for (let step = 0; step < 30 && (await row.getAttribute("aria-selected")) !== "true"; step += 1) await page.keyboard.press("ArrowDown");
  await expect(row).toHaveAttribute("aria-selected", "true");
  await expect(row).not.toHaveAttribute("aria-disabled", "true");
  await page.keyboard.press("Enter");
  await expect(page.locator("[data-palette-input]")).toHaveCount(0);
}

/**
 * Moves the keyboard with Tab or Shift+Tab until `target` has it. An editor
 * keeps Tab for indenting; Escape first lets the next Tab leave it, as
 * CodeMirror offers.
 */
async function tabTo(page: Page, target: Locator, key: "Tab" | "Shift+Tab"): Promise<void> {
  for (let step = 0; step < 40; step += 1) {
    if (await target.evaluate((node) => node === document.activeElement)) return;
    if (await page.evaluate(() => Boolean(document.activeElement?.closest(".cm-editor")))) await page.keyboard.press("Escape");
    await page.keyboard.press(key);
  }
  await expect(target).toBeFocused();
}

/** Opens the focused tab's menu with ⇧F10 and chooses `item` with the arrow keys and Enter. */
async function menuByKeyboard(page: Page, item: string): Promise<void> {
  await page.keyboard.press("Shift+F10");
  const target = page.locator(`[role="menu"] [data-menu-item="${item}"]`);
  await expect(target).toBeEnabled();
  for (let step = 0; step < 20 && !(await target.evaluate((node) => node === document.activeElement)); step += 1) await page.keyboard.press("ArrowDown");
  await expect(target).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.locator('[role="menu"]')).toHaveCount(0);
}

test("splits, moves, resizes, focus and closes run from the palette and menus by keyboard, each landing once", async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  // The split frame is delivered twice at the transport, as a resend would
  // be; the core must split once (B18).
  let pageSplits = 0;
  let duplicated = 0;
  const duplicateFirstSplit = () =>
    page.routeWebSocket(/\/ws$/, (ws) => {
      const server = ws.connectToServer();
      ws.onMessage((message) => {
        server.send(message);
        if (typeof message !== "string" || !message.includes('"kind":"view_layout"') || !message.includes('"action":"split"')) return;
        pageSplits += 1;
        if (duplicated > 0) return;
        duplicated += 1;
        server.send(message);
      });
    });
  const stack = await startStack(page, "s7-keys", { "a.txt": "a\n", "b.txt": "b\n", "c.txt": "c\n", "d.txt": "d\n" }, { beforeOpen: duplicateFirstSplit });
  try {
    for (const name of ["a.txt", "b.txt", "c.txt", "d.txt"]) await explorerRow(page, stack, name).dblclick();
    await page.locator('[data-layout-choice="views"]').click();
    const workspace = page.locator("[data-workspace-screen]");
    await expect(workspace).toHaveAttribute("data-layout", "views");
    await expect.poll(() => shape(page)).toBe("@(a.txt b.txt c.txt >d.txt)");
    const count = (key: string) => stack.sent.get(key) ?? 0;

    // Split right from the palette: one event, delivered twice, one new area;
    // the keyboard follows the view into it (B9, B18, B20).
    await paletteCommand(page, "Split right", "command:view:split_right");
    await expect.poll(() => shape(page)).toBe("(a.txt b.txt >c.txt) | @(>d.txt)");
    // The page sent one split; the wire carried it to the core twice.
    expect(pageSplits).toBe(1);
    expect(duplicated).toBe(1);
    expect(count("view_layout.split")).toBe(2);
    await page.waitForTimeout(500);
    await expect.poll(() => shape(page)).toBe("(a.txt b.txt >c.txt) | @(>d.txt)");
    await expect(editor(page, 1)).toBeFocused();

    // Focus moves between areas from the palette, and the keyboard goes along (B20).
    await paletteCommand(page, "Focus previous view area", "command:view:focus_previous");
    await expect.poll(() => shape(page)).toBe("@(a.txt b.txt >c.txt) | (>d.txt)");
    await expect(editor(page, 0)).toBeFocused();
    expect(count("view_layout.focus_area")).toBe(1);

    // The tab menu from the keyboard: Split down, then Move right; the
    // emptied area goes and its neighbour takes its height (B9, B10, B11).
    await tabTo(page, tab(area(page, 0), "b.txt"), "Shift+Tab");
    await menuByKeyboard(page, "split_down");
    await expect.poll(() => shape(page)).toBe("(a.txt >c.txt) | @(>b.txt) | (>d.txt)");
    const [top, bottom] = [await boxOf(area(page, 0)), await boxOf(area(page, 1))];
    expect(bottom.y).toBeGreaterThan(top.y + top.height - 1);
    await tabTo(page, tab(area(page, 1), "b.txt"), "Shift+Tab");
    await menuByKeyboard(page, "move_right");
    await expect.poll(() => shape(page)).toBe("(a.txt >c.txt) | @(d.txt >b.txt)");
    const views = await boxOf(page.locator("[data-view-areas]"));
    await expect.poll(async () => Math.round((await boxOf(area(page, 0))).height)).toBe(Math.round(views.height));
    await screenshot(page, "s7-keys-after-move");

    // A divider drag moves a guide line, resizes nothing while it lasts,
    // and lands one resize on release (B9, B18).
    const divider = page.locator("[data-view-divider]");
    await expect(divider).toHaveCount(1);
    const ratio = Number(await divider.getAttribute("aria-valuenow"));
    const before = await areaBoxes(page);
    const line = await boxOf(divider);
    await page.mouse.move(line.x + line.width / 2, line.y + line.height / 2);
    await page.mouse.down();
    await page.mouse.move(line.x + 60, line.y + line.height / 2, { steps: 4 });
    await page.mouse.move(line.x + 150, line.y + line.height / 2, { steps: 6 });
    await expect(page.locator("[data-view-resize-guide]")).toBeVisible();
    expect(await areaBoxes(page)).toEqual(before);
    expect(count("view_layout.resize")).toBe(0);
    await page.mouse.up();
    await expect(page.locator("[data-view-resize-guide]")).toHaveCount(0);
    await expect.poll(() => count("view_layout.resize")).toBe(1);
    await expect.poll(async () => Number(await divider.getAttribute("aria-valuenow"))).toBeGreaterThan(ratio + 5);
    await expect.poll(async () => Math.round((await boxOf(area(page, 0))).width - before[0]!.width)).toBeGreaterThan(140);
    await page.waitForTimeout(300);
    expect(count("view_layout.resize")).toBe(1);

    // The focused divider moves one step per arrow key, one event each (B9, B20).
    const dragged = Number(await divider.getAttribute("aria-valuenow"));
    await tabTo(page, divider, "Tab");
    await page.keyboard.press("ArrowLeft");
    await expect.poll(async () => Number(await divider.getAttribute("aria-valuenow"))).toBe(dragged - 5);
    await page.keyboard.press("ArrowLeft");
    await expect.poll(async () => Number(await divider.getAttribute("aria-valuenow"))).toBe(dragged - 10);
    expect(count("view_layout.resize")).toBe(3);
    // Tabbing through the left area's view on the way made it the area in use (B20).
    await expect.poll(() => shape(page)).toBe("@(a.txt >c.txt) | (d.txt >b.txt)");

    // Grow, focus and close from the palette act on the active area and its view (B20).
    await paletteCommand(page, "Grow view area", "command:view:grow");
    await expect.poll(async () => Number(await divider.getAttribute("aria-valuenow"))).toBe(dragged - 5);
    await paletteCommand(page, "Focus next view area", "command:view:focus_next");
    await expect.poll(() => shape(page)).toBe("(a.txt >c.txt) | @(d.txt >b.txt)");
    await paletteCommand(page, "Close view", "command:view:close_view");
    await expect.poll(() => shape(page)).toBe("(a.txt >c.txt) | @(>d.txt)");
    await paletteCommand(page, "Close view", "command:view:close_view");
    await expect.poll(() => shape(page)).toBe("@(a.txt >c.txt)");
    await expect(divider).toHaveCount(0);
    await expect.poll(async () => Math.round((await boxOf(area(page, 0))).width)).toBe(Math.round(views.width));

    // Closing the last view leaves the empty area with the way to a file,
    // and the layout stays what it was (B10).
    await paletteCommand(page, "Hide Explorer", "command:tool:explorer");
    await expect(page.locator('[data-tool="explorer"]')).toHaveCount(0);
    const layoutsBefore = stack.events.filter((event) => event.kind === "workspace_view" && "mode" in event.payload).length;
    await tabTo(page, tab(page, "c.txt"), "Shift+Tab");
    await menuByKeyboard(page, "close_view");
    await expect.poll(() => shape(page)).toBe("@(>a.txt)");
    await tabTo(page, tab(page, "a.txt"), "Shift+Tab");
    await menuByKeyboard(page, "close_view");
    await expect(page.locator('[data-area-empty="no-view"]')).toBeVisible();
    await expect(page.locator("[data-empty-open-explorer]")).toBeVisible();
    await expect(page.locator("[data-empty-open-file]")).toBeVisible();
    await expect(workspace).toHaveAttribute("data-layout", "views");
    expect(stack.events.filter((event) => event.kind === "workspace_view" && "mode" in event.payload).length).toBe(layoutsBefore);
    await screenshot(page, "s7-keys-empty");
    await page.locator("[data-empty-open-explorer]").click();
    await expect(page.locator('[data-tool="explorer"]')).toBeVisible();
    expectFrontWorkspaceOnEveryViewEvent(stack);
  } finally {
    stopStack(stack);
  }
});

type MenuRow = { id: string; label: string; reason: string | null; disabled: boolean };

/** The open menu's items as drawn: id, label, the reason under a disabled one. */
async function menuRows(page: Page): Promise<MenuRow[]> {
  return page.locator('[role="menu"] [data-menu-item]').evaluateAll((buttons) =>
    buttons.map((button) => {
      const lines = [...button.querySelectorAll("span")].map((span) => span.textContent ?? "");
      return { id: button.getAttribute("data-menu-item") ?? "", label: lines[0] ?? "", reason: lines[1] ?? null, disabled: (button as HTMLButtonElement).disabled };
    }),
  );
}

async function tabMenuRows(page: Page, scope: Page | Locator, name: string): Promise<MenuRow[]> {
  await tab(scope, name).click({ button: "right" });
  await expect(page.locator('[role="menu"]')).toBeVisible();
  const rows = await menuRows(page);
  await page.keyboard.press("Escape");
  await expect(page.locator('[role="menu"]')).toHaveCount(0);
  return rows;
}

const SPLITS = ["split_right", "split_left", "split_up", "split_down"];

/** A drag from one tab to a point near the right edge of another area's content, released in place. */
async function dragToEdge(page: Page, from: Locator, target: Locator, edge: "right" | "down", during?: () => Promise<void>): Promise<void> {
  const body = await boxOf(target.locator("[data-view-body]"));
  const point = edge === "right" ? { x: body.x + body.width * 0.92, y: body.y + body.height / 2 } : { x: body.x + body.width / 2, y: body.y + body.height * 0.92 };
  await dragTab(page, from, point, during);
}

test("the tab menu offers only what a view can do, and each cap refuses with its reason and leaves the views alone", async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  const files = Object.fromEntries(Array.from({ length: 65 }, (_, index) => [`f${String(index + 1).padStart(2, "0")}.txt`, `file ${index + 1}\n`]));
  const stack = await startStack(page, "s7-caps", files);
  try {
    await page.locator('[data-layout-choice="views"]').click();
    await explorerRow(page, stack, "f01.txt").click();
    await expect.poll(() => shape(page)).toBe("@(>f01.txt*)");
    const layoutEvents = () => viewEvents(stack).length;

    // A preview alone in its area: Keep open, four splits that cannot land
    // (with the reason), the path, Reveal and Close view; no Move without a
    // neighbour and never a file deletion. Opening it changes nothing (B11).
    const sentBefore = layoutEvents();
    const alone = await tabMenuRows(page, page, "f01.txt");
    expect(alone.map((row) => row.id)).toEqual(["keep_open", ...SPLITS, "copy_path", "reveal", "close_view"]);
    expect(alone.map((row) => row.label)).toEqual(["Keep open", "Split right", "Split left", "Split up", "Split down", "Copy path", "Reveal in Explorer", "Close view"]);
    for (const row of alone.filter((entry) => SPLITS.includes(entry.id))) expect(row).toMatchObject({ disabled: true, reason: "This is the only view in its area." });
    expect(layoutEvents()).toBe(sentBefore);
    await tab(page, "f01.txt").click({ button: "right" });
    await expect(page.locator('[role="menu"]')).not.toContainText(/Trash|Delete/);
    await screenshot(page, "s7-caps-menu-preview");
    await page.keyboard.press("Escape");

    // A pinned view has no Keep open; beside another area it can move there.
    // (The first click of each double click takes the preview's place, so
    // the f01 preview gives way to f02, B1.)
    for (let index = 2; index <= 10; index += 1) await explorerRow(page, stack, `f${String(index).padStart(2, "0")}.txt`).dblclick();
    await expect.poll(() => shape(page)).toBe("@(f02.txt f03.txt f04.txt f05.txt f06.txt f07.txt f08.txt f09.txt >f10.txt)");
    await tabMenu(page, page, "f10.txt", "split_right");
    await expect.poll(() => shape(page)).toBe("(f02.txt f03.txt f04.txt f05.txt f06.txt f07.txt f08.txt >f09.txt) | @(>f10.txt)");
    const left = await tabMenuRows(page, area(page, 0), "f02.txt");
    expect(left.map((row) => row.id)).toEqual([...SPLITS, "move_right", "copy_path", "reveal", "close_view"]);
    expect(left.every((row) => !row.disabled)).toBe(true);
    const right = await tabMenuRows(page, area(page, 1), "f10.txt");
    expect(right.map((row) => row.id)).toEqual([...SPLITS, "move_left", "copy_path", "reveal", "close_view"]);

    // Room is a reason too: in a narrower window the areas side by side
    // cannot be halved again across, only down (B9, D-06).
    await page.setViewportSize({ width: 1280, height: 1080 });
    await expect.poll(async () => (await boxOf(area(page, 0))).width).toBeLessThan(450);
    const cramped = await tabMenuRows(page, area(page, 0), "f02.txt");
    expect(cramped.find((row) => row.id === "split_right")).toMatchObject({ disabled: true, reason: "This view area is too narrow to split." });
    expect(cramped.find((row) => row.id === "split_down")).toMatchObject({ disabled: false });
    await page.setViewportSize({ width: 1920, height: 1080 });
    await expect.poll(async () => (await boxOf(area(page, 0))).width).toBeGreaterThan(600);

    // Down to the depth limit: an area three splits deep cannot split again (B9, B19).
    await tabMenu(area(page, 0), page, "f09.txt", "split_down");
    await tabMenu(area(page, 0), page, "f08.txt", "split_right");
    await expect.poll(() => page.locator("[data-view-area-id]").count()).toBe(4);
    const deep = await tabMenuRows(page, area(page, 0), "f02.txt");
    for (const row of deep.filter((entry) => SPLITS.includes(entry.id))) expect(row).toMatchObject({ disabled: true, reason: "View areas can be split only 3 levels deep." });

    // Up to the area limit by drops on other areas' edges: six areas (B7, B19).
    const areaWith = (name: string) => page.locator("[data-view-area-id]").filter({ has: tab(page, name) });
    await dragToEdge(page, tab(area(page, 0), "f07.txt"), areaWith("f10.txt"), "down");
    await expect.poll(() => page.locator("[data-view-area-id]").count()).toBe(5);
    await dragToEdge(page, tab(area(page, 0), "f06.txt"), areaWith("f09.txt"), "right");
    await expect.poll(() => page.locator("[data-view-area-id]").count()).toBe(6);
    const six = await shape(page);
    const splitsAtSix = stack.sent.get("view_layout.split") ?? 0;

    // At six every split is refused with the reason, in the menu, on a drag
    // and in the palette, and every view stays where it was (B9, B19).
    const capped = await tabMenuRows(page, area(page, 0), "f02.txt");
    for (const row of capped.filter((entry) => SPLITS.includes(entry.id))) expect(row).toMatchObject({ disabled: true, reason: "This Workspace already shows 6 view areas, the most it can." });
    await tab(area(page, 0), "f02.txt").click({ button: "right" });
    await screenshot(page, "s7-caps-area-limit-menu");
    await page.keyboard.press("Escape");
    await dragToEdge(page, tab(area(page, 0), "f05.txt"), areaWith("f10.txt"), "down", async () => {
      await expect(page.locator("[data-view-drop]")).toHaveCount(0);
      await expect(page.locator("html")).toHaveAttribute("data-view-drag", "forbidden");
      await expect(page.locator("[data-view-drag-tab]")).toContainText("This Workspace already shows 6 view areas, the most it can.");
    });
    // The palette acts on the active view: one of several in its area.
    await tab(area(page, 0), "f02.txt").click();
    await expect.poll(() => shape(page)).toMatch(/^@\(>f02\.txt/);
    const sixFocused = await shape(page);
    await page.keyboard.press("Meta+KeyK");
    await page.keyboard.type("Split right");
    const paletteRow = page.locator('[data-palette-row="command:view:split_right"]');
    await expect(paletteRow).toHaveAttribute("aria-disabled", "true");
    await expect(paletteRow).toContainText("This Workspace already shows 6 view areas, the most it can.");
    // Picking a command that cannot run does nothing: the palette stays open.
    await page.keyboard.press("Enter");
    await expect(page.locator("[data-palette-input]")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.locator("[data-palette-input]")).toHaveCount(0);
    expect(stack.sent.get("view_layout.split") ?? 0).toBe(splitsAtSix);
    expect(await shape(page)).toBe(sixFocused);
    expect(sixFocused.replace(/[>@]/g, "")).toBe(six.replace(/[>@]/g, ""));

    // Sixty-four views are the most one Workspace holds: an open past it is
    // refused with the reason and changes nothing, and opening a file already
    // shown still just moves to it (B19, contract 2).
    const seeded = {
      schema_version: 2,
      workspaces: [
        {
          device_id: "local",
          path: stack.root,
          mode: "views",
          explorer: true,
          changes: false,
          agent_share: 0.5,
          last_used_unix_ms: Date.now(),
          layout: {
            root: {
              area: {
                id: "a1",
                active: "d64",
                displays: Array.from({ length: 64 }, (_, index) => ({
                  id: `d${index + 1}`,
                  path: path.join(stack.root, `f${String(index + 1).padStart(2, "0")}.txt`),
                  kind: "file",
                  committed: null,
                  preview: false,
                  last_focused_unix_ms: 0,
                })),
              },
            },
            active_area: "a1",
            next_id: 65,
          },
        },
      ],
    };
    stack.daemon = await stack.daemon.restart((dir) => fs.writeFileSync(path.join(dir, "workspace-views.json"), JSON.stringify(seeded)));
    await page.goto("about:blank");
    await open(page, stack.daemon);
    await expect(page.locator('[data-view-area-id] [role="tab"][data-view-state="open"]')).toHaveCount(64, { timeout: 30_000 });
    await explorerRow(page, stack, "f65.txt").click();
    await expect(page.locator("[data-notice]")).toContainText("This Workspace has 64 views open. Close a view to open another.");
    await expect(page.locator('[data-view-area-id] [role="tab"]')).toHaveCount(64);
    await expect(tab(page, "f65.txt")).toHaveCount(0);
    await screenshot(page, "s7-caps-display-limit");
    await page.locator('[data-notice] button[aria-label="Dismiss"]').click();
    await explorerRow(page, stack, "f60.txt").click();
    await expect(tab(page, "f60.txt")).toHaveAttribute("aria-selected", "true");
    await expect(page.locator("[data-notice]")).toHaveCount(0);
    await expect(page.locator('[data-view-area-id] [role="tab"]')).toHaveCount(64);
    expectFrontWorkspaceOnEveryViewEvent(stack);
  } finally {
    stopStack(stack);
  }
});

type StoredWorkspace = { device_id: string; path: string; mode: string; explorer: boolean; changes: boolean; agent_share: number; layout: unknown };

/** This Workspace's entry in `workspace-views.json`, without the clock stamps a focus writes. */
function storedWorkspace(daemon: Daemon, root: string): StoredWorkspace | null {
  let text: string;
  try {
    text = fs.readFileSync(viewsFile(daemon), "utf8");
  } catch {
    return null;
  }
  const stored = JSON.parse(text, (key, value: unknown) => (key === "last_used_unix_ms" || key === "last_focused_unix_ms" ? undefined : value)) as {
    workspaces: StoredWorkspace[];
  };
  return stored.workspaces.find((entry) => entry.device_id === "local" && entry.path === root) ?? null;
}

test("a narrow window floats the tools, shows one region and one area with a way to the others, and stores none of it", async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  const stack = await startStack(page, "s7-narrow", { "a.txt": "a\n", "b.txt": "b\n" });
  try {
    await explorerRow(page, stack, "a.txt").dblclick();
    await explorerRow(page, stack, "b.txt").dblclick();
    await tabMenu(page, page, "b.txt", "split_right");
    await expect.poll(() => shape(page)).toBe("(>a.txt) | @(>b.txt)");
    const divider = page.locator("[data-view-divider]");
    await divider.focus();
    await page.keyboard.press("ArrowLeft");
    await expect(divider).toHaveAttribute("aria-valuenow", "45");
    const workspace = page.locator("[data-workspace-screen]");
    await expect(workspace).toHaveAttribute("data-layout", "together");
    await expect.poll(() => (storedWorkspace(stack.daemon, stack.root)?.layout as { root?: { split?: { ratio?: number } } } | undefined)?.root?.split?.ratio).toBeCloseTo(0.45);
    const stored = storedWorkspace(stack.daemon, stack.root);
    const sentBefore = stack.events.filter((event) => event.kind === "workspace_view").length;
    const body = page.locator("[data-workspace-body]");
    const sidebar = (await boxOf(body)).x;
    const toggle = page.locator('[data-tool-toggle="explorer"]');

    // Too narrow for the tool column: the tools float, closed until asked for,
    // and the View areas that cannot all fit show the active one with a
    // switcher to the others (B12, B13).
    await page.setViewportSize({ width: Math.round(sidebar + 700), height: 1080 });
    await expect(body).toHaveAttribute("data-workspace-body", "narrow");
    await expect(page.locator("[data-workspace-tools]")).toHaveCount(0);
    await expect(toggle).toHaveAttribute("aria-pressed", "false");
    await expect(page.locator('[data-view-areas="single"]')).toBeVisible();
    await expect(page.locator("[data-view-area-id]")).toHaveCount(1);
    await expect.poll(() => shape(page)).toBe("@(>b.txt)");
    const switcher = page.locator("[data-view-area-switch]");
    await expect(switcher).toHaveText("2/2 ▾");
    await screenshot(page, "s7-narrow-single-area");

    // The overlay opens on request, takes the keyboard, and Escape closes it
    // with the keyboard back on its toggle (B12).
    await toggle.click();
    const overlay = page.locator('[data-tools-overlay="true"]');
    await expect(overlay).toBeVisible();
    await expect(overlay.locator('[data-tool="explorer"]')).toBeVisible();
    await expect(overlay).toBeFocused();
    await screenshot(page, "s7-narrow-tools-overlay");
    await page.keyboard.press("Escape");
    await expect(overlay).toHaveCount(0);
    await expect(toggle).toBeFocused();
    // A click outside it (here the toolbar's own padding, which takes no
    // focus) closes it too (B12).
    await toggle.click();
    await expect(overlay).toBeVisible();
    await page.locator("[data-workspace-toolbar]").click({ position: { x: 3, y: 3 } });
    await expect(overlay).toHaveCount(0);

    // The switcher shows another area; choosing one is an ordinary focus (B13).
    await switcher.click();
    await page.locator('[role="menu"] [data-menu-item]').first().click();
    await expect.poll(() => shape(page)).toBe("@(>a.txt)");
    await expect(switcher).toHaveText("1/2 ▾");
    await switcher.click();
    await page.locator('[role="menu"] [data-menu-item]').nth(1).click();
    await expect.poll(() => shape(page)).toBe("@(>b.txt)");

    // Too narrow for Agents and Views side by side: the region worked in last
    // fills the width, with an explicit switch to the other (B13).
    await page.setViewportSize({ width: Math.round(sidebar + 420), height: 1080 });
    const regions = page.locator("[data-region-switch]");
    await expect(regions).toBeVisible();
    await expect(regions).toHaveAttribute("data-region-switch", "views");
    await expect(page.locator("[data-agent-area]")).toHaveCount(0);
    await expect(page.locator("[data-view-area]")).toBeVisible();
    await screenshot(page, "s7-narrow-one-region");
    await page.locator('[data-region-choice="agents"]').click();
    await expect(page.locator("[data-agent-area]")).toBeVisible();
    await expect(page.locator("[data-view-area]")).toHaveCount(0);
    await page.locator('[data-region-choice="views"]').click();
    await expect(page.locator("[data-view-area]")).toBeVisible();

    // Widening brings the stored layout back: both regions, both areas at
    // their ratio and the tool column; nothing narrow was sent or stored (B13).
    await page.setViewportSize({ width: 1920, height: 1080 });
    await expect(body).toHaveAttribute("data-workspace-body", "wide");
    await expect(page.locator("[data-region-switch]")).toHaveCount(0);
    await expect(page.locator("[data-agent-area]")).toBeVisible();
    await expect(page.locator('[data-view-areas="tree"]')).toBeVisible();
    await expect.poll(() => shape(page)).toBe("(>a.txt) | @(>b.txt)");
    await expect(divider).toHaveAttribute("aria-valuenow", "45");
    await expect(page.locator('[data-workspace-tools="explorer"]')).toBeVisible();
    await expect(page.locator("[data-tools-overlay]")).toHaveCount(0);
    expect(stack.events.filter((event) => event.kind === "workspace_view").length).toBe(sentBefore);
    await expect.poll(() => storedWorkspace(stack.daemon, stack.root)).toEqual(stored);
    expectFrontWorkspaceOnEveryViewEvent(stack);
  } finally {
    stopStack(stack);
  }
});

// PRD B12 and DESIGN.md "Narrow windows": a click outside the narrow tools
// overlay closes it and returns the keyboard to the toggle that opened it,
// as Escape does. The page closes it but leaves the keyboard on the page's
// body when the click lands on something that takes no focus: the outside
// press handler in `Tools` (web/src/Tools.tsx) closes without restoring focus.
test("an outside click that closes the narrow tools overlay gives the keyboard back to its toggle", async ({ page }) => {
  test.fail(true, "product bug: Tools.tsx closes the overlay on an outside press without returning focus to the toggle (PRD B12)");
  await page.setViewportSize({ width: 1920, height: 1080 });
  const stack = await startStack(page, "s7-overlay-focus", { "a.txt": "a\n" });
  try {
    const sidebar = (await boxOf(page.locator("[data-workspace-body]"))).x;
    await page.setViewportSize({ width: Math.round(sidebar + 440), height: 1080 });
    await expect(page.locator("[data-workspace-body]")).toHaveAttribute("data-workspace-body", "narrow");
    const toggle = page.locator('[data-tool-toggle="explorer"]');
    await toggle.click();
    const overlay = page.locator('[data-tools-overlay="true"]');
    await expect(overlay).toBeVisible();
    await page.locator("[data-workspace-toolbar]").click({ position: { x: 3, y: 3 } });
    await expect(overlay).toHaveCount(0);
    await expect(toggle).toBeFocused();
  } finally {
    stopStack(stack);
  }
});

/** The pane and tab ids the private Herdr holds, to show a restart used them as they were (B15). */
function herdrIds(herdr: HerdrFixture): string[] {
  const text = JSON.stringify(herdr.run(["api", "snapshot"]));
  return [...new Set([...text.matchAll(/"(?:pane_id|tab_id)":"([^"]+)"/g)].map((match) => match[1] ?? ""))].sort();
}

/** Reopens the app as a fresh page on the daemon, as relaunching it does. */
async function reopen(page: Page, daemon: Daemon): Promise<void> {
  await page.goto("about:blank");
  await open(page, daemon);
}

test("a restart brings the View areas back as they were, and a file gone meanwhile is unavailable on its own view until Retry", async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  const stack = await startStack(page, "s7-restore", { "a.txt": "a text\n", "b.txt": "b text\n", "c.txt": "c text\n", "d.txt": "d text\n", "e.txt": "e text\n" });
  try {
    const row = (name: string) => explorerRow(page, stack, name);
    // Three areas, a preview, two ratios, Views only, History without the Explorer.
    await row("a.txt").dblclick();
    await row("b.txt").dblclick();
    await row("c.txt").click();
    await tabMenu(page, page, "b.txt", "split_right");
    await expect.poll(() => shape(page)).toBe("(a.txt >c.txt*) | @(>b.txt)");
    await row("d.txt").dblclick();
    await tabMenu(page, page, "d.txt", "split_down");
    await row("e.txt").dblclick();
    await expect.poll(() => shape(page)).toBe("(a.txt >c.txt*) | (>b.txt) | @(d.txt >e.txt)");
    const dividers = page.locator("[data-view-divider]");
    await expect(dividers).toHaveCount(2);
    await dividers.nth(0).focus();
    await page.keyboard.press("ArrowLeft");
    await dividers.nth(1).focus();
    await page.keyboard.press("ArrowDown");
    await expect(dividers.nth(0)).toHaveAttribute("aria-valuenow", "45");
    await expect(dividers.nth(1)).toHaveAttribute("aria-valuenow", "55");
    await tab(area(page, 2), "e.txt").click();
    await page.locator('[data-layout-choice="views"]').click();
    await page.locator('[data-tool-toggle="changes"]').click();
    await page.locator('[data-tool-close="explorer"]').click();
    await expect(page.locator('[data-tool="changes"]')).toBeVisible();
    await expect(page.locator('[data-tool="explorer"]')).toHaveCount(0);
    const before = await shape(page);
    expect(before).toBe("(a.txt >c.txt*) | (>b.txt) | @(d.txt >e.txt)");
    await expect.poll(() => storedWorkspace(stack.daemon, stack.root)).toMatchObject({ mode: "views", explorer: false, changes: true });
    await expect
      .poll(() => JSON.stringify(storedWorkspace(stack.daemon, stack.root)?.layout))
      .toMatch(/"ratio":0\.45.*"ratio":0\.55/);
    const panes = herdrIds(stack.herdr);
    await screenshot(page, "s7-restore-before");

    // The file goes while Hide is stopped, and Hide comes back on the same state.
    stack.daemon = await stack.daemon.restart(() => fs.rmSync(path.join(stack.root, "e.txt")));
    await reopen(page, stack.daemon);

    // The same Workspace, not Main: the tree, ratios, order, preview, each
    // area's view, the area in use, the layout and the tools (B14).
    const workspace = page.locator("[data-workspace-screen]");
    await expect(workspace).toHaveAttribute("data-layout", "views", { timeout: 20_000 });
    await expect(page.locator("[data-main-screen]")).toHaveCount(0);
    await expect.poll(() => shape(page), { timeout: 15_000 }).toBe(before);
    await expect(dividers.nth(0)).toHaveAttribute("aria-valuenow", "45");
    await expect(dividers.nth(1)).toHaveAttribute("aria-valuenow", "55");
    await expect(page.locator('[data-tool="changes"]')).toBeVisible();
    await expect(page.locator('[data-tool="explorer"]')).toHaveCount(0);
    await expect(tab(page, "c.txt").getByText("c.txt")).toHaveCSS("font-style", "italic");
    await expect(editor(page, 0)).toContainText("c text");
    await expect(editor(page, 1)).toContainText("b text");

    // Only the gone file's view is unavailable, with Close view and Retry;
    // the others and their areas stay (B16).
    await expect(tab(page, "e.txt")).toHaveAttribute("data-unavailable", "true", { timeout: 15_000 });
    await expect(page.locator('[data-view-area-id] [role="tab"][data-unavailable="true"]')).toHaveCount(1);
    await expect(area(page, 2).locator("[data-close-unavailable]")).toBeVisible();
    await expect(area(page, 2).locator("[data-retry-unavailable]")).toBeVisible();
    await expect(page.locator("[data-close-unavailable]")).toHaveCount(1);
    await expect(page.locator("[data-retry-unavailable]")).toHaveCount(1);
    await screenshot(page, "s7-restore-unavailable");

    // Herdr's panes and tabs are used as they are: nothing was started or rewritten (B15).
    expect(herdrIds(stack.herdr)).toEqual(panes);

    // Retry reads the file again once it is back (B16).
    fs.writeFileSync(path.join(stack.root, "e.txt"), "e is back\n");
    await area(page, 2).locator("[data-retry-unavailable]").click();
    await expect(editor(page, 2)).toContainText("e is back");
    await expect(tab(page, "e.txt")).toHaveAttribute("data-unavailable", "false");
    expect(stack.last.get("view_layout.retry")).toMatchObject({ action: "retry" });
    await expect.poll(() => shape(page)).toBe(before);
    expectFrontWorkspaceOnEveryViewEvent(stack);
  } finally {
    stopStack(stack);
  }
});

test("a broken or unknown Views file is kept aside for Main and a fresh layout, and an S6 file becomes one area", async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  const stack = await startStack(page, "s7-recover", { "a.txt": "a text\n", "b.txt": "b text\n", "c.txt": "c text\n" });
  try {
    await explorerRow(page, stack, "a.txt").dblclick();
    await expect.poll(() => storedWorkspace(stack.daemon, stack.root)).not.toBeNull();
    const main = page.locator("[data-main-screen]");
    const workspace = page.locator("[data-workspace-screen]");
    const aside = () => fs.readdirSync(stateDir(stack.daemon)).filter((name) => name.startsWith("workspace-views.json.unreadable-"));

    // Unparseable, then a version this build does not know: each is moved
    // aside byte for byte, the app starts on Main, and a Workspace chosen
    // there starts a new layout (B17).
    for (const [index, broken] of ["{ not json", '{"schema_version": 3, "workspaces": [{"device_id": "local"}]}\n'].entries()) {
      stack.diagnostics.clear();
      stack.daemon = await stack.daemon.restart((dir) => fs.writeFileSync(path.join(dir, "workspace-views.json"), broken));
      await reopen(page, stack.daemon);
      await expect(main).toBeVisible({ timeout: 20_000 });
      await expect(workspace).toHaveCount(0);
      const kept = aside().map((name) => fs.readFileSync(path.join(stateDir(stack.daemon), name), "utf8"));
      expect(kept).toHaveLength(index + 1);
      expect(kept).toContain(broken);
      await expect.poll(() => stack.diagnostics.has("workspace_views.unreadable")).toBe(true);
      if (index === 0) await screenshot(page, "s7-recover-main");
      await enterWorkspace(page, "fixture");
      await expect(workspace).toHaveAttribute("data-layout", "agents");
      await explorerRow(page, stack, "b.txt").click();
      await expect.poll(() => shape(page)).toBe("@(>b.txt*)");
      await expect.poll(() => JSON.parse(fs.readFileSync(viewsFile(stack.daemon), "utf8")).schema_version as number).toBe(2);
    }

    // An S6 (schema 1) file: its tabs become one area in their order, with
    // its active tab, preview, layout and tools; the next save writes
    // schema 2 (B17, contract 1).
    const v1 = {
      schema_version: 1,
      workspaces: [
        {
          device_id: "local",
          path: stack.root,
          mode: "views",
          explorer: false,
          changes: true,
          agent_share: 0.4,
          tabs: ["a.txt", "b.txt", "c.txt"].map((name) => ({ path: path.join(stack.root, name), kind: "file", preview: name === "c.txt" })),
          active: { path: path.join(stack.root, "b.txt"), kind: "file" },
          last_used_unix_ms: Date.now(),
        },
      ],
    };
    stack.diagnostics.clear();
    stack.daemon = await stack.daemon.restart((dir) => fs.writeFileSync(path.join(dir, "workspace-views.json"), JSON.stringify(v1)));
    await reopen(page, stack.daemon);
    await expect(workspace).toHaveAttribute("data-layout", "views", { timeout: 20_000 });
    await expect.poll(() => shape(page), { timeout: 15_000 }).toBe("@(a.txt >b.txt c.txt*)");
    await expect(page.locator('[data-tool="changes"]')).toBeVisible();
    await expect(page.locator('[data-tool="explorer"]')).toHaveCount(0);
    await expect(editor(page, 0)).toContainText("b text");
    await expect.poll(() => stack.diagnostics.has("workspace_views.migrated")).toBe(true);
    await screenshot(page, "s7-recover-migrated");
    await tab(page, "a.txt").click();
    await expect.poll(() => {
      const stored = JSON.parse(fs.readFileSync(viewsFile(stack.daemon), "utf8")) as { schema_version: number; workspaces: { path: string; layout?: { root?: { area?: { displays?: { path: string; preview: boolean }[] } } } }[] };
      const entry = stored.workspaces.find((candidate) => candidate.path === stack.root);
      return { version: stored.schema_version, displays: entry?.layout?.root?.area?.displays?.map((display) => `${path.basename(display.path)}${display.preview ? "*" : ""}`) };
    }).toEqual({ version: 2, displays: ["a.txt", "b.txt", "c.txt*"] });
    expectFrontWorkspaceOnEveryViewEvent(stack);
  } finally {
    stopStack(stack);
  }
});

/** A committed checkout whose `a.txt` then changes, so History lists a working diff. */
function gitCheckout(checkout: string): void {
  const git = (...args: string[]) => execFileSync("git", args, { cwd: checkout });
  git("init", "-q", "-b", "main");
  git("add", "-A");
  git("-c", "user.email=e2e@example.com", "-c", "user.name=e2e", "-c", "commit.gpgsign=false", "commit", "-qm", "base");
  fs.appendFileSync(path.join(checkout, "a.txt"), "changed\n");
}

test("S6's layouts, tools, kind marks and the open from Agents only keep working beside several View areas", async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  const stack = await startStack(page, "s7-s6", { "a.txt": "a text\n", "b.txt": "b text\n", "c.txt": "c text\n" }, { prepare: gitCheckout });
  try {
    const workspace = page.locator("[data-workspace-screen]");
    const layouts = () => stack.events.filter((event) => event.kind === "workspace_view" && "mode" in event.payload).length;
    const tools = () => stack.events.filter((event) => event.kind === "workspace_view" && ("explorer" in event.payload || "changes" in event.payload)).length;

    // Three layout icons, one chosen (B22, S6 D-03).
    const choices = page.locator("[data-layout-choice]");
    await expect(choices).toHaveCount(3);
    expect(await choices.evaluateAll((buttons) => buttons.map((button) => button.getAttribute("aria-label")))).toEqual(["Agents only", "Agents and Views", "Views only"]);
    await expect(page.locator('[data-layout-choice="agents"]')).toHaveAttribute("aria-checked", "true");

    // Choosing Views only never splits: one empty View area, then one area for a file (B22).
    await page.locator('[data-layout-choice="views"]').click();
    await expect(workspace).toHaveAttribute("data-layout", "views");
    await expect(page.locator('[data-area-empty="no-view"]')).toBeVisible();
    await expect(page.locator("[data-view-divider]")).toHaveCount(0);
    await explorerRow(page, stack, "a.txt").dblclick();
    await page.locator('[data-layout-choice="together"]').click();
    await expect(workspace).toHaveAttribute("data-layout", "together");
    await expect.poll(() => shape(page)).toBe("@(>a.txt)");
    await expect(page.locator("[data-view-divider]")).toHaveCount(0);
    expect(viewEvents(stack)).toHaveLength(0);

    // Kind marks, never colour alone: a file's type mark, a diff's comparison
    // mark, and each agent tab's provider mark (B22, S6 D-15).
    await page.locator('[data-tool-toggle="changes"]').click();
    await page.locator('[data-history-group="working"][data-history-path="a.txt"]').click();
    await expect.poll(() => shape(page)).toBe("@(a.txt >a.txt*)");
    await expect(page.locator('[data-view-area-id] [data-tab-kind="file"] [data-view-mark="file"]')).toHaveCount(1);
    await expect(page.locator('[data-view-area-id] [data-tab-kind="diff"] [data-view-mark="diff"]')).toHaveCount(1);
    await expect(page.locator('[data-tab-kind="diff"]')).toHaveAttribute("aria-label", /^Working diff: /);
    await expect(page.locator("[data-agent-tab-bar] [data-agent-mark]").first()).toBeVisible();
    await screenshot(page, "s7-s6-kind-marks");

    // Two areas, the right one used last. The diff is pinned first, since
    // the next click would take its preview slot (B1).
    await page.locator('[data-tab-kind="diff"]').dblclick();
    await expect.poll(() => shape(page)).toBe("@(a.txt >a.txt)");
    await explorerRow(page, stack, "b.txt").dblclick();
    await tabMenu(page, page, "b.txt", "split_right");
    await expect.poll(() => shape(page)).toBe("(a.txt >a.txt) | @(>b.txt)");

    // Explorer and History open and close on their own beside several areas,
    // one event each, and the areas stay as they are (B22, S6 B10).
    const toolsBefore = tools();
    const viewsBefore = viewEvents(stack).length;
    await page.locator('[data-tool-close="explorer"]').click();
    await expect(page.locator('[data-tool="explorer"]')).toHaveCount(0);
    await expect(page.locator('[data-tool="changes"]')).toBeVisible();
    await page.locator('[data-tool-toggle="explorer"]').click();
    await expect(page.locator('[data-tool="explorer"]')).toBeVisible();
    await page.locator('[data-tool-toggle="changes"]').click();
    await expect(page.locator('[data-tool="changes"]')).toHaveCount(0);
    await expect(page.locator('[data-tool="explorer"]')).toBeVisible();
    expect(tools()).toBe(toolsBefore + 3);
    expect(viewEvents(stack).length).toBe(viewsBefore);
    await expect.poll(() => shape(page)).toBe("(a.txt >a.txt) | @(>b.txt)");

    // A file opened from Agents only brings the Views back beside the agents
    // and lands in the area used last, which is the one in use (B22, S6 B11).
    await page.locator('[data-layout-choice="agents"]').click();
    await expect(workspace).toHaveAttribute("data-layout", "agents");
    await expect(page.locator("[data-view-area-id]")).toHaveCount(0);
    const layoutsBefore = layouts();
    await explorerRow(page, stack, "c.txt").click();
    await expect(workspace).toHaveAttribute("data-layout", "together");
    await expect.poll(() => shape(page)).toBe("(a.txt >a.txt) | @(b.txt >c.txt*)");
    await expect(area(page, 1)).toHaveAttribute("data-active-area", "true");
    expect(layouts()).toBe(layoutsBefore);
    await screenshot(page, "s7-s6-open-from-agents");
    expectFrontWorkspaceOnEveryViewEvent(stack);
  } finally {
    stopStack(stack);
  }
});
