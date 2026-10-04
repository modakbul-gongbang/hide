import { expect, test, type Locator, type Page } from "@playwright/test";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, enterWorkspace, screenshot } from "./wire";

// PRD tab-view-bookmark: two Agent tabs of one checkout share File Views'
// View list, and each remembers which View was in front for it.

test.describe.configure({ timeout: 180_000 });
test.use({ actionTimeout: 15_000 });

function quote(value: string): string {
  return `'${value.replaceAll("'", "'\\''")}'`;
}

/** Runs `hide <args>` in the shell of `pane`, as an agent in that tab would. */
async function hideFrom(herdr: HerdrFixture, daemon: Daemon, pane: string, args: string[], sequence: number): Promise<void> {
  const status = path.join(herdr.root, `cli-${sequence}.status`);
  const error = path.join(herdr.root, `cli-${sequence}.err`);
  const hide = path.resolve("..", "target", "debug", "hide");
  const command = `HIDE_STATE_DIR=${quote(daemon.stateDir)} ${[hide, ...args].map(quote).join(" ")} > /dev/null 2> ${quote(error)}; printf '%s' "$?" > ${quote(status)}\n`;
  const sent = spawnSync(herdr.bin, ["pane", "send-text", pane, command], { env: herdr.env, encoding: "utf8", timeout: 10_000 });
  expect(sent.status, sent.stderr).toBe(0);
  await expect.poll(() => (fs.existsSync(status) ? fs.readFileSync(status, "utf8") : null), { timeout: 20_000 }).toBe("0");
}

async function box(node: Locator) {
  const rect = await node.boundingBox();
  if (!rect) throw new Error("Missing drag target");
  return rect;
}

async function drag(page: Page, tab: Locator, target: { x: number; y: number }) {
  await tab.scrollIntoViewIfNeeded();
  const rect = await box(tab);
  const start = { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 };
  await page.mouse.move(start.x, start.y);
  await page.mouse.down();
  await page.mouse.move(start.x + 12, start.y + 2, { steps: 3 });
  await page.mouse.move(target.x, target.y, { steps: 10 });
  await page.mouse.up();
}

const agentTab = (page: Page, id: string) => page.locator(`[data-agent-tab-bar] [data-tab="${id}"]`);
const viewTabs = (page: Page) => page.locator('[data-view-tab-bar] [role="tab"]');
const front = (page: Page) => page.locator('[data-view-tab-bar] [role="tab"][aria-selected="true"]');

test("an Agent tab gets back the View it had in front, and an agent in another tab opens its View behind the operator's", async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | null = null;
  try {
    const fixture = path.join(herdr.root, "fixture");
    for (const name of ["a.md", "b.md", "c.md"]) fs.writeFileSync(path.join(fixture, name), `${name}\n`);
    const created = herdr.run(["tab", "create", "--workspace", herdr.workspace, "--cwd", fixture, "--label", "second", "--no-focus"]) as {
      result: { tab: { tab_id: string }; root_pane: { pane_id: string } };
    };
    const [first, second] = [herdr.tab, created.result.tab.tab_id];
    const [firstPane, secondPane] = [herdr.panes[0], created.result.root_pane.pane_id];
    daemon = await startHided(herdr, "view-bookmarks");
    const sent = countSent(page);
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    await expect(agentTab(page, second)).toBeVisible();
    // File Views on, so the agents' opens without --reveal are seen; with
    // nothing open it starts on a New tab page (PRD three-column-panel B16).
    await page.locator('[data-column-toggle="views"]').click();
    await expect(page.locator("[data-view-area]")).toBeVisible();

    // Tab 1 is in front and opens a.md: that is its bookmark.
    await hideFrom(herdr, daemon, firstPane, ["file", "open", "a.md"], 1);
    await expect(front(page)).toHaveAttribute("aria-label", /\/a\.md/);
    await closeNewTabPage(page);

    // Tab 2 has no bookmark, so File Views stays; its own agent opens b.md.
    await agentTab(page, second).click();
    await expect(agentTab(page, second)).toHaveAttribute("aria-selected", "true");
    await expect(front(page)).toHaveAttribute("aria-label", /\/a\.md/);
    await hideFrom(herdr, daemon, secondPane, ["file", "open", "b.md"], 2);
    await expect(front(page)).toHaveAttribute("aria-label", /\/b\.md/);
    await expect(viewTabs(page)).toHaveCount(2);

    // B1, B2: back and forth, one tab click each, and the page sends no
    // View action of its own after the switch.
    const viewActions = sent.get("view_layout") ?? 0;
    await agentTab(page, first).click();
    await expect(front(page)).toHaveAttribute("aria-label", /\/a\.md/);
    await agentTab(page, second).click();
    await expect(front(page)).toHaveAttribute("aria-label", /\/b\.md/);
    await expect(viewTabs(page)).toHaveCount(2);
    expect(sent.get("view_layout") ?? 0).toBe(viewActions);
    await screenshot(page, "view-bookmarks-tab-2-front");
    await agentTab(page, first).click();
    await expect(front(page)).toHaveAttribute("aria-label", /\/a\.md/);
    await screenshot(page, "view-bookmarks-tab-1-front");

    // B12: while the operator is on tab 2, tab 1's agent opens c.md. The
    // screen stays on b.md, c.md joins the strip, and tab 1 shows it later.
    await agentTab(page, second).click();
    await expect(front(page)).toHaveAttribute("aria-label", /\/b\.md/);
    await hideFrom(herdr, daemon, firstPane, ["file", "open", "c.md"], 3);
    await expect(viewTabs(page)).toHaveCount(3);
    await expect(front(page)).toHaveAttribute("aria-label", /\/b\.md/);
    await screenshot(page, "view-bookmarks-agent-open-behind");
    await agentTab(page, first).click();
    await expect(front(page)).toHaveAttribute("aria-label", /\/c\.md/);
    await agentTab(page, second).click();
    await expect(front(page)).toHaveAttribute("aria-label", /\/b\.md/);

    // B18: the bookmarks are in the file and survive a daemon restart.
    await expect.poll(() => fs.readFileSync(path.join(daemon!.stateDir, "workspace-views.json"), "utf8")).toContain("view_bookmarks");
    daemon = await daemon.restart();
    await page.goto("about:blank");
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await expect(agentTab(page, second)).toHaveAttribute("aria-selected", "true");
    await agentTab(page, first).click();
    await expect(front(page)).toHaveAttribute("aria-label", /\/c\.md/);
    await agentTab(page, second).click();
    await expect(front(page)).toHaveAttribute("aria-label", /\/b\.md/);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});

