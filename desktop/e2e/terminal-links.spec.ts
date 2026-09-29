// Terminal links end to end (docs/UI_BEHAVIOR.md, Terminal links): text a
// shell printed into a pane is a link where it names a URL or a path that
// exists, whatever program printed it. A click opens it in the Workspace (a
// browser display, the file in View at its line, a folder in the Explorer),
// ⌘-click hands it to macOS, a path outside every checkout goes to macOS on a
// plain click too and a program there is only revealed, a path the terminal
// wrapped is one link, a program's own OSC 8 link opens without xterm's
// confirm dialog, a drag across a link selects rather than opens, and no link
// click reaches the program as a mouse click. Everything runs on a private Herdr server, hided and Electron
// profile (see `fixture.ts`); macOS's own handlers are replaced with
// recorders, so nothing opens outside the test.

import { expect, type ElectronApplication, type Page } from "@playwright/test";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import type { AddressInfo } from "node:net";
import path from "node:path";
import { startHerdr, type HerdrFixture } from "../../web/e2e/herdr-fixture";
import { countSent, enterWorkspace } from "../../web/e2e/wire";
import { hostLog, isolate, launch, screenshot, shellPage, test, type Isolated } from "./fixture";

test.describe.configure({ timeout: 240_000 });
test.use({ actionTimeout: 15_000 });

let herdr: HerdrFixture;
let run: Isolated;
/** A folder the test made outside every checkout, removed however the test ends. */
let scratch: string | null = null;
let app: ElectronApplication | null = null;
let server: http.Server;
let origin: string;

