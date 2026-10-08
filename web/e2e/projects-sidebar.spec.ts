// One sidebar on an isolated Herdr and daemon: new checkouts open by default,
// roots own direct-child badges, and project/checkout folds persist.
// Existing row geometry, context menus and type scaling remain stable.

import { expect, test, type Locator, type Page } from "@playwright/test";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { declareParent, elsewhereTab, finishFixtureTurn, labelAgent, setFixtureLifecycle, startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, keyboardFocus, rest, rowGeometry, screenshot, sidebarColumns, sidebarOverflow, sidebarRowsFit } from "./wire";
import { chord } from "./chords";
import { animationsFinished, quietFor } from "./wait";

test.describe.configure({ timeout: 180_000 });

function git(cwd: string, args: string[]): void {
  execFileSync("git", ["-c", "user.name=e2e", "-c", "user.email=e2e@example.invalid", "-c", "init.defaultBranch=main", "-c", "commit.gpgsign=false", ...args], { cwd, stdio: "ignore" });
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
    labelAgent(herdr, pane, { task });
  }
  return pane;
}

async function open(page: Page, daemon: Daemon): Promise<void> {
  await page.goto(`${daemon.origin}/#token=${daemon.token}`);
  await expect(page.locator("[data-main-screen]").or(page.locator("[data-workspace-screen]"))).toBeVisible({ timeout: 20_000 });
}

/** Whether the project row's Overview button, its click target and focus ring, fills the row's whole height. */
async function projectButtonFills(page: Page): Promise<boolean> {
  return page.locator("[data-project-row]").first().evaluate((button) => {
    const row = button.parentElement!.getBoundingClientRect();
    return Math.abs(button.getBoundingClientRect().height - row.height) < 0.5;
  });
}

/** The font size, in px, of a `text-body` probe placed inside `selector`: what the interface's body text measures there. */
async function probeTextSize(page: Page, selector: string): Promise<number> {
  return page.evaluate((within) => {
    const probe = document.createElement("span");
    probe.className = "text-body";
    probe.textContent = "x";
    document.querySelector(within)!.append(probe);
    const size = Number.parseFloat(getComputedStyle(probe).fontSize);
    probe.remove();
    return size;
  }, selector);
}

/** Every text-holding element's font size in the sidebar's lists, in document order. */
async function sidebarTextSizes(page: Page): Promise<string[]> {
  return page.evaluate(() =>
    Array.from(document.querySelectorAll("nav[data-sidebar] *"))
      .filter((part) => part.getClientRects().length > 0 && Array.from(part.childNodes).some((node) => node.nodeType === Node.TEXT_NODE && node.textContent?.trim()))
      .map((part) => getComputedStyle(part).fontSize),
  );
}

async function chooseTheme(page: Page, theme: "light" | "dark"): Promise<void> {
  await page.keyboard.press(chord("settings"));
  await expect(page.locator('[data-settings="true"]')).toBeVisible();
  await page.locator('[data-settings-tab="general"]').click();
  await page.locator(`[data-theme-option="${theme}"]`).click();
  await expect(page.locator("html")).toHaveClass(new RegExp(`\\b${theme}\\b`));
  await page.keyboard.press("Escape");
  await expect(page.locator('[data-settings="true"]')).toHaveCount(0);
  await animationsFinished(page);
}

