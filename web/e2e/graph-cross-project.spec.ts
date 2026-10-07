// A delegation into another project in the Agents graph (issue 718), on an
// isolated pinned Herdr and hided: the fixture project's lead spawned two
// agents in `sasu`, a resting one on its main checkout and a working one in a
// worktree, and one in `docs`, which rests. A project's graph is its own, so no line joins them;
// the parent's row carries `→ sasu 2` and `→ docs`, each child's row `←
// fixture`, and a chip goes to the box at the other end: on All projects it
// selects that box, opening its fold and clearing a filter that hides it, and
// on one project's Overview it opens the other project's Overview with that
// box selected. Light and Dark captures land in HIDE_E2E_SCREENSHOT_DIR.

import { expect, test, type Locator, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { setFixtureLifecycle, spawnAgent, startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { openCurrentProjectOverview } from "./overview-entry";
import { chooseTheme, screenshot } from "./wire";

test.describe.configure({ timeout: 180_000 });

function git(cwd: string, args: string[]): void {
  execFileSync("git", ["-c", "commit.gpgsign=false", "-c", "user.name=e2e", "-c", "user.email=e2e@example.invalid", "-c", "init.defaultBranch=main", ...args], { cwd, stdio: "ignore" });
}

/** Every chip's project name in `row` is drawn whole: not cut by an ellipsis and not squeezed to nothing. */
async function expectNamesWhole(row: Locator): Promise<void> {
  const names = row.locator("[data-graph-cross-name]");
  await expect(names).not.toHaveCount(0);
  for (const name of await names.all()) {
    const size = await name.evaluate((element) => ({ text: element.textContent, client: element.clientWidth, scroll: element.scrollWidth }));
    expect(size.client, `chip name ${size.text} is drawn`).toBeGreaterThan(0);
    expect(size.scroll, `chip name ${size.text} is not cut`).toBeLessThanOrEqual(size.client);
  }
}

type Stack = { herdr: HerdrFixture; daemon: Daemon; lead: string; spec: string; build: string; docs: string };

/** The fixture project's lead with its children in `sasu` (main and a worktree) and in `docs`, and the page open on hided. */
async function start(page: Page, name: string): Promise<Stack> {
  await page.setViewportSize({ width: 1440, height: 900 });
  const herdr = await startHerdr();
  try {
    const lead = herdr.panes[0];
    const sasu = path.join(herdr.root, "sasu");
    const worktree = path.join(herdr.root, "sasu-wt");
    fs.mkdirSync(sasu);
    git(sasu, ["init"]);
    git(sasu, ["commit", "--allow-empty", "-m", "initial"]);
    git(sasu, ["worktree", "add", "-b", "prd/runner", worktree]);
    git(worktree, ["commit", "--allow-empty", "-m", "runner work"]);
    const spec = await spawnAgent(herdr, "sasu", lead, sasu, { task: "sasu 명세 정리하기" });
    const build = await spawnAgent(herdr, "sasu-wt", lead, worktree, { task: "러너 재시도 구현하기" });
    const docs = await spawnAgent(herdr, "docs", lead, undefined, { task: "문서 링크 고치기" });
    await setFixtureLifecycle(herdr, lead, "working");
    await setFixtureLifecycle(herdr, build, "working");
    const daemon = await startHided(herdr, name);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await expect(page.locator("[data-main-screen]").or(page.locator("[data-workspace-screen]"))).toBeVisible({ timeout: 20_000 });
    return { herdr, daemon, lead, spec, build, docs };
  } catch (error) {
    herdr.stop();
    throw error;
  }
}

test("All projects: a delegation into another project is a chip at each end, and a chip selects the box at the other end", async ({ page }) => {
  const stack = await start(page, "graph-cross-all");
  try {
    const { lead, spec, build, docs } = stack;
    await page.locator("[data-home-destination]").click();
    const main = page.locator("[data-main-screen]");
    await page.locator('[data-main-tab="agents"]').click();
    await expect(main.locator("[data-graph=all]")).toBeVisible();
    const row = (pane: string) => main.locator(`[data-graph-row="${pane}"]`);
    const out = row(lead).locator('[data-graph-cross="out"]');

    // One chip per project, counted, the agents named in its accessible name and tooltip, the working one first.
    await expect(out).toHaveText(["sasu2", "docs"], { timeout: 30_000 });
    await expect(out.first()).toHaveAttribute("aria-label", "Delegated to sasu: 러너 재시도 구현하기, sasu 명세 정리하기", { timeout: 30_000 });
    await expect(row(build).locator('[data-graph-cross="in"]')).toHaveText("fixture");
    // No line crosses projects, and nothing is selected before a chip is used.
    await expect(main.locator("[data-graph-edge]")).toHaveCount(0);
    await expect(main.locator('[data-graph-box][data-selected="true"]')).toHaveCount(0);
    await out.first().hover();
    await expect(page.getByRole("tooltip")).toContainText("Delegated to sasu");
    for (const theme of ["dark", "light"] as const) {
      await chooseTheme(page, theme);
      await page.mouse.move(2, 898);
      await screenshot(page, `graph-cross-project-all-${theme}`);
    }

    // `docs` rests, so its box is folded away: the chip opens its fold and selects it.
    await expect(row(docs)).toHaveCount(0);
    await expect(row(spec)).toHaveCount(0);
    await out.nth(1).click();
    const docsBox = main.locator('[data-graph-box][data-selected="true"]');
    await expect(docsBox).toHaveCount(1);
    await expect(docsBox.locator(`[data-graph-row="${docs}"]`)).toBeVisible();
    await expect(docsBox).toBeInViewport();
    await expect(main.locator('[data-graph-fold="resting"][aria-expanded="true"]')).toHaveCount(1);

    // A search that hides the other end is cleared by the chip, which selects the worktree's box.
    await main.locator("[data-graph-search]").fill("문서 링크");
    await expect(row(build)).toHaveCount(0);
    await row(docs).locator('[data-graph-cross="in"]').click();
    await expect(main.locator("[data-graph-search]")).toHaveValue("");
    await expect(main.locator('[data-graph-box][data-selected="true"]').locator(`[data-graph-row="${lead}"]`)).toBeVisible();
    await out.first().click();
    const selected = main.locator('[data-graph-box][data-selected="true"]');
    await expect(selected.locator(`[data-graph-row="${build}"]`)).toBeVisible();
    await expect(selected).toBeInViewport();
    // Hovered and focused, the row shows its hint where the age was; the title gives way, never a chip's project name.
    await row(lead).locator("[data-graph-open]").hover({ position: { x: 4, y: 4 } });
    await expect(row(lead).locator("[data-graph-row-hint]")).toBeVisible();
    await expectNamesWhole(row(lead));
    await screenshot(page, "graph-cross-project-selected-light");
  } finally {
    stack.daemon.stop();
    stack.herdr.stop();
  }
});

test("one project's Overview: a chip opens the other project's Overview with the box at the other end selected", async ({ page }) => {
  const stack = await start(page, "graph-cross-project");
  try {
    const { lead, build } = stack;
    // The fixture project's sidebar row opens its Workspace, whose Overview is one tab away.
    await page.getByRole("button", { name: /^fixture,/ }).click();
    await openCurrentProjectOverview(page, "fixture");
    const overview = page.locator("[data-overview-screen]");
    const out = overview.locator(`[data-graph-row="${lead}"] [data-graph-cross="out"]`);
    await expect(out).toHaveText(["sasu2", "docs"], { timeout: 30_000 });
    const sasu = await out.first().getAttribute("data-graph-cross-project");
    await out.first().click();
    await expect(overview).toHaveAttribute("data-overview-screen", sasu!);
    const selected = overview.locator('[data-graph-box][data-selected="true"]');
    await expect(selected.locator(`[data-graph-row="${build}"]`)).toBeVisible();
    await expect(selected.locator(`[data-graph-row="${build}"] [data-graph-cross="in"]`)).toHaveText("fixture");
    await page.mouse.move(2, 898);
    await screenshot(page, "graph-cross-project-opened");
  } finally {
    stack.daemon.stop();
    stack.herdr.stop();
  }
});
