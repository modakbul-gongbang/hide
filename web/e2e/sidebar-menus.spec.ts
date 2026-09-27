// The sidebar's row menus (PRD sidebar-context-menus D-11) on an isolated
// pinned Herdr and hided: a Git project Herdr shows without a registration,
// with an agent in its main checkout and one in a linked worktree. The
// project row's menu lists the board's items and Pin registers the row and
// pins it; New tab in main opens a tab in the default checkout and brings it
// to the front; Set as default checkout moves the home glyph and the first
// place to the worktree; the agent row's Copy session id puts Herdr's
// session id on the clipboard, and its Close tab… closes that agent's tab.
// Light and Dark captures of each menu land in HIDE_E2E_SCREENSHOT_DIR.

import { expect, test, type Locator, type Page } from "@playwright/test";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { rest, screenshot } from "./wire";

test.describe.configure({ timeout: 180_000 });

const BRANCH = "feature/menus";
const SESSION = "5b0c7e2a-menu-e2e-session";

function git(cwd: string, args: string[]): void {
  execFileSync("git", ["-c", "user.name=e2e", "-c", "user.email=e2e@example.invalid", "-c", "init.defaultBranch=main", ...args], { cwd, stdio: "ignore" });
}

async function prompt(herdr: HerdrFixture, pane: string): Promise<void> {
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    const read = spawnSync(herdr.bin, ["pane", "read", pane, "--source", "visible", "--format", "text"], { env: herdr.env, encoding: "utf8", timeout: 10_000 });
    if (read.status === 0 && read.stdout.includes("fixture %")) return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`no prompt in pane ${pane}`);
}

/** A Herdr workspace at `cwd` with a fake `claude` agent titled `task`; returns its pane. */
async function workspaceAt(herdr: HerdrFixture, cwd: string, task: string): Promise<string> {
  const created = herdr.run(["workspace", "create", "--cwd", cwd, "--label", path.basename(cwd), "--env", `PATH=${herdr.fixturePath}`, "--no-focus"]) as {
    result: { root_pane: { pane_id: string } };
  };
  const pane = created.result.root_pane.pane_id;
  await prompt(herdr, pane);
  herdr.run(["agent", "start", `agent-${path.basename(cwd)}`, "--kind", "claude", "--pane", pane]);
  execFileSync(herdr.bin, ["pane", "report-metadata", pane, "--source", "e2e", "--token", `task=${task}`], { env: herdr.env, timeout: 30_000 });
  return pane;
}

/** The open menu as it is drawn: a line per item (its label and chord), "─" per separator, "(disabled)" after a disabled item. */
async function menuLines(menu: Locator): Promise<string[]> {
  return menu.evaluate((element) =>
    Array.from(element.children).map((child) => {
      if (child.getAttribute("role") === "separator") return "─";
      const label = child.querySelector("span > span")?.textContent ?? "";
      const chord = child.querySelector("[data-menu-shortcut]")?.textContent;
      return [label, chord, child.hasAttribute("data-disabled") ? "(disabled)" : null].filter(Boolean).join(" ");
    }),
  );
}

async function openMenu(page: Page, row: Locator, name: string): Promise<Locator> {
  await row.click({ button: "right" });
  const menu = page.getByRole("menu", { name });
  await expect(menu).toBeVisible();
  return menu;
}

async function chooseTheme(page: Page, theme: "light" | "dark"): Promise<void> {
  await page.keyboard.press("Alt+Comma");
  await expect(page.locator('[data-settings="true"]')).toBeVisible();
  await page.locator('[data-settings-tab="appearance"]').click();
  await page.locator(`[data-theme-option="${theme}"]`).click();
  await expect(page.locator("html")).toHaveClass(new RegExp(`\\b${theme}\\b`));
  await page.keyboard.press("Escape");
  await expect(page.locator('[data-settings="true"]')).toHaveCount(0);
}