test.beforeAll(async () => {
  herdr = await startHerdr({ agents: false });
  server = http.createServer((request, response) => {
    const title = { "/a.html": "Page A", "/b.html": "Page B" }[request.url ?? ""];
    response.writeHead(title ? 200 : 404, { "content-type": "text/html; charset=utf-8" });
    response.end(title ? `<!doctype html><title>${title}</title><h1>${title}</h1>` : "not found");
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});

test.afterAll(async () => {
  herdr?.stop();
  await new Promise((resolve) => server?.close(resolve));
});

test.beforeEach(() => {
  run = isolate(herdr, "terminal-links");
});

test.afterEach(async () => {
  const info = test.info();
  if (info.status !== info.expectedStatus) console.log(hostLog(run.env).map((line) => JSON.stringify(line)).join("\n"));
  await app?.close().catch(() => undefined);
  app = null;
  run.cleanup();
  if (scratch) fs.rmSync(scratch, { recursive: true, force: true });
  scratch = null;
});

type Probe = { paneId: () => string | null; paneText: (id: string) => string; paneGrid: (id: string) => { cols: number; rows: number } | null };

/** Reloads the shell with the terminal probe, the only way to read what xterm draws through WebGL. */
async function withProbe(page: Page): Promise<void> {
  await page.evaluate(() => {
    const url = new URL(location.href);
    url.searchParams.set("probe", "1");
    location.href = url.href;
  });
  await expect
    .poll(() => page.evaluate(() => typeof (window as { __hideProbe?: unknown }).__hideProbe).catch(() => "reloading"), { timeout: 20_000 })
    .toBe("object");
}

/** Prints `lines` in the fixture pane through `cat`, so the shell draws them as a program would. */
async function print(page: Page, paneId: string, lines: string[], last: string): Promise<void> {
  const file = path.join(herdr.root, `print-${Date.now()}.txt`);
  fs.writeFileSync(file, `${lines.join("\n")}\n`);
  const sent = spawnSync(herdr.bin, ["pane", "send-text", herdr.panes[0], `clear; cat '${file}'\n`], { env: herdr.env, encoding: "utf8", timeout: 10_000 });
  expect(sent.status, sent.stderr).toBe(0);
  await expect.poll(() => page.evaluate(([id, text]) => (window as unknown as { __hideProbe: Probe }).__hideProbe.paneText(id).includes(text), [paneId, last] as const), { timeout: 20_000 }).toBe(true);
}

/** Where `text` is on the screen of `paneId`: its row and the column of its `offset`th character. */
async function cellOf(page: Page, paneId: string, text: string, offset = 0): Promise<{ row: number; column: number }> {
  const lines = await page.evaluate((id) => (window as unknown as { __hideProbe: Probe }).__hideProbe.paneText(id).split("\n"), paneId);
  const row = lines.findIndex((line) => line.includes(text));
  expect(row, `${text} is not on the screen:\n${lines.join("\n")}`).toBeGreaterThanOrEqual(0);
  return { row, column: lines[row]!.indexOf(text) + offset };
}

/** The window point at the middle of a cell of `paneId`'s screen. */
async function cellPoint(page: Page, paneId: string, cell: { row: number; column: number }): Promise<{ x: number; y: number }> {
  const grid = await page.evaluate((id) => (window as unknown as { __hideProbe: Probe }).__hideProbe.paneGrid(id)!, paneId);
  const box = (await page.locator(`[data-terminal="${paneId}"] .xterm-screen`).boundingBox())!;
  return { x: box.x + ((cell.column + 0.5) * box.width) / grid.cols, y: box.y + ((cell.row + 0.5) * box.height) / grid.rows };
}

async function pointOf(page: Page, paneId: string, text: string, { offset = 0 } = {}): Promise<{ x: number; y: number }> {
  return cellPoint(page, paneId, await cellOf(page, paneId, text, offset));
}

/** Moves over a point and waits until xterm shows a link there. */
async function hoverLink(page: Page, paneId: string, point: { x: number; y: number }): Promise<void> {
  await page.mouse.move(point.x - 3, point.y);
  await page.mouse.move(point.x, point.y);
  await expect(page.locator(`[data-terminal="${paneId}"] .xterm-screen`)).toHaveClass(/xterm-cursor-pointer/, { timeout: 10_000 });
}

/** A click with ⌘ held; the mouse carries no modifiers of its own. */
async function metaClick(page: Page, point: { x: number; y: number }): Promise<void> {
  await page.keyboard.down("Meta");
  await page.mouse.click(point.x, point.y);
  await page.keyboard.up("Meta");
}

/** A View tab by what its label carries. */
function viewTab(page: Page, text: string) {
  return page.locator(`[data-view-tab-bar] [role="tab"][data-display][aria-label*="${text}"]`);
}

/** Replaces macOS's handlers in the main process with recorders. */
async function recordMacOS(): Promise<void> {
  await app!.evaluate(({ shell }) => {
    const opened: { how: string; target: string }[] = [];
    (globalThis as { opened?: unknown }).opened = opened;
    shell.openExternal = async (target: string) => {
      opened.push({ how: "external", target });
    };
    shell.openPath = async (target: string) => {
      opened.push({ how: "open", target });
      return "";
    };
    shell.showItemInFolder = (target: string) => {
      opened.push({ how: "reveal", target });
    };
  });
}

function opened(): Promise<{ how: string; target: string }[]> {
  return app!.evaluate(() => (globalThis as { opened?: { how: string; target: string }[] }).opened ?? []);
}

test("links: a pane's URLs and paths open in the Workspace on a click and in macOS on ⌘-click, with no confirm dialog and no click for the program", async () => {
  const checkout = path.join(fs.realpathSync(herdr.root), "fixture");
  fs.mkdirSync(path.join(checkout, "src"), { recursive: true });
  fs.writeFileSync(path.join(checkout, "src", "linked.ts"), Array.from({ length: 60 }, (_, index) => `// line ${index + 1}`).join("\n"));
  // Short, so its line never wraps; macOS keeps /tmp at /private/tmp.
  scratch = fs.mkdtempSync("/tmp/hide-links-");
  const outside = path.join(scratch, "o.txt");
  fs.writeFileSync(outside, "outside every checkout");
  const program = path.join(scratch, "tool");
  fs.writeFileSync(program, "#!/bin/sh\necho ran\n", { mode: 0o755 });
  test.info().attach("scratch", { body: scratch });

  ({ app } = await launch(run.env));
  const page = await shellPage(app);
  // Counted from the reload on, so the socket it opens is heard.
  const sent = countSent(page);
  await withProbe(page);
  await enterWorkspace(page, "fixture");
  const dialogs: string[] = [];
  page.on("dialog", (dialog) => {
    dialogs.push(dialog.message());
    void dialog.dismiss();
  });
  await recordMacOS();
  const paneId = await page.evaluate(() => (window as unknown as { __hideProbe: Probe }).__hideProbe.paneId()!);
  const grid = await page.evaluate((id) => (window as unknown as { __hideProbe: Probe }).__hideProbe.paneGrid(id)!, paneId);

  // A path longer than the pane is wide, so the terminal wraps it.
  const segments: string[] = [];
  while (["deep", ...segments].join("/").length < grid.cols + 8) segments.push(`segment-${segments.length}`);
  const wrapped = path.join("deep", ...segments, "wrapped.md");
  fs.mkdirSync(path.join(checkout, path.dirname(wrapped)), { recursive: true });
  fs.writeFileSync(path.join(checkout, wrapped), "# wrapped");

  await print(
    page,
    paneId,
    [
      `url ${origin}/a.html`,
      "file src/linked.ts:42 and missing/nothing.ts",
      `outside ${outside}`,
      `program ${program}`,
      `osc8 \u001b]8;;${origin}/b.html\u001b\\Page B link\u001b]8;;\u001b\\`,
      `osc8 \u001b]8;;file://${checkout}/src/linked.ts#L7\u001b\\Linked at 7\u001b]8;;\u001b\\`,
      "folder ./src",
      wrapped,
      "end-of-links",
    ],
    "end-of-links",
  );

  // A URL opens as a browser display in the Workspace, and the program hears no click.
  const clicksBefore = sent.get("terminal_click") ?? 0;
  let point = await pointOf(page, paneId, `${origin}/a.html`, { offset: 4 });
  await hoverLink(page, paneId, point);
  await page.mouse.click(point.x, point.y);
  await expect(viewTab(page, "Page A")).toBeVisible({ timeout: 20_000 });
  expect(sent.get("terminal_click") ?? 0).toBe(clicksBefore);
  expect(dialogs).toEqual([]);

  // A path that exists opens its file in View at the line; one that does not is no link.
  point = await pointOf(page, paneId, "src/linked.ts:42", { offset: 2 });
  await hoverLink(page, paneId, point);
  await screenshot(page, "terminal-link-hover");
  await page.mouse.click(point.x, point.y);
  await expect(viewTab(page, "linked.ts")).toBeVisible({ timeout: 20_000 });
  await expect(page.locator(".cm-activeLine").first()).toHaveText("// line 42");
  await screenshot(page, "terminal-link-file-at-line");
  expect(sent.get("reveal_path")).toBe(1);
  point = await pointOf(page, paneId, "missing/nothing.ts", { offset: 2 });
  await page.mouse.move(point.x, point.y);
  await page.waitForTimeout(500);
  await expect(page.locator(`[data-terminal="${paneId}"] .xterm-screen`)).not.toHaveClass(/xterm-cursor-pointer/);

  // ⌘-click hands the URL to the default browser and the file to macOS.
  point = await pointOf(page, paneId, `${origin}/a.html`, { offset: 4 });
  await hoverLink(page, paneId, point);
  await metaClick(page, point);
  point = await pointOf(page, paneId, "src/linked.ts:42", { offset: 2 });
  await hoverLink(page, paneId, point);
  await metaClick(page, point);
  // A path outside every checkout goes to macOS on a plain click, and a program there is only revealed.
  point = await pointOf(page, paneId, outside, { offset: 2 });
  await hoverLink(page, paneId, point);
  await page.mouse.click(point.x, point.y);
  point = await pointOf(page, paneId, program, { offset: 2 });
  await hoverLink(page, paneId, point);
  await page.mouse.click(point.x, point.y);
  await expect.poll(opened).toEqual([
    { how: "external", target: `${origin}/a.html` },
    { how: "open", target: path.join(checkout, "src", "linked.ts") },
    { how: "open", target: fs.realpathSync(outside) },
    { how: "reveal", target: fs.realpathSync(program) },
  ]);

  // A program's own OSC 8 link opens without xterm's confirm dialog.
  point = await pointOf(page, paneId, "Page B link", { offset: 1 });
  await hoverLink(page, paneId, point);
  await page.mouse.click(point.x, point.y);
  await expect(viewTab(page, "Page B")).toBeVisible({ timeout: 20_000 });
  expect(dialogs).toEqual([]);
  // A program's file link opens the file at the line its address names.
  point = await pointOf(page, paneId, "Linked at 7", { offset: 1 });
  await hoverLink(page, paneId, point);
  await page.mouse.click(point.x, point.y);
  await expect(page.locator(".cm-activeLine").first()).toHaveText("// line 7", { timeout: 20_000 });
  expect(sent.get("reveal_path")).toBe(2);

  // A folder is revealed in the Explorer.
  point = await pointOf(page, paneId, "./src", { offset: 1 });
  await hoverLink(page, paneId, point);
  await page.mouse.click(point.x, point.y);
  await expect(page.locator(`[data-explorer-row="${path.join(checkout, "src")}"][data-selected="true"]`)).toBeVisible({ timeout: 20_000 });
  expect(sent.get("reveal_path")).toBe(3);

  // A path the terminal wrapped is one link from its second row.
  const head = await cellOf(page, paneId, "deep/segment-0");
  point = await cellPoint(page, paneId, { row: head.row + 1, column: 1 });
  await hoverLink(page, paneId, point);
  await page.mouse.click(point.x, point.y);
  await expect(viewTab(page, "wrapped.md")).toBeVisible({ timeout: 20_000 });
  expect(sent.get("terminal_click") ?? 0).toBe(clicksBefore);
  await screenshot(page, "terminal-link-wrapped");
  expect(sent.get("reveal_path")).toBe(4);

  // A drag across a link selects its text and opens nothing.
  const from = await pointOf(page, paneId, "src/linked.ts:42", { offset: 1 });
  const to = await pointOf(page, paneId, "src/linked.ts:42", { offset: 12 });
  await hoverLink(page, paneId, from);
  await page.mouse.down();
  await page.mouse.move(to.x, to.y, { steps: 5 });
  await page.mouse.up();
  await page.waitForTimeout(500);
  expect(sent.get("reveal_path")).toBe(4);
  expect(await opened()).toHaveLength(4);
  expect(sent.get("terminal_click") ?? 0).toBe(clicksBefore);

  // A click on plain text is still the program's, so the counts above heard the wire.
  point = await pointOf(page, paneId, "end-of-links", { offset: 1 });
  await page.mouse.move(point.x, point.y);
  await page.mouse.click(point.x, point.y);
  await expect.poll(() => sent.get("terminal_click") ?? 0).toBe(clicksBefore + 1);
  expect(hostLog(run.env).filter((line) => line.event.startsWith("probe_paths.refused"))).toEqual([]);
});
