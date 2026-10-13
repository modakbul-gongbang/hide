import { expect, test, type Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { fixtureExecutable } from "./platform-fixture";
import { runInPane, startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { enterWorkspace, screenshot } from "./wire";

const agentTab = (page: Page, id: string) => page.locator(`[data-agent-tab-bar] [data-tab="${id}"]`);
const fileViews = (page: Page) => page.locator('[data-column="views"]');
const toggle = (page: Page) => page.locator('[data-column-toggle="views"]');
const front = (page: Page) => page.locator('[data-view-tab-bar] [role="tab"][aria-selected="true"]');

async function openFile(herdr: HerdrFixture, daemon: Daemon, pane: string, name: string): Promise<void> {
  const hide = path.resolve("..", "target", "debug", fixtureExecutable("hide"));
  const ran = await runInPane(herdr, pane, name, {
    env: { HIDE_STATE_DIR: daemon.stateDir }, argv: [hide, "file", "open", name, "--reveal"],
  });
  expect(ran.status, ran.stderr).toBe(0);
}

function savedVisibility(daemon: Daemon, tab: string): boolean | undefined {
  const stored = JSON.parse(fs.readFileSync(path.join(daemon.stateDir, "workspace-views.json"), "utf8")) as {
    workspaces: { view_visibility?: Record<string, boolean> }[];
  };
  return stored.workspaces.find((workspace) => workspace.view_visibility?.[tab] !== undefined)?.view_visibility?.[tab];
}

test("Agent tabs keep File Views on or off through switching and restart @platform", async ({ page }) => {
  test.setTimeout(180_000);
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | null = null;
  try {
    const fixture = path.join(herdr.root, "fixture");
    for (const name of ["a.md", "b.md", "c.md"]) fs.writeFileSync(path.join(fixture, name), `${name}\n`);
    const created = herdr.run(["tab", "create", "--workspace", herdr.workspace, "--cwd", fixture, "--label", "second", "--no-focus"]) as {
      result: { tab: { tab_id: string }; root_pane: { pane_id: string } };
    };
    const [first, second] = [herdr.tab, created.result.tab.tab_id];
    daemon = await startHided(herdr, "file-view-visibility");
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    await expect(agentTab(page, second)).toBeVisible();

    await openFile(herdr, daemon, herdr.panes[0], "a.md");
    await expect(front(page)).toHaveAttribute("aria-label", /\/a\.md/);
    await agentTab(page, second).click();
    await openFile(herdr, daemon, created.result.root_pane.pane_id, "b.md");
    await expect(front(page)).toHaveAttribute("aria-label", /\/b\.md/);
    await toggle(page).click();
    await expect(toggle(page)).toHaveAttribute("aria-pressed", "false");
    await expect(fileViews(page)).toBeHidden();

    await agentTab(page, first).click();
    await expect(toggle(page)).toHaveAttribute("aria-pressed", "true");
    await expect(front(page)).toHaveAttribute("aria-label", /\/a\.md/);
    await expect(page.locator('[data-view-tab-bar] [role="tab"]')).toHaveCount(2);
    await screenshot(page, "file-view-visibility-first-on");
    await agentTab(page, second).click();
    await expect(toggle(page)).toHaveAttribute("aria-pressed", "false");
    await expect(fileViews(page)).toBeHidden();
    await screenshot(page, "file-view-visibility-second-off");

    await expect.poll(() => savedVisibility(daemon!, first)).toBe(true);
    await expect.poll(() => savedVisibility(daemon!, second)).toBe(false);
    daemon = await daemon.restart();
    await page.goto("about:blank");
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await expect(agentTab(page, second)).toHaveAttribute("aria-selected", "true");
    await expect(toggle(page)).toHaveAttribute("aria-pressed", "false");
    await agentTab(page, first).click();
    await expect(toggle(page)).toHaveAttribute("aria-pressed", "true");
    await expect(front(page)).toHaveAttribute("aria-label", /\/a\.md/);
    await agentTab(page, second).click();
    await expect(toggle(page)).toHaveAttribute("aria-pressed", "false");

    // An explicit file open makes the hidden tab visible and remembers it.
    await openFile(herdr, daemon, created.result.root_pane.pane_id, "c.md");
    await expect(toggle(page)).toHaveAttribute("aria-pressed", "true");
    await expect(front(page)).toHaveAttribute("aria-label", /\/c\.md/);
    await agentTab(page, first).click();
    await agentTab(page, second).click();
    await expect(toggle(page)).toHaveAttribute("aria-pressed", "true");
    await expect(front(page)).toHaveAttribute("aria-label", /\/c\.md/);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