test("the sidebar: kind, age, status badges, opened checkouts and folded projects", async ({ page }) => {
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
    const emptyTree = path.join(herdr.root, "repo-empty");
    git(repo, ["worktree", "add", "-b", "empty", emptyTree]);
    await workspaceAt(herdr, emptyTree, null);
    const notes = path.join(herdr.root, "notes");
    fs.mkdirSync(notes);

    const mainPane = await workspaceAt(herdr, repo, "메인 체크아웃 정리");
    const rowsPane = await workspaceAt(herdr, worktree, "사이드바 행 구현");
    const notesPane = await workspaceAt(herdr, notes, "회의록 요약 정리");

    daemon = await startHided(herdr, "projects-sidebar");
    const last = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, last);
    let holdOpen = false;
    const heldOpen: { release: (() => void) | null } = { release: null };
    await page.routeWebSocket(/\/ws$/, (socket) => {
      const server = socket.connectToServer();
      socket.onMessage((message) => {
        const event = typeof message === "string" ? JSON.parse(message) as { kind?: string } : null;
        if (holdOpen && event?.kind === "focus_checkout") {
          heldOpen.release = () => server.send(message);
        } else server.send(message);
      });
    });
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

    // Session UI B5: checkout agents start open; the operator can fold them, and its
    // status badge counts its one agent under the mark that agent's row draws,
    // idle here. Line two is the purpose with the age ending it. The project's
    // badge counts both checkouts' agents.
    const featureToggle = feature.locator("[data-checkout-toggle]");
    await expect(featureToggle).toHaveAttribute("aria-expanded", "true");
    await featureToggle.click();
    await expect(featureToggle).toHaveAttribute("aria-expanded", "false");
    await expect(feature.locator("[data-checkout-agents-open]")).toHaveCount(0);
    await expect(feature.locator('[data-checkout-status] [data-badge-part="idle"]')).toHaveText("1");
    await expect(feature.locator("[data-purpose]")).toHaveText("Projects 탭 행 다시 그리기");
    const lineOf = async (part: Locator) => Math.round((await part.boundingBox())!.y);
    expect(await lineOf(feature.locator("[data-checkout-age]"))).toBeGreaterThan(await lineOf(feature.getByText("feature/sidebar-rows", { exact: true })));
    expect(await lineOf(feature.locator("[data-checkout-age]"))).toBe(await lineOf(feature.locator("[data-purpose]")));
    // PRD sidebar-typography B4: line two is the purpose, here the agent's
    // title standing in for one, with the age ending it. Agents alone earn no
    // line two (projects.test.ts); a checkout with neither is one line (below).
    await expect(primary.locator("[data-purpose]")).toHaveText("메인 체크아웃 정리");
    expect(await lineOf(primary.locator("[data-checkout-age]"))).toBe(await lineOf(primary.locator("[data-purpose]")));
    await expect(project.locator('[data-project-status] [data-badge-part="idle"]')).toHaveText("2");
    await expect(project.locator("[data-project-row]")).toHaveAccessibleName("repo, 2 idle");
    await screenshot(page, "projects-sidebar-closed");

    // sidebar-readability B2, B4, B7: the fold sits on the right in a slot
    // kept at rest. A folded chevron is always shown, an unfolded one waits for
    // the pointer; nothing on the row stands for its menu, which a right-click
    // or ⇧F10 opens; the age stays while it is open; and hover, keyboard
    // focus, an open menu or a selection move neither the name, the age, the
    // row's height nor the row after it.
    const primaryRow = primary.locator("[data-checkout]").locator("xpath=..");
    const primaryParts = [primary.getByText("main", { exact: true }), primary.locator("[data-checkout-age]")];
    const primaryToggle = primary.locator("[data-checkout-toggle]");
    await expect(primaryToggle).toHaveAttribute("aria-expanded", "true");
    const togglesBefore = sent.get("checkout_agents_toggle") ?? 0;
    await primaryToggle.click();
    await expect.poll(() => sent.get("checkout_agents_toggle") ?? 0).toBe(togglesBefore + 1);
    const primaryMenu = page.getByRole("menu", { name: "main actions" });
    const beforeLooking = new Map(sent);
    await rest(page);
    const atRest = await rowGeometry(primaryRow, feature, primaryParts);
    expect(await projectButtonFills(page)).toBe(true);
    await expect(primaryToggle).toHaveCSS("opacity", "1");
    await expect(projectToggle).toHaveCSS("opacity", "0");
    await expect(page.locator("[data-project-list]").getByRole("button", { name: /actions$/ })).toHaveCount(0);
    const status = project.locator("[data-project-status]");
    expect((await projectToggle.boundingBox())!.x).toBeGreaterThan((await status.boundingBox())!.x);
    await project.locator("[data-project-row]").hover();
    await expect(projectToggle).toHaveCSS("opacity", "1");
    await primaryRow.hover();
    expect(await rowGeometry(primaryRow, feature, primaryParts)).toEqual(atRest);
    await rest(page);
    await keyboardFocus(page, primary.locator("[data-checkout]"));
    expect(await rowGeometry(primaryRow, feature, primaryParts)).toEqual(atRest);
    await page.keyboard.press("Shift+F10");
    await expect(primaryMenu).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(primaryMenu).toHaveCount(0);
    await rest(page);
    await primaryRow.click({ button: "right" });
    await expect(primaryMenu).toBeVisible();
    await expect(primary.locator("[data-checkout-age]")).toHaveCSS("opacity", "1");
    expect(await rowGeometry(primaryRow, feature, primaryParts)).toEqual(atRest);
    await screenshot(page, "projects-sidebar-menu-open");
    await page.keyboard.press("Escape");
    await expect(primaryMenu).toHaveCount(0);
    // An unfolded chevron stays shown while its row's menu is open, with the
    // pointer and focus gone from the row.
    await project.locator("[data-project-row]").click({ button: "right" });
    const projectMenu = page.getByRole("menu", { name: /actions$/ });
    await expect(projectMenu).toBeVisible();
    await page.mouse.move(900, 600);
    await expect(projectToggle).toHaveCSS("opacity", "1");
    await page.keyboard.press("Escape");
    await expect(projectMenu).toHaveCount(0);
    // Looking at a row sends nothing: hover, focus and an open menu are the list's own.
    await quietFor(page, 300, "looking at a row sends nothing");
    expect([...sent].filter(([kind, count]) => count !== (beforeLooking.get(kind) ?? 0)).map(([kind]) => kind)).toEqual([]);
    const overview = page.locator("[data-home-destination]");
    await expect(project.locator("[data-project-overview]")).toHaveCount(0);
    await overview.click();
    await expect(page.locator("[data-sidebar-overview]")).toHaveAttribute("aria-current", "page");
    await expect(project.locator("[data-project-row]")).not.toHaveAttribute("aria-current");
    await expect(project.locator("[data-project-row]").locator("xpath=..")).not.toHaveClass(/bg-secondary/);
    const beforeOpen = new Map(sent);
    await primary.locator("[data-checkout]").click();
    await expect(primary.locator("[data-checkout]")).toHaveAttribute("aria-current", "true");
    await expect(primaryToggle).toHaveAttribute("aria-expanded", "true");
    expect((sent.get("focus_checkout") ?? 0) - (beforeOpen.get("focus_checkout") ?? 0)).toBe(1);
    expect(sent.get("ui_state_update") ?? 0).toBe(beforeOpen.get("ui_state_update") ?? 0);
    expect(last.get("focus_checkout")?.expanded).toBe(true);
    await expect(overview).not.toHaveAttribute("aria-current");
    await screenshot(page, "row-click-expanded");
    await primary.locator("[data-checkout]").click();
    await expect(primaryToggle).toHaveAttribute("aria-expanded", "false");
    await expect(primary.locator("[data-checkout]")).toHaveAttribute("aria-current", "true");
    await keyboardFocus(page, primary.locator("[data-checkout]"));
    await page.keyboard.press("Enter");
    await expect(primaryToggle).toHaveAttribute("aria-expanded", "true");
    await page.keyboard.press("Space");
    await expect(primaryToggle).toHaveAttribute("aria-expanded", "false");
    await keyboardFocus(page, overview);
    await page.keyboard.press("Enter");
    await expect(page.locator("[data-sidebar-overview]")).toHaveAttribute("aria-current", "page");
    await expect(primary.locator("[data-checkout]")).not.toHaveAttribute("aria-current");
    await screenshot(page, "overview-row-selected");
    await page.keyboard.press("Space");
    await expect(page.locator("[data-sidebar-overview]")).toHaveAttribute("aria-current", "page");
    // The existing focused checkout cannot land from the old folded snapshot.
    holdOpen = true;
    await primary.locator("[data-checkout]").click();
    await expect.poll(() => heldOpen.release !== null).toBe(true);
    await expect(page.locator("[data-sidebar-overview]")).toHaveAttribute("aria-current", "page");
    await expect(page.locator("[data-main-screen]")).toBeVisible();
    await expect(primaryToggle).toHaveAttribute("aria-expanded", "false");
    holdOpen = false;
    heldOpen.release!();
    await expect(primaryToggle).toHaveAttribute("aria-expanded", "true");
    await expect(primary.locator("[data-checkout]")).toHaveAttribute("aria-current", "true");
    await primary.locator("[data-checkout]").click();
    await expect(primaryToggle).toHaveAttribute("aria-expanded", "false");
    await expect(primary.locator("[data-purpose]")).toHaveText("메인 체크아웃 정리");
    await rest(page);
    expect(await rowGeometry(primaryRow, feature, primaryParts)).toEqual(atRest);

    // An agentless checkout uses the same single-line height as Overview;
    // opening it sends no disclosure intent and leaves the fold slot empty.
    await project.locator("[data-empty-worktrees]").click();
    const emptyRow = project.locator("[data-checkout-row]").filter({ has: page.locator('[data-checkout][aria-label^="empty"]') });
    const emptyButton = emptyRow.locator("[data-checkout]");
    await expect(emptyRow.locator("[data-checkout-toggle]")).toHaveCount(0);
    expect((await emptyButton.boundingBox())!.height).toBe(await emptyButton.evaluate((node) => parseFloat(getComputedStyle(node).getPropertyValue("--size-checkout-row"))));
    await emptyButton.click();
    await expect(emptyButton).toHaveAttribute("aria-current", "true");
    expect(last.get("focus_checkout")?.expanded).toBeUndefined();

    // The chevron opens the agent rows below the row: their own marks stand
    // for the checkout's badge, which goes, and the row keeps its purpose,
    // its age and its height. The project's badge stays.
    const featureButton = feature.locator("[data-checkout]");
    const closedHeight = (await featureButton.boundingBox())!.height;
    await featureToggle.click();
    await expect(featureToggle).toHaveAttribute("aria-expanded", "true");
    await expect(feature.locator(`[data-checkout-agents-open] [data-pane="${rowsPane}"]`)).toBeVisible();
    await expect(feature.locator("[data-checkout-status]")).toHaveCount(0);
    await expect(feature.locator("[data-purpose]")).toBeVisible();
    expect((await featureButton.boundingBox())!.height).toBe(closedHeight);
    await expect(project.locator("[data-project-status]")).toBeVisible();
    await screenshot(page, "projects-sidebar-open");

    // The row itself still opens the checkout.
    await feature.locator("[data-checkout]").click();
    await expect(feature.locator("[data-checkout]")).toHaveAttribute("aria-current", "true");

    // A plain folder is one row: no project row or fold of its own, a folder
    // glyph, and its agent counted on its badge, which, being the project's,
    // stays while its chevron has the agent rows open.
    const folder = page.locator("[data-project]", { hasText: /^notes/ });
    await expect(folder.locator("[data-project-row]")).toHaveCount(0);
    await expect(folder.locator("[data-project-toggle]")).toHaveCount(0);
    const folderRow = folder.locator("[data-checkout]");
    await expect(folderRow).toHaveCount(1);
    await expect(folderRow).toHaveAttribute("data-checkout-kind", "folder");
    await expect(folder.locator('[data-project-status] [data-badge-part="idle"]')).toHaveText("1");
    const folderToggle = folder.locator("[data-checkout-toggle]");
    await expect(folderToggle).toHaveAttribute("aria-expanded", "true");
    await folderToggle.click();
    await expect(folderToggle).toHaveAttribute("aria-expanded", "false");
    // The row opens the folder's checkout, not an Overview.
    await folderRow.click();
    await expect(folderRow).toHaveAttribute("aria-current", "true");
    await expect(page.locator("[data-workspace-screen]")).toBeVisible();
    await expect(feature.locator("[data-checkout]")).not.toHaveAttribute("aria-current", "true");
    await expect(folder.locator("[data-project-overview]")).toHaveCount(0);
    await expect(folderToggle).toHaveAttribute("aria-expanded", "true");
    await expect(featureToggle).toHaveAttribute("aria-expanded", "true");
    await expect(folder.locator(`[data-checkout-agents-open] [data-pane="${notesPane}"]`)).toBeVisible();
    await expect(folder.locator("[data-project-status]")).toBeVisible();
    // B7: an agent's elapsed time counts from the state change the core saw.
    await expect(folder.locator(`[data-pane="${notesPane}"] [data-agent-elapsed]`)).toHaveText(/^\d+[sm]$/);
    await expect(feature.locator(`[data-pane="${rowsPane}"] [data-agent-elapsed]`)).toHaveText(/^\d+[sm]$/);
    await folderToggle.click();
    await expect(folder.locator("[data-checkout-agents-open]")).toHaveCount(0);

    // Delegated children leave the checkout tree and stay reachable from
    // the root's direct-child badge, without a second lineage disclosure.
    declareParent(herdr, rowsPane, mainPane);
    await primaryToggle.click();
    const parentRow = primary.locator(`[data-checkout-agents-open] [data-pane="${mainPane}"]`);
    const badge = parentRow.locator("[data-descendant-badge]");
    await expect(badge).toBeVisible({ timeout: 20_000 });
    await expect(page.locator(`nav[data-sidebar] [data-pane="${rowsPane}"]`)).toHaveCount(0);
    await expect(page.locator("[data-agent-tree-toggle], [data-checkout-line]")).toHaveCount(0);
    const beforeBadge = new Map(sent);
    await badge.click();
    await expect(page.locator(`[data-agent-child="${rowsPane}"] [data-branch-chip]`)).toHaveText("feature/sidebar-rows");
    await screenshot(page, "projects-sidebar-child-popover");
    await page.keyboard.press("Escape");
    await expect(badge).toBeFocused();
    expect([...sent].filter(([kind, count]) => count !== (beforeBadge.get(kind) ?? 0)).map(([kind]) => kind)).toEqual([]);
    expect(await sidebarColumns(page)).toEqual({ times: [expect.any(Number)], chevrons: [expect.any(Number)] });
    const screenBefore = await page.locator("[data-workspace-screen]").count();
    const quiet = new Map(sent);
    await primaryToggle.click();
    await primaryToggle.click();
    await projectToggle.click();
    await projectToggle.click();
    await expect(projectToggle).toHaveAttribute("aria-expanded", "true");
    const moved = [...sent].filter(([kind, count]) => count !== (quiet.get(kind) ?? 0)).map(([kind]) => kind);
    expect(moved.filter((kind) => !["checkout_agents_toggle", "project_checkouts_fold", "ui_state_update", "ui_state_update.usage_hint"].includes(kind))).toEqual([]);
    expect(await page.locator("[data-workspace-screen]").count()).toBe(screenBefore);
    await expect(folderRow).toHaveAttribute("aria-current", "true");

    // Folding the project hides its checkouts; both folds survive a reload.
    await projectToggle.click();
    await expect(projectToggle).toHaveAttribute("aria-expanded", "false");
    await expect(project.locator("[data-checkout]")).toHaveCount(0);
    await expect(project.locator("[data-project-overview]")).toHaveCount(0);
    await open(page, daemon);
    await expect(projectToggle).toHaveAttribute("aria-expanded", "false");
    await expect(project.locator("[data-checkout]")).toHaveCount(0);
    // One project-row event returns to the last checkout and carries its fold intent.
    const projectRow = project.locator("[data-project-row]");
    const beforeRow = new Map(sent);
    await projectRow.click();
    await expect(page.locator("[data-workspace-screen]")).toBeVisible();
    await expect(projectToggle).toHaveAttribute("aria-expanded", "true");
    expect((sent.get("focus_checkout") ?? 0) - (beforeRow.get("focus_checkout") ?? 0)).toBe(1);
    expect(last.get("focus_checkout")).toMatchObject({ workspace_id: await project.getAttribute("data-project"), project_expanded: true });
    expect(sent.get("project_checkouts_fold") ?? 0).toBe(beforeRow.get("project_checkouts_fold") ?? 0);
    await projectRow.click();
    await expect(projectToggle).toHaveAttribute("aria-expanded", "false");
    await expect(project.locator("[data-checkout]")).toHaveCount(0);
    await expect(page.locator("[data-workspace-screen]")).toBeVisible();
    await projectRow.click();
    // B1: this checkout now contains only the delegated child. It keeps its
    // checkout identity but has no operator session rows to disclose.
    await expect(featureToggle).toHaveCount(0);
    await expect(primaryToggle).toHaveAttribute("aria-expanded", "true");
    await expect(parentRow.locator("[data-descendant-badge]")).toBeVisible();
    await expect(page.locator(`nav[data-sidebar] [data-pane="${rowsPane}"]`)).toHaveCount(0);
    // B25: every level open, at the design width and the app's minimum, nothing runs sideways.
    for (const width of ["calc(240px + var(--size-rail))", "calc(var(--size-sidebar-min) + var(--size-rail))"]) expect(await sidebarOverflow(page, width)).toEqual([]);
    await sidebarOverflow(page, "");

    // B4 in Agents: a quiet row and the row after it stay put through hover,
    // keyboard focus and selection.
    await folderToggle.click();
    const quietRows = page.locator("nav[data-sidebar] li[data-pane]:not(:has([data-agent-line])):has([data-agent-elapsed])");
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
    await folderToggle.click();
    await expect(folderToggle).toHaveAttribute("aria-expanded", "false");

    for (const theme of ["light", "dark"] as const) {
      await chooseTheme(page, theme);
      // The Settings shortcut leaves keyboard focus on the last clicked control; its ring is not part of the rows.
      await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
      await page.locator("[data-project-list]").hover({ position: { x: 1, y: 1 } });
      await screenshot(page, `projects-sidebar-${theme}`);
    }

    // PRD sidebar-typography B8 (was sidebar-readability B26): the sidebar
    // does not follow the interface font. At the largest size its text and
    // rows are the ones the default draws, while text outside it grows, and
    // hover still moves nothing.
    await rest(page);
    const projectsAtDefault = { sizes: await sidebarTextSizes(page), row: await rowGeometry(primaryRow, feature, primaryParts) };
    const outsideAtDefault = await probeTextSize(page, "main");
    await rest(page);
    const agentsAtDefault = { sizes: await sidebarTextSizes(page), row: await rowGeometry(agentRow, nextRow, agentParts) };
    await page.keyboard.press(chord("settings"));
    await page.locator('[data-settings-tab="general"]').click();
    const fontSize = page.locator('[data-font-size="true"] [role="slider"]');
    await fontSize.focus();
    await page.keyboard.press("End");
    await expect(fontSize).toHaveAttribute("aria-valuenow", "17");
    await page.keyboard.press("Escape");
    await expect(page.locator('[data-settings="true"]')).toHaveCount(0);
    // The scale is already set at the default size, so wait for its effect, not its presence.
    await expect.poll(() => probeTextSize(page, "main")).toBeGreaterThan(outsideAtDefault);
    await rest(page);
    expect(await probeTextSize(page, "nav[data-sidebar]")).toBe(12);
    expect(await sidebarTextSizes(page)).toEqual(projectsAtDefault.sizes);
    expect(await rowGeometry(primaryRow, feature, primaryParts)).toEqual(projectsAtDefault.row);
    expect(await sidebarRowsFit(page)).toEqual([]);
    expect(await projectButtonFills(page)).toBe(true);
    await primaryRow.hover();
    expect(await rowGeometry(primaryRow, feature, primaryParts)).toEqual(projectsAtDefault.row);
    await rest(page);
    await keyboardFocus(page, primary.locator("[data-checkout]"));
    expect(await rowGeometry(primaryRow, feature, primaryParts)).toEqual(projectsAtDefault.row);
    await rest(page);
    await screenshot(page, "projects-sidebar-large-font");
    await rest(page);
    expect(await sidebarTextSizes(page)).toEqual(agentsAtDefault.sizes);
    expect(await rowGeometry(agentRow, nextRow, agentParts)).toEqual(agentsAtDefault.row);
    expect(await sidebarRowsFit(page)).toEqual([]);
    await agentRow.hover();
    expect(await rowGeometry(agentRow, nextRow, agentParts)).toEqual(agentsAtDefault.row);
    await rest(page);
    await screenshot(page, "agents-sidebar-large-font");

    // B3: on an input with no hover, the controls that wait for the pointer are always shown.
    const cdp = await page.context().newCDPSession(page);
    await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 1 });
    await open(page, daemon);
    expect(await page.evaluate(() => matchMedia("(hover: none)").matches)).toBe(true);
    await rest(page);
    await expect(projectToggle).toHaveAttribute("aria-expanded", "true");
    await expect(projectToggle).toHaveCSS("opacity", "1");
    await expect(primaryToggle).toHaveCSS("opacity", "1");
    await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false });

    // B20: before the first snapshot the list says it is connecting, not that it is empty.
    await page.goto("about:blank");
    await page.goto(`${daemon.origin}/#token=refused`);
    await expect(page.locator("[data-sidebar-loading]")).toBeVisible({ timeout: 20_000 });
    await expect(page.locator("[data-projects-empty]")).toHaveCount(0);
  } finally {
    await daemon?.stop();
    await herdr.stop();
  }
});

