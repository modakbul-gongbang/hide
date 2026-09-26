// The sidebar's Projects tab on an isolated pinned Herdr and hided: a Git
// project with an agent in its primary checkout and one in a worktree, and a
// plain folder with one agent. The primary checkout leads its project, and
// the rows name their kind and last-commit age;
// a checkout's agent rows start closed, where its second line names them, and
// open on its chevron; a plain folder is one row that opens its checkout; a
// project folds its checkouts; both folds are the core's ui state, so they
// survive a reload. Light and Dark captures land in HIDE_E2E_SCREENSHOT_DIR.

import { expect, test, type Page } from "@playwright/test";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, keyboardFocus, rest, rowGeometry, screenshot } from "./wire";

test.describe.configure({ timeout: 180_000 });

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

/** A Herdr workspace at `cwd`, with a fake `claude` agent titled `task` unless it is null. */
async function workspaceAt(herdr: HerdrFixture, cwd: string, task: string | null): Promise<string> {
  const created = herdr.run(["workspace", "create", "--cwd", cwd, "--label", path.basename(cwd), "--env", `PATH=${herdr.fixturePath}`, "--no-focus"]) as {
    result: { root_pane: { pane_id: string } };
  };
  const pane = created.result.root_pane.pane_id;
  await prompt(herdr, pane);
  if (task) {
    herdr.run(["agent", "start", `agent-${path.basename(cwd)}`, "--kind", "claude", "--pane", pane]);
    execFileSync(herdr.bin, ["pane", "report-metadata", pane, "--source", "e2e", "--token", `task=${task}`], { env: herdr.env, timeout: 30_000 });
  }
  return pane;
}

async function open(page: Page, daemon: Daemon): Promise<void> {
  await page.goto(`${daemon.origin}/#token=${daemon.token}`);
  await expect(page.locator("[data-main-screen]").or(page.locator("[data-workspace-screen]"))).toBeVisible({ timeout: 20_000 });
  await page.locator('[data-sidebar-mode="projects"]').click();
}

async function chooseTheme(page: Page, theme: "light" | "dark"): Promise<void> {
  await page.keyboard.press("Alt+Comma");
  await expect(page.locator('[data-settings="true"]')).toBeVisible();
  await page.locator('[data-settings-tab="appearance"]').click();
  await page.locator(`[data-theme-option="${theme}"]`).click();
  await expect(page.locator("html")).toHaveClass(new RegExp(`\\b${theme}\\b`));
  await page.keyboard.press("Escape");
  await expect(page.locator('[data-settings="true"]')).toHaveCount(0);
  await page.waitForTimeout(400);
}