test("the sidebar's row menus: pin an unregistered project, open a tab, move the default, copy a session id", async ({ page }) => {
  await page.setViewportSize({ width: 1400, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const repo = path.join(herdr.root, "repo");
    fs.mkdirSync(repo);
    git(repo, ["init"]);
    fs.writeFileSync(path.join(repo, "README.md"), "# repo\n");
    git(repo, ["add", "README.md"]);
    git(repo, ["commit", "-m", "initial"]);
    const worktree = path.join(herdr.root, "repo-menus");
    git(repo, ["worktree", "add", "-b", BRANCH, worktree]);
    await workspaceAt(herdr, repo, "메인 정리");
    const worktreePane = await workspaceAt(herdr, worktree, "메뉴 구현");
    execFileSync(herdr.bin, ["pane", "report-agent-session", worktreePane, "--source", "herdr:claude", "--agent", "claude", "--agent-session-id", SESSION], {
      env: herdr.env,
      timeout: 30_000,
    });

    daemon = await startHided(herdr, "sidebar-menus");
    await page.context().grantPermissions(["clipboard-read", "clipboard-write"], { origin: daemon.origin });
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await expect(page.locator("[data-main-screen]")).toBeVisible({ timeout: 20_000 });
    await page.locator('[data-sidebar-mode="projects"]').click();

    const project = page.locator("[data-project]").filter({ has: page.locator("[data-project-row]", { hasText: /^repo/ }) });
    const projectRow = project.locator("[data-project-menu]").first();
    const main = project.locator("[data-checkout-row]").filter({ has: page.locator('[data-checkout][aria-label^="main"]') });
    const feature = project.locator("[data-checkout-row]").filter({ has: page.locator(`[data-checkout][aria-label^="${BRANCH}"]`) });
    await expect(feature).toBeVisible({ timeout: 30_000 });
    await expect(page.locator('[data-section="Pinned"]')).toHaveCount(0);

    // B1, B3: the project row's menu, in the board's order. A browser tab has
    // no Finder, so Reveal in Finder is absent; a row Herdr shows without a
    // registration still offers Pin and Remove project….
    let menu = await openMenu(page, projectRow, "repo actions");
    expect(await menuLines(menu)).toEqual(["Open Overview", "New worktree…", "New tab in main ⌥T", "─", "Copy path", "─", "Pin", "Remove project…"]);
    await screenshot(page, "sidebar-menus-project-dark");
    await menu.locator('[data-menu-item="pin"]').click();
    await expect(menu).toHaveCount(0);

    // Pin registered the row and pinned it: it now stands under Pinned, once.
    await expect(page.locator('[data-section="Pinned"]')).toHaveText(/Pinned · 1/);
    await expect(page.locator("[data-project-row]", { hasText: /^repo/ })).toHaveCount(1);
    menu = await openMenu(page, projectRow, "repo actions");
    await expect(menu.locator('[data-menu-item="unpin"]')).toHaveText(/Unpin/);
    await page.keyboard.press("Escape");

    // B4: the checkout row's menu; the worktree is not the default yet.
    menu = await openMenu(page, feature.locator("[data-checkout-menu]"), `${BRANCH} actions`);
    expect(await menuLines(menu)).toEqual([
      "Open",
      "New tab here ⌥T",
      "─",
      "Set purpose…",
      "Set as default checkout",
      "Copy branch name",
      "Copy path",
      "─",
      // The worktree's gate refuses while its pane is open; the reason is drawn under the item.
      "Delete worktree… (disabled)",
    ]);
    await screenshot(page, "sidebar-menus-checkout-dark");
    await page.keyboard.press("Escape");
    menu = await openMenu(page, main.locator("[data-checkout-menu]"), "main actions");
    await expect(menu.locator('[data-menu-item="set_primary"]')).toHaveAttribute("data-disabled", "");
    await expect(menu.locator('[data-menu-item="set_primary"]')).toContainText("Already the default checkout.");
    await page.keyboard.press("Escape");

    // B2: New tab in main opens a tab in the default checkout and brings it
    // to the front.
    menu = await openMenu(page, projectRow, "repo actions");
    await menu.locator('[data-menu-item="new_tab_primary"]').click();
    await expect(page.locator("[data-workspace-screen]")).toBeVisible({ timeout: 20_000 });
    const mainTabs = page.locator(`[data-tab-bar] [data-tab-kind="herdr"]`);
    await expect(mainTabs).toHaveCount(2, { timeout: 20_000 });
    await expect(main.locator("[data-checkout]")).toHaveAttribute("aria-current", "true");

    // B6: Set as default checkout moves the home glyph and the first place.
    await expect(main.locator("[data-checkout]")).toHaveAttribute("data-checkout-kind", "primary");
    menu = await openMenu(page, feature.locator("[data-checkout-menu]"), `${BRANCH} actions`);
    await menu.locator('[data-menu-item="set_primary"]').click();
    await expect(feature.locator("[data-checkout]")).toHaveAttribute("data-checkout-kind", "primary");
    await expect(main.locator("[data-checkout]")).toHaveAttribute("data-checkout-kind", "branch");
    await expect(project.locator("[data-checkout-row]").first()).toHaveAttribute("data-checkout-row", await feature.getAttribute("data-checkout-row") ?? "");

    // B7, B8: the agent row's menu; Copy session id puts Herdr's session id on
    // the clipboard and Copy title the row's title.
    await page.locator('[data-sidebar-mode="agents"]').click();
    const agentRow = page.locator(`[data-agent-list] li[data-pane="${worktreePane}"]`);
    await expect(agentRow).toBeVisible();
    menu = await openMenu(page, agentRow, "메뉴 구현 actions");
    expect(await menuLines(menu)).toEqual(["Show", "─", "Copy title", "Copy session id", "─", "Close tab…"]);
    await screenshot(page, "sidebar-menus-agent-dark");
    await menu.locator('[data-menu-item="copy_session_id"]').click();
    await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe(SESSION);
    menu = await openMenu(page, agentRow, "메뉴 구현 actions");
    await menu.locator('[data-menu-item="copy_title"]').click();
    await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe("메뉴 구현");

    // The three menus in Light too.
    await chooseTheme(page, "light");
    await rest(page);
    menu = await openMenu(page, agentRow, "메뉴 구현 actions");
    await screenshot(page, "sidebar-menus-agent-light");
    await page.keyboard.press("Escape");
    await page.locator('[data-sidebar-mode="projects"]').click();
    menu = await openMenu(page, projectRow, "repo actions");
    await screenshot(page, "sidebar-menus-project-light");
    await page.keyboard.press("Escape");
    menu = await openMenu(page, main.locator("[data-checkout-menu]"), "main actions");
    await screenshot(page, "sidebar-menus-checkout-light");
    await page.keyboard.press("Escape");

    // B8: Close tab… closes the tab that holds the agent's pane, in a
    // checkout that is not the one in front, through the tab close flow: an
    // idle agent's tab closes without asking (close.ts asks only for working
    // or attention panes), and the row leaves with its pane.
    await page.locator('[data-sidebar-mode="agents"]').click();
    menu = await openMenu(page, agentRow, "메뉴 구현 actions");
    await menu.locator('[data-menu-item="close_tab"]').click();
    await expect(agentRow).toHaveCount(0, { timeout: 20_000 });
    await expect(page.locator('[data-confirm-close="tab"]')).toHaveCount(0);
    await expect(mainTabs).toHaveCount(2);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
