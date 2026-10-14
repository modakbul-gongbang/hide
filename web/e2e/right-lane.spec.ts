import { expect, test, type Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { fixtureExecutable } from "./platform-fixture";
import { runInPane, startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { chord } from "./chords";
import { quietFor } from "./wait";
import { countSent, enterWorkspace, screenshot } from "./wire";

// PRD right-lane: one lane beside or over Agent Views, a rail of four tools,
// one pin, the file tree as a column of the files tool. Each test starts one
// stack and asserts one contract.

const screen = (page: Page) => page.locator("[data-workspace-screen]");
const railTool = (page: Page, tool: string) => page.locator(`[data-lane-tool="${tool}"]`);
const front = (page: Page) => page.locator('[data-view-tab-bar] [role="tab"][aria-selected="true"]');
const pane = (page: Page, id: string) => page.locator(`[data-pane-view="${id}"]`);
const documentArea = (page: Page) => page.locator('[data-lane-frame] [data-view-area="true"]');

type Lane = { page: Page; herdr: HerdrFixture; daemon: Daemon; second: { tab: string; pane: string }; sent: Map<string, number> };

/** A Workspace with two Agent tabs, three files and a page counting what it sends. */
async function startLane(page: Page, name: string): Promise<Lane> {
  const herdr = await startHerdr({ agents: false });
  const fixture = path.join(herdr.root, "fixture");
  fs.mkdirSync(path.join(fixture, "docs"), { recursive: true });
  for (const file of ["a.md", "b.md", "docs/c.md"]) fs.writeFileSync(path.join(fixture, file), `${file}\n`);
  const created = herdr.run(["tab", "create", "--workspace", herdr.workspace, "--cwd", fixture, "--label", "second", "--no-focus"]) as {
    result: { tab: { tab_id: string }; root_pane: { pane_id: string } };
  };
  const daemon = await startHided(herdr, name);
  const sent = countSent(page);
  await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
  await enterWorkspace(page, "fixture");
  return { page, herdr, daemon, second: { tab: created.result.tab.tab_id, pane: created.result.root_pane.pane_id }, sent };
}

async function openFile(lane: Lane, paneId: string, name: string): Promise<void> {
  const hide = path.resolve("..", "target", "debug", fixtureExecutable("hide"));
  const ran = await runInPane(lane.herdr, paneId, name, { env: { HIDE_STATE_DIR: lane.daemon.stateDir }, argv: [hide, "file", "open", name, "--reveal"] });
  expect(ran.status, ran.stderr).toBe(0);
}

const resizes = (lane: Lane) => lane.sent.get("terminal_resize") ?? 0;

test("an opened file comes up over Agent Views without resizing a terminal, and a terminal click puts it away with its document (B1, B3, B4, B16) @platform", async ({ page }) => {
  test.setTimeout(120_000);
  const lane = await startLane(page, "lane-overlay");
  try {
    await expect(screen(page)).toHaveAttribute("data-lane-mode", "closed");
    await expect(page.locator("[data-lane-tool]")).toHaveCount(4);
    await expect(railTool(page, "files")).toHaveAttribute("aria-pressed", "false");
    await expect(pane(page, lane.herdr.panes[0]!)).toBeVisible();
    await quietFor(page, 500, "the first fit has landed before the lane opens");
    const before = resizes(lane);

    await openFile(lane, lane.herdr.panes[0]!, "a.md");
    await expect(screen(page)).toHaveAttribute("data-lane-mode", "overlay");
    await expect(front(page)).toHaveAttribute("aria-label", /\/a\.md/);
    await expect(railTool(page, "files")).toHaveAttribute("aria-pressed", "true");
    await expect(page.locator("[data-lane-header]")).toContainText("Files");
    await screenshot(page, "right-lane-overlay");

    await pane(page, lane.herdr.panes[0]!).click();
    await expect(screen(page)).toHaveAttribute("data-lane-mode", "hidden");
    await expect(page.locator('[data-lane-badge="1"]')).toBeVisible();
    await railTool(page, "files").click();
    await expect(screen(page)).toHaveAttribute("data-lane-mode", "overlay");
    await expect(front(page)).toHaveAttribute("aria-label", /\/a\.md/);

    // Escape in the lane puts it away; the keyboard goes back to the pane.
    await documentArea(page).locator(".cm-content").click();
    await page.keyboard.press("Escape");
    await expect(screen(page)).toHaveAttribute("data-lane-mode", "hidden");
    await expect(pane(page, lane.herdr.panes[0]!).locator(".xterm-helper-textarea")).toBeFocused();
    await quietFor(page, 500, "no terminal was fitted again while the lane came and went");
    expect(resizes(lane)).toBe(before);

    // The rail's icon on the tool it shows closes the lane.
    await railTool(page, "files").click();
    await expect(screen(page)).toHaveAttribute("data-lane-mode", "overlay");
    await railTool(page, "files").click();
    await expect(screen(page)).toHaveAttribute("data-lane-mode", "closed");
  } finally {
    lane.daemon.stop();
    lane.herdr.stop();
  }
});

test("the pin docks the lane once, keeps it through a terminal click, a tool and an Agent tab, and a restart brings it back docked (B5, B7, B15, B22, B24) @platform", async ({ page }) => {
  test.setTimeout(150_000);
  const lane = await startLane(page, "lane-pin");
  try {
    await openFile(lane, lane.herdr.panes[0]!, "a.md");
    await expect(screen(page)).toHaveAttribute("data-lane-mode", "overlay");
    await quietFor(page, 500, "the overlay has settled");
    const before = resizes(lane);
    const agents = await page.locator("[data-agent-area]").boundingBox();

    await page.locator("[data-lane-pin]").click();
    await expect(screen(page)).toHaveAttribute("data-lane-mode", "docked");
    await expect(page.locator("[data-lane-pin]")).toHaveAttribute("aria-pressed", "true");
    // One fit per attached terminal, the background tab's included.
    const attached = await page.locator("[data-pane-view]").count();
    await expect.poll(() => resizes(lane)).toBe(before + attached);
    await quietFor(page, 500, "the pin fitted each terminal once");
    expect(resizes(lane)).toBe(before + attached);
    const docked = await page.locator("[data-agent-area]").boundingBox();
    expect(docked!.width).toBeLessThan(agents!.width);
    await screenshot(page, "right-lane-docked");

    await pane(page, lane.herdr.panes[0]!).click();
    await expect(screen(page)).toHaveAttribute("data-lane-mode", "docked");
    await railTool(page, "agent_sessions").click();
    await expect(page.locator("[data-lane]")).toHaveAttribute("data-lane", "agent_sessions");
    await expect(screen(page)).toHaveAttribute("data-lane-mode", "docked");

    // Another Agent tab keeps the lane and shows its own views.
    await railTool(page, "files").click();
    const tab = (id: string) => page.locator(`[data-agent-tab-bar] [data-tab="${id}"]`);
    await tab(lane.second.tab).click();
    await openFile(lane, lane.second.pane, "b.md");
    await expect(front(page)).toHaveAttribute("aria-label", /\/b\.md/);
    await tab(lane.herdr.tab).click();
    await expect(screen(page)).toHaveAttribute("data-lane-mode", "docked");
    await expect(front(page)).toHaveAttribute("aria-label", /\/a\.md/);
    await expect(page.locator('[data-view-tab-bar] [role="tab"]')).toHaveCount(1);

    lane.daemon = await lane.daemon.restart();
    await page.goto("about:blank");
    await page.goto(`${lane.daemon.origin}/?probe=1#token=${lane.daemon.token}`);
    await expect(screen(page)).toHaveAttribute("data-lane-mode", "docked");

    // Unpinned, the lane goes back over Agent Views, and a restart starts it put away.
    await page.locator("[data-lane-pin]").click();
    await expect(screen(page)).toHaveAttribute("data-lane-mode", "overlay");
    lane.daemon = await lane.daemon.restart();
    await page.goto("about:blank");
    await page.goto(`${lane.daemon.origin}/?probe=1#token=${lane.daemon.token}`);
    await expect(screen(page)).toHaveAttribute("data-lane-mode", "hidden");
    await expect(railTool(page, "files")).toHaveAttribute("aria-pressed", "false");
  } finally {
    lane.daemon.stop();
    lane.herdr.stop();
  }
});

test("the tree opens beside the document without narrowing it, stays through a tool switch, and fills the lane once the last view closes (B9-B12) @platform", async ({ page }) => {
  test.setTimeout(120_000);
  const lane = await startLane(page, "lane-tree");
  try {
    await openFile(lane, lane.herdr.panes[0]!, "a.md");
    await expect(front(page)).toHaveAttribute("aria-label", /\/a\.md/);
    const width = (await documentArea(page).boundingBox())!.width;
    const toggle = page.locator("[data-lane-tree-toggle]");
    await expect(toggle).toHaveAttribute("aria-pressed", "false");

    await toggle.click();
    await expect(page.locator('[data-lane-tree="beside"]')).toBeVisible();
    await expect(page.locator("[data-lane-tree-toggle]")).toHaveAttribute("aria-pressed", "true");
    expect(Math.abs((await documentArea(page).boundingBox())!.width - width)).toBeLessThanOrEqual(1);
    await screenshot(page, "right-lane-tree");

    await railTool(page, "changes").click();
    await railTool(page, "files").click();
    await expect(page.locator('[data-lane-tree="beside"]')).toBeVisible();

    // The last view closed leaves the lane on the files tool, the tree alone.
    await page.getByRole("button", { name: "Close view a.md" }).click();
    await expect(page.locator('[data-lane-tree="only"]')).toBeVisible();
    await expect(page.locator("[data-lane-tree-hint]")).toBeVisible();
    await expect(screen(page)).toHaveAttribute("data-lane-mode", "overlay");
    await screenshot(page, "right-lane-tree-only");

    // Picking a file opens it beside the tree; the chord turns the tree off.
    await page.locator('[data-explorer-row$="/b.md"]').dblclick();
    await expect(front(page)).toHaveAttribute("aria-label", /\/b\.md/);
    await expect(page.locator('[data-lane-tree="beside"]')).toBeVisible();
    await page.locator('[data-lane-frame] [data-view-area="true"] .cm-content').click();
    await page.keyboard.press(chord("toggle_file_tree"));
    await expect(page.locator("[data-lane-tree]")).toHaveCount(0);
  } finally {
    lane.daemon.stop();
    lane.herdr.stop();
  }
});

test("expand takes the whole body and returns on Escape without resizing a terminal (B8) @platform", async ({ page }) => {
  test.setTimeout(120_000);
  const lane = await startLane(page, "lane-expand");
  try {
    await openFile(lane, lane.herdr.panes[0]!, "a.md");
    await expect(screen(page)).toHaveAttribute("data-lane-mode", "overlay");
    await quietFor(page, 500, "the overlay has settled");
    const before = resizes(lane);
    await page.locator("[data-lane-expand]").click();
    await expect(screen(page)).toHaveAttribute("data-lane-mode", "expanded");
    const row = (await page.locator("[data-lane-row]").boundingBox())!;
    expect(Math.abs((await page.locator("[data-lane-frame]").boundingBox())!.width - row.width)).toBeLessThanOrEqual(1);
    await screenshot(page, "right-lane-expanded");
    await page.locator('[data-lane-frame] [data-view-area="true"] .cm-content').click();
    await page.keyboard.press("Escape");
    await expect(screen(page)).toHaveAttribute("data-lane-mode", "overlay");
    await quietFor(page, 500, "no terminal was fitted again while the lane expanded");
    expect(resizes(lane)).toBe(before);
  } finally {
    lane.daemon.stop();
    lane.herdr.stop();
  }
});

test("the rail's names stay whole in four languages and both themes, and Tab reaches each tool with its pressed state (B23)", async ({ page }) => {
  test.setTimeout(120_000);
  const lane = await startLane(page, "lane-rail");
  try {
    await openFile(lane, lane.herdr.panes[0]!, "a.md");
    await expect(railTool(page, "files")).toHaveAttribute("aria-pressed", "true");
    const rail = page.locator("[data-tool-rail]");
    // Every name sits inside the rail on one line: no ellipsis, no second line.
    const spill = () =>
      rail.evaluate((node) => {
        const edge = node.getBoundingClientRect();
        return [...node.querySelectorAll("[data-lane-tool-name]")].map((name) => {
          const box = name.getBoundingClientRect();
          return Math.max(edge.left + 1 - box.left, box.right - edge.right, box.height > 16 ? box.height : 0, name.scrollWidth - name.clientWidth);
        });
      });
    for (const [language, theme] of [
      ["en", "light"],
      ["ko", "dark"],
      ["zh-CN", "light"],
      ["ja", "dark"],
    ] as const) {
      await page.locator("[data-open-settings]").click();
      await page.locator('[data-settings-tab="general"]').click();
      await page.locator("[data-interface-language]").click();
      await page.locator(`[data-language-option="${language}"]`).click();
      await expect(page.locator("html")).toHaveAttribute("lang", language);
      await page.locator(`[data-theme-option="${theme}"]`).click();
      await page.keyboard.press("Escape");
      await expect(page.locator('[data-settings="true"]')).toHaveCount(0);
      await expect.poll(async () => Math.max(...(await spill()))).toBeLessThanOrEqual(0);
      await screenshot(page, `right-lane-rail-${language}-${theme}`);
    }

    // The keyboard reaches the rail; each tool says whether it shows.
    await railTool(page, "files").focus();
    await page.keyboard.press("Tab");
    await expect(railTool(page, "changes")).toBeFocused();
    await expect(railTool(page, "changes")).toHaveAttribute("aria-pressed", "false");
    await page.keyboard.press("Enter");
    await expect(railTool(page, "changes")).toHaveAttribute("aria-pressed", "true");
    await expect(railTool(page, "files")).toHaveAttribute("aria-pressed", "false");
  } finally {
    lane.daemon.stop();
    lane.herdr.stop();
  }
});