test("moving focus between two Agent tabs shown side by side leaves File Views alone", async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | null = null;
  try {
    const fixture = path.join(herdr.root, "fixture");
    for (const name of ["a.md", "b.md", "c.md"]) fs.writeFileSync(path.join(fixture, name), `${name}\n`);
    const created = herdr.run(["tab", "create", "--workspace", herdr.workspace, "--cwd", fixture, "--label", "second", "--no-focus"]) as {
      result: { tab: { tab_id: string }; root_pane: { pane_id: string } };
    };
    const [first, second] = [herdr.tab, created.result.tab.tab_id];
    const [firstPane, secondPane] = [herdr.panes[0], created.result.root_pane.pane_id];
    daemon = await startHided(herdr, "view-bookmarks-side-by-side");
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    await expect(agentTab(page, second)).toBeVisible();
    await page.locator('[data-column-toggle="views"]').click();
    await expect(page.locator("[data-view-area]")).toBeVisible();

    await hideFrom(herdr, daemon, firstPane, ["file", "open", "a.md"], 1);
    await expect(front(page)).toHaveAttribute("aria-label", /\/a\.md/);
    await closeNewTabPage(page);
    await agentTab(page, second).click();
    await hideFrom(herdr, daemon, secondPane, ["file", "open", "b.md"], 2);
    await expect(front(page)).toHaveAttribute("aria-label", /\/b\.md/);
    await agentTab(page, first).click();
    await expect(front(page)).toHaveAttribute("aria-label", /\/a\.md/);

    // Tab 2 goes to its own Agent area, right of tab 1: both are shown, and
    // it is the one that was not shown before, so its bookmark comes back.
    const body = await box(page.locator("[data-agent-body]").first());
    await drag(page, agentTab(page, second), { x: body.x + body.width * 0.92, y: body.y + body.height / 2 });
    await expect(page.locator("[data-agent-area-id]")).toHaveCount(2);
    await expect(front(page)).toHaveAttribute("aria-label", /\/b\.md/);

    // B7: focus moves to the other area's tab, by its strip and by a pane;
    // both tabs were shown already, so File Views does not swap.
    await agentTab(page, first).click();
    await expect(front(page)).toHaveAttribute("aria-label", /\/b\.md/);
    await page.locator(`[data-pane-view="${secondPane}"]`).click();
    await expect(front(page)).toHaveAttribute("aria-label", /\/b\.md/);
    await page.locator(`[data-pane-view="${firstPane}"]`).click();
    await expect(front(page)).toHaveAttribute("aria-label", /\/b\.md/);
    await expect(viewTabs(page)).toHaveCount(2);
    await screenshot(page, "view-bookmarks-side-by-side");

    // What is picked in File Views next belongs to the tab that has focus:
    // choosing a.md now, then focusing the other area, leaves it in front.
    await page.locator('[data-view-tab-bar] [role="tab"][aria-label*="/a.md"]').click();
    await expect(front(page)).toHaveAttribute("aria-label", /\/a\.md/);
    await page.locator(`[data-pane-view="${secondPane}"]`).click();
    await expect(front(page)).toHaveAttribute("aria-label", /\/a\.md/);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});

/** Closes the New tab page File Views starts on when it is turned on with nothing open. */
async function closeNewTabPage(page: Page): Promise<void> {
  const blank = page.locator('[data-view-tab-bar] [role="tab"]').filter({ hasNotText: /\.md/ });
  await expect(blank).toHaveCount(1);
  await blank.hover();
  await blank.getByRole("button", { name: /Close view/ }).click();
  await expect(blank).toHaveCount(0);
}
