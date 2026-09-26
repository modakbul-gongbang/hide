// The ⌘K palette in the Swift search view's form (GitHub issue 154) on an
// isolated pinned Herdr and hided: the sidebar Search field and ⌘K open the
// same overlay; results sit under `<project> > AGENTS`, `WORKSPACES >
// PROJECTS` and `WORKSPACES > CHECKOUTS`; an agent row is its mark, its title
// and its state line under it; ↵ follows the selection across groups; Enter
// opens the agent; Escape closes and hands focus back; a query with no match
// says so. Captured in Dark and Light.

import { expect, test, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, screenshot } from "./wire";

test.describe.configure({ timeout: 120_000 });

function report(herdr: HerdrFixture, pane: string, set: Record<string, string>): void {
  const args = ["pane", "report-metadata", pane, "--source", "e2e-palette"];
  for (const [name, value] of Object.entries(set)) args.push("--token", `${name}=${value}`);
  execFileSync(herdr.bin, args, { env: herdr.env, timeout: 30_000 });
}

/** The group headings the open palette draws, top to bottom. */
function headings(page: Page) {
  return page.locator('[data-palette="Search"] [cmdk-group-heading]');
}

test("⌘K groups agents, projects and checkouts, and the sidebar field opens the same palette", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const [one, two] = herdr.panes;
    daemon = await startHided(herdr, "palette");
    const last = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, last);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await expect(page.locator('[data-sidebar="agents"]')).toBeVisible({ timeout: 20_000 });
    report(herdr, one, { status_working: "●", progress: "팔레트 그룹 검증 중" });
    await expect(page.locator(`[data-agent-list] [data-pane="${one}"]`)).toContainText("팔레트 그룹 검증 중", { timeout: 20_000 });

    // The sidebar field opens the palette with the query focused.
    const field = page.locator("[data-sidebar-search]");
    await expect(field).toContainText("Search");
    await expect(field).toContainText("⌘K");
    await field.click();
    const input = page.locator('[data-palette="Search"] [data-palette-input]');
    await expect(input).toBeFocused();
    await expect(input).toHaveAttribute("placeholder", "Search agents and workspaces");
    await expect(page.locator("[data-palette-esc]")).toHaveText("Esc");

    // Agents under their project, then projects, then checkouts.
    await expect(headings(page)).toHaveText(["fixture > AGENTS", "WORKSPACES > PROJECTS", "WORKSPACES > CHECKOUTS"]);
    const agents = page.locator('[data-palette-group^="agents:"]');
    await expect(agents.locator("[data-palette-row]")).toHaveCount(2);

    // An agent row: the provider mark, the title, the state line under it.
    const rowOne = page.locator(`[data-palette-row="agent:${one}"]`);
    await expect(rowOne.locator('[data-agent-mark="claude"]')).toBeVisible();
    await expect(rowOne).toContainText("Agent one");
    await expect(rowOne.locator("[data-palette-detail]")).toHaveText("팔레트 그룹 검증 중");
    const rowTwo = page.locator(`[data-palette-row="agent:${two}"]`);
    await expect(rowTwo).toContainText("Agent two");
    await expect(rowTwo.locator("[data-palette-detail]")).not.toBeEmpty();
    const titleBox = await rowOne.getByText("Agent one").boundingBox();
    const detailBox = await rowOne.locator("[data-palette-detail]").boundingBox();
    expect(titleBox && detailBox && detailBox.y >= titleBox.y + titleBox.height - 1).toBe(true);

    // ↵ marks the selected row and follows the arrows across a group boundary.
    const firstRow = page.locator('[data-palette="Search"] [data-palette-row]').first();
    await expect(firstRow).toHaveAttribute("aria-selected", "true");
    await expect(firstRow.locator("[data-palette-enter]")).toBeVisible();
    await screenshot(page, "palette-dark-default");
    const project = page.locator('[data-palette-group="projects"] [data-palette-row]').first();
    for (let step = 0; step < 6 && (await project.getAttribute("aria-selected")) !== "true"; step += 1) await page.keyboard.press("ArrowDown");
    await expect(project).toHaveAttribute("aria-selected", "true");
    await expect(project.locator("[data-palette-enter]")).toBeVisible();
    await expect(firstRow.locator("[data-palette-enter]")).toBeHidden();

    // Escape closes and hands focus back to the field that opened it.
    await page.keyboard.press("Escape");
    await expect(page.locator("[data-palette-input]")).toHaveCount(0);
    await expect(field).toBeFocused();

    // ⌘K is the same overlay; a query keeps its group, and Enter opens the agent.
    await page.keyboard.press("Meta+KeyK");
    await expect(input).toBeFocused();
    await page.keyboard.type("Agent two");
    await expect(headings(page).first()).toHaveText("fixture > AGENTS");
    await expect(page.locator('[data-palette="Search"] [data-palette-row]').first()).toHaveAttribute("data-palette-row", `agent:${two}`);
    const focuses = sent.get("focus_pane") ?? 0;
    await page.keyboard.press("Enter");
    await expect(page.locator("[data-palette-input]")).toHaveCount(0);
    await expect.poll(() => sent.get("focus_pane") ?? 0).toBeGreaterThan(focuses);
    expect(last.get("focus_pane")?.pane_id).toBe(two);

    // No match is one line, with no group and no selection.
    await page.keyboard.press("Meta+KeyK");
    await page.keyboard.type("zzzz-no-such-agent");
    await expect(page.locator('[data-palette-state="no-match"]')).toHaveText("No matching agents or workspaces");
    await expect(headings(page)).toHaveCount(0);
    await screenshot(page, "palette-dark-no-match");
    await page.keyboard.press("Escape");

    // Light: the same palette on the Light tokens. Opening the agent put its
    // Workspace on screen, so the Workspace commands lead as their own group.
    await page.keyboard.press("Alt+Comma");
    await page.locator('[data-settings-tab="appearance"]').click();
    await page.locator('[data-theme-option="light"]').click();
    await expect(page.locator("html")).toHaveClass(/(^|\s)light(\s|$)/);
    await page.keyboard.press("Escape");
    await field.click();
    await expect(input).toBeFocused();
    await expect(headings(page)).toHaveText(["WORKSPACE > COMMANDS", "fixture > AGENTS", "WORKSPACES > PROJECTS", "WORKSPACES > CHECKOUTS"]);
    await screenshot(page, "palette-light-default");
    // A query the agents match best brings their group above the commands'.
    await page.keyboard.type("Agent");
    await expect(headings(page).first()).toHaveText("fixture > AGENTS");
    await expect(page.locator('[data-palette="Search"] [data-palette-row]').first()).toHaveAttribute("aria-selected", "true");
    await screenshot(page, "palette-light-query");
    await page.keyboard.press("Escape");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