test("the Projects tab: kind, age, agent line, opened checkouts and folded projects", async ({ page }) => {
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
    const worktree = path.join(herdr.root, "repo-rows");
    git(repo, ["worktree", "add", "-b", "feature/sidebar-rows", worktree]);
    git(worktree, ["commit", "--allow-empty", "-m", "rows"]);
    // The branch description is a purpose the core reads from Git itself.
    git(repo, ["config", "branch.feature/sidebar-rows.description", "Projects 탭 행 다시 그리기"]);
    const notes = path.join(herdr.root, "notes");
    fs.mkdirSync(notes);

    const mainPane = await workspaceAt(herdr, repo, "메인 체크아웃 정리");
    const rowsPane = await workspaceAt(herdr, worktree, "사이드바 행 구현");
    const notesPane = await workspaceAt(herdr, notes, "회의록 요약 정리");

    daemon = await startHided(herdr, "projects-sidebar");
    const last = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, last);
    await open(page, daemon);

    const project = page.locator("[data-project]").filter({ has: page.locator("[data-project-row]", { hasText: /^repo/ }) });
    const projectToggle = project.locator("[data-project-toggle]");
    await expect(projectToggle).toHaveAttribute("aria-expanded", "true");
    const primary = project.locator("[data-checkout-row]", { hasText: /^main/ });
    const feature = project.locator("[data-checkout-row]").filter({ has: page.locator(`[data-checkout][aria-label^="feature/sidebar-rows"]`) });
    await expect(primary.locator("[data-checkout]")).toHaveAttribute("data-checkout-kind", "primary");
    await expect(feature.locator("[data-checkout]")).toHaveAttribute("data-checkout-kind", "branch");
    // The primary checkout leads its project although the worktree moved later.
    await expect(project.locator("[data-checkout]").first()).toHaveAttribute("data-checkout-kind", "primary");
    // The worktree's commit was just made: its age is the first minute.
    await expect(feature.locator("[data-checkout-age]")).toHaveText("now");

    // A checkout's agent rows start closed: line two names the one agent,
    // then the checkout's purpose.
    const featureToggle = feature.locator("[data-checkout-toggle]");
    await expect(featureToggle).toHaveAttribute("aria-expanded", "false");
    await expect(feature.locator("[data-checkout-agents-open]")).toHaveCount(0);
    await expect(feature.locator('[data-checkout-agents="1"]')).toBeVisible();
    await expect(feature.locator("[data-purpose]")).toHaveText("Projects 탭 행 다시 그리기");
    await screenshot(page, "projects-sidebar-closed");

    // sidebar-readability B2, B4, B7: the controls sit on the right in slots
    // kept at rest. A folded chevron is always shown, an unfolded one waits for
    // the pointer; the age stays beside the menu; and hover, keyboard focus,
    // an open menu or a selection move neither the name, the age, the row's
    // height nor the row after it.
    const primaryRow = primary.locator("[data-checkout]").locator("xpath=..");
    const primaryParts = [primary.getByText("main", { exact: true }), primary.locator("[data-checkout-age]")];
    const primaryToggle = primary.locator("[data-checkout-toggle]");
    const primaryMenu = primary.locator("button[data-checkout-menu]");
    await rest(page);
    const atRest = await rowGeometry(primaryRow, feature, primaryParts);
    await expect(primaryToggle).toHaveCSS("opacity", "1");
    await expect(primaryMenu).toHaveCSS("opacity", "0");
    await expect(projectToggle).toHaveCSS("opacity", "0");
    const activity = project.locator("[data-project-activity]");
    expect((await projectToggle.boundingBox())!.x).toBeGreaterThan((await activity.boundingBox())!.x);
    await project.locator("[data-project-row]").hover();
    await expect(projectToggle).toHaveCSS("opacity", "1");
    await primaryRow.hover();
    await expect(primaryMenu).toHaveCSS("opacity", "1");
    expect(await rowGeometry(primaryRow, feature, primaryParts)).toEqual(atRest);
    await rest(page);
    await keyboardFocus(page, primary.locator("[data-checkout]"));
    await expect(primaryMenu).toHaveCSS("opacity", "1");
    expect(await rowGeometry(primaryRow, feature, primaryParts)).toEqual(atRest);
    await rest(page);
    await primaryMenu.click();
    await expect(page.getByRole("menu", { name: "main actions" })).toBeVisible();
    await expect(primary.locator("[data-checkout-age]")).toHaveCSS("opacity", "1");
    expect(await rowGeometry(primaryRow, feature, primaryParts)).toEqual(atRest);
    await screenshot(page, "projects-sidebar-menu-open");
    await page.keyboard.press("Escape");
    await expect(page.getByRole("menu", { name: "main actions" })).toHaveCount(0);
    await primary.locator("[data-checkout]").click();
    await expect(primary.locator("[data-checkout]")).toHaveAttribute("aria-current", "true");
    await rest(page);
    expect(await rowGeometry(primaryRow, feature, primaryParts)).toEqual(atRest);

    // The chevron opens the agent rows, which take line two's place.
    await featureToggle.click();
    await expect(featureToggle).toHaveAttribute("aria-expanded", "true");
    await expect(feature.locator(`[data-checkout-agents-open] [data-pane="${rowsPane}"]`)).toBeVisible();
    await expect(feature.locator("[data-checkout-agents]")).toHaveCount(0);
    await screenshot(page, "projects-sidebar-open");

    // The row itself still opens the checkout.
    await feature.locator("[data-checkout]").click();
    await expect(feature.locator("[data-checkout]")).toHaveAttribute("aria-current", "true");

    // A plain folder is one row: no project row or fold of its own, a folder
    // glyph, and its agent named on line two until its chevron opens the row.
    const folder = page.locator("[data-project]", { hasText: /^notes/ });
    await expect(folder.locator("[data-project-row]")).toHaveCount(0);
    await expect(folder.locator("[data-project-toggle]")).toHaveCount(0);
    const folderRow = folder.locator("[data-checkout]");
    await expect(folderRow).toHaveCount(1);
    await expect(folderRow).toHaveAttribute("data-checkout-kind", "folder");
    await expect(folder.locator('[data-checkout-agents="1"]')).toBeVisible();
    const folderToggle = folder.locator("[data-checkout-toggle]");
    await expect(folderToggle).toHaveAttribute("aria-expanded", "false");
    // The row opens the folder's checkout, not an Overview.
    await folderRow.click();
    await expect(folderRow).toHaveAttribute("aria-current", "true");
    await expect(page.locator("[data-workspace-screen]")).toBeVisible();
    await expect(feature.locator("[data-checkout]")).not.toHaveAttribute("aria-current", "true");
    await folderToggle.click();
    await expect(folder.locator(`[data-checkout-agents-open] [data-pane="${notesPane}"]`)).toBeVisible();
    await folderToggle.click();
    await expect(folder.locator("[data-checkout-agents-open]")).toHaveCount(0);

    // sidebar-readability B12, B13, B9: a parent in Projects folds its
    // children with the core's lineage state, the one Agents folds by. The
    // worktree's agent becomes the primary agent's child: folded by default,
    // the parent carries the badge and an always-shown chevron; unfolding is
    // one agent_tree_toggle and nothing else, the child is drawn under its
    // parent, and its own checkout still lists it.
    execFileSync(herdr.bin, ["pane", "report-metadata", rowsPane, "--source", "e2e-lineage", "--token", `parent_pane=${mainPane}`], { env: herdr.env, timeout: 30_000 });
    await primaryToggle.click();
    const parentRow = primary.locator(`[data-checkout-agents-open] [data-pane="${mainPane}"]`);
    await expect(parentRow.locator("[data-descendant-badge]")).toHaveAttribute("data-descendant-badge", "1", { timeout: 20_000 });
    const lineageToggle = parentRow.locator(`[data-agent-tree-toggle="${mainPane}"]`);
    await expect(lineageToggle).toHaveAttribute("aria-expanded", "false");
    await rest(page);
    await expect(lineageToggle).toHaveCSS("opacity", "1");
    await expect(primary.locator(`[data-checkout-agents-open] [data-pane="${rowsPane}"]`)).toHaveCount(0);
    const beforeFold = new Map(sent);
    await lineageToggle.click();
    await expect.poll(() => (sent.get("agent_tree_toggle") ?? 0) - (beforeFold.get("agent_tree_toggle") ?? 0)).toBe(1);
    expect(last.get("agent_tree_toggle")?.pane_id).toBe(mainPane);
    const childUnderParent = primary.locator(`[data-checkout-agents-open] [data-pane="${rowsPane}"]`);
    await expect(childUnderParent).toHaveAttribute("data-depth", "1", { timeout: 15_000 });
    await expect(parentRow.locator("[data-descendant-badge]")).toHaveCount(0);
    await expect(feature.locator(`[data-checkout-agents-open] [data-pane="${rowsPane}"]`)).toHaveAttribute("data-depth", "0");
    await screenshot(page, "projects-sidebar-lineage-open");
    // The same fold in Agents: the child is drawn under its parent there too.
    await page.locator('[data-sidebar-mode="agents"]').click();
    await expect(page.locator(`[data-agent-list] [data-pane="${rowsPane}"]`)).toHaveAttribute("data-depth", "1");
    await expect(page.locator(`[data-agent-list] [data-pane="${mainPane}"] [data-agent-place]`)).toHaveText("repo › main");
    await page.locator('[data-sidebar-mode="projects"]').click();

    // Folding changes nothing but the list (B9): no focus, open, read, start
    // or close event, and the center keeps the Workspace it showed.
    const screenBefore = await page.locator("[data-workspace-screen]").count();
    const quiet = new Map(sent);
    await lineageToggle.click();
    await expect(childUnderParent).toHaveCount(0, { timeout: 15_000 });
    await lineageToggle.click();
    await expect(childUnderParent).toHaveAttribute("data-depth", "1", { timeout: 15_000 });
    await primaryToggle.click();
    await primaryToggle.click();
    await projectToggle.click();
    await projectToggle.click();
    await expect(projectToggle).toHaveAttribute("aria-expanded", "true");
    const moved = [...sent].filter(([kind, count]) => count !== (quiet.get(kind) ?? 0)).map(([kind]) => kind);
    expect(moved.filter((kind) => !["agent_tree_toggle", "ui_state_update", "ui_state_update.usage_hint"].includes(kind))).toEqual([]);
    expect(await page.locator("[data-workspace-screen]").count()).toBe(screenBefore);
    await expect(folderRow).toHaveAttribute("aria-current", "true");

    // Folding the project hides its checkouts; both folds survive a reload.
    await projectToggle.click();
    await expect(projectToggle).toHaveAttribute("aria-expanded", "false");
    await expect(project.locator("[data-checkout]")).toHaveCount(0);
    await open(page, daemon);
    await expect(projectToggle).toHaveAttribute("aria-expanded", "false");
    await expect(project.locator("[data-checkout]")).toHaveCount(0);
    await projectToggle.click();
    await expect(featureToggle).toHaveAttribute("aria-expanded", "true");
    await expect(primaryToggle).toHaveAttribute("aria-expanded", "true");
    // The lineage fold is the core's too: left unfolded above, it is still unfolded.
    await expect(primary.locator(`[data-checkout-agents-open] [data-pane="${mainPane}"] [data-agent-tree-toggle]`)).toHaveAttribute("aria-expanded", "true");
    await expect(primary.locator(`[data-checkout-agents-open] [data-pane="${rowsPane}"]`)).toHaveAttribute("data-depth", "1");

    // B4 in Agents: a quiet row and the row after it stay put through hover,
    // keyboard focus and selection.
    await page.locator('[data-sidebar-mode="agents"]').click();
    const quietRows = page.locator("[data-agent-list] li[data-pane]:not(:has([data-agent-line]))");
    const [agentRow, nextRow] = [quietRows.nth(0), quietRows.nth(1)];
    const agentParts = [agentRow.locator("[data-agent-title]"), agentRow.locator("[data-agent-elapsed]")];
    await rest(page);
    const agentAtRest = await rowGeometry(agentRow, nextRow, agentParts);
    await agentRow.hover();
    expect(await rowGeometry(agentRow, nextRow, agentParts)).toEqual(agentAtRest);
    await keyboardFocus(page, agentRow.locator("[data-agent-open]"));
    expect(await rowGeometry(agentRow, nextRow, agentParts)).toEqual(agentAtRest);
    await agentRow.locator("[data-agent-open]").click();
    await expect(agentRow.locator("[data-agent-open]")).toHaveAttribute("aria-current", "true");
    await rest(page);
    expect(await rowGeometry(agentRow, nextRow, agentParts)).toEqual(agentAtRest);
    await page.locator('[data-sidebar-mode="projects"]').click();
    await expect(folderToggle).toHaveAttribute("aria-expanded", "false");

    for (const theme of ["light", "dark"] as const) {
      await chooseTheme(page, theme);
      // The Settings shortcut leaves keyboard focus on the last clicked control; its ring is not part of the rows.
      await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
      await page.locator("[data-project-list]").hover({ position: { x: 1, y: 1 } });
      await screenshot(page, `projects-sidebar-${theme}`);
    }
  } finally {
    await daemon?.stop();
    await herdr.stop();
  }
});