test("Needs You and Done are raised above Pinned and stay in their tree, whatever is folded", async ({ page }) => {
  await page.setViewportSize({ width: 1400, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const repo = path.join(herdr.root, "raised");
    fs.mkdirSync(repo);
    git(repo, ["init"]);
    git(repo, ["commit", "--allow-empty", "-m", "initial"]);
    const asking = await workspaceAt(herdr, repo, "배포 전 확인 요청");
    const [finished] = herdr.panes;
    daemon = await startHided(herdr, "projects-raised");
    const sent = countSent(page);
    await open(page, daemon);

    const project = page.locator("[data-project]").filter({ has: page.locator("[data-project-row]", { hasText: /^raised/ }) });
    await project.locator("[data-project-row]").click({ button: "right" });
    await page.getByRole("menuitem", { name: "Pin", exact: true }).click();
    const pinned = page.locator('[data-section="pinned"]');
    await expect(pinned).toBeVisible();
    // Nothing needs the operator yet: no raised section is drawn.
    await expect(page.locator("[data-raised-group]")).toHaveCount(0);

    labelAgent(herdr, asking, { task: "배포 전 확인 요청", reply: "배포해도 될까요?", question: true });
    labelAgent(herdr, finished, { task: "Agent one", progress: "정리 끝" });
    await finishFixtureTurn(herdr, finished, elsewhereTab(herdr));
    const needsYou = page.locator('[data-raised-group="needs_you"]');
    const done = page.locator('[data-raised-group="done"]');
    await expect(needsYou.locator(`[data-pane="${asking}"]`)).toBeVisible({ timeout: 20_000 });
    await expect(needsYou.locator('[data-section="needs_you"]')).toHaveText("Needs You · 1");
    await expect(done.locator(`[data-pane="${finished}"]`)).toBeVisible();
    await expect(done.locator('[data-section="done"]')).toHaveText("Done · 1");
    // Needs You, then Done, then Pinned, at the top of the list; neither is past its cap, so nothing folds.
    const top = async (locator: Locator) => (await locator.boundingBox())!.y;
    expect(await top(needsYou)).toBeLessThan(await top(done));
    expect(await top(done)).toBeLessThan(await top(pinned));
    await expect(page.locator("[data-raised-more]")).toHaveCount(0);
    await expect(page.locator("[data-project-list] > li").first()).toHaveAttribute("data-raised-group", "needs_you");
    // The raised row names where the agent runs, as the Agents list does.
    await expect(needsYou.locator(`[data-pane="${asking}"] [data-agent-place]`)).toHaveText("raised › main");

    // The agent stays in its checkout tree too.
    const checkoutToggle = project.locator("[data-checkout-toggle]");
    await checkoutToggle.click();
    await expect(project.locator(`[data-checkout-agents-open] [data-pane="${asking}"]`)).toBeVisible();
    await screenshot(page, "projects-raised-open");

    // Folding the checkout and the project hides the tree rows, not the raised ones.
    await checkoutToggle.click();
    await project.locator("[data-project-toggle]").click();
    await expect(project.locator("[data-project-toggle]")).toHaveAttribute("aria-expanded", "false");
    await expect(project.locator(`[data-pane="${asking}"]`)).toHaveCount(0);
    await expect(needsYou.locator(`[data-pane="${asking}"]`)).toBeVisible();
    await screenshot(page, "projects-raised-folded");

    // A raised row opens its agent as the Agents row does: one focus_pane.
    const before = sent.get("focus_pane") ?? 0;
    await needsYou.locator(`[data-pane="${asking}"] [data-agent-open]`).click();
    await expect.poll(() => sent.get("focus_pane") ?? 0).toBe(before + 1);
    await expect(page.locator("[data-workspace-screen]")).toBeVisible();

    // Answered, the agent leaves Needs You and its empty section goes.
    await setFixtureLifecycle(herdr, asking, "working");
    await setFixtureLifecycle(herdr, asking, "idle");
    await expect(needsYou).toHaveCount(0, { timeout: 20_000 });
  } finally {
    await daemon?.stop();
    await herdr.stop();
  }
});
