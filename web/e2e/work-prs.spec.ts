// The PRs tool and the pull request decision card on an isolated pinned
// Herdr and hided (PRD work-tab-pr-lens; PRD sidebar-task-lens B26-B37): a
// Git project whose pull requests a fake `gh` answers and records every write
// of, so nothing reaches GitHub. The list groups by whose move it is with a
// reason line per row (B6, B8, B24); a row opens the card in the lane's 보기
// slot, never as a Files tab (B27, B50); Merge asks once and goes out
// conditioned on the head commit the card showed (B37); a pull request that
// moved after the card was drawn is refused and the card says so (B37);
// Update branch reaches a behind pull request. Light and Dark captures land in
// HIDE_E2E_SCREENSHOT_DIR.

import { expect, test, type Locator, type Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { initGithubRepository } from "./github-remote";
import { startHerdr } from "./herdr-fixture";
import { startHided } from "./hided-fixture";
import { fakeGh, git, head, moveHead, skewCardHead, writes } from "./work-prs-fixture";
import { chord } from "./chords";
import { chooseTheme, countSent, screenshot, showProjectRow, showTool } from "./wire";

test.describe.configure({ timeout: 180_000 });

/** A project on a private Herdr and hided with the fake `gh`, the PRs tool open. */
async function openWork(page: Page, fn: (stack: { gh: { bin: string; state: string }; work: Locator; sent: Map<string, number>; last: Map<string, Record<string, unknown>> }) => Promise<void>): Promise<void> {
  await page.setViewportSize({ width: 1600, height: 1000 });
  const herdr = await startHerdr();
  let daemon: Awaited<ReturnType<typeof startHided>> | null = null;
  try {
    const repo = path.join(herdr.root, "repo");
    const origin = path.join(herdr.root, "origin.git");
    fs.mkdirSync(repo);
    git(herdr.root, ["init", "--bare", origin]);
    initGithubRepository(repo);
    fs.writeFileSync(path.join(repo, "README.md"), "# repo\n");
    git(repo, ["add", "README.md"]);
    git(repo, ["commit", "-m", "initial"]);
    git(repo, ["remote", "add", "origin", origin]);
    git(repo, ["push", "-q", "origin", "main"]);
    git(repo, ["remote", "set-head", "origin", "main"]);
    herdr.run(["workspace", "create", "--cwd", repo, "--label", "repo", "--env", `PATH=${herdr.fixturePath}`, "--no-focus"]);

    const gh = fakeGh(herdr.root);
    daemon = await startHided(herdr, "work-prs", undefined, { PATH: `${gh.bin}${path.delimiter}${herdr.fixturePath}` });
    const last = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, last);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await (await showProjectRow(page, page.locator("[data-project]", { hasText: "repo" }).locator("[data-checkout]").first())).click();
    await chooseTheme(page, "light");
    await showTool(page, "prs");
    const work = page.locator('aside[data-lane="prs"]');
    await expect(work.locator("[data-pr-group]").first()).toBeVisible({ timeout: 30_000 });
    await fn({ gh, work, sent, last });
  } finally {
    daemon?.stop();
    herdr.stop();
  }
}

/** The card in the lane's 보기 slot (PRD sidebar-task-lens D-50). */
const cardOf = (page: Page, number: number) => page.locator(`aside[data-lane="detail"] [data-pr-card="${number}"]`);

/** Opens one of the card's folded lines (B36); the card keeps it open while the app runs. */
async function unfold(card: Locator, section: "body" | "files" | "links"): Promise<void> {
  const toggle = card.locator(`[data-pr-card-toggle="${section}"]`);
  if ((await toggle.getAttribute("aria-expanded")) !== "true") await toggle.click();
  await expect(toggle).toHaveAttribute("aria-expanded", "true");
}

test("the PRs tool groups by whose move it is and says why on each row", async ({ page }) => {
  await openWork(page, async ({ work }) => {
    // The rail is Memory · PRs | Files · Changes, PRs pressed (B23).
    await expect(page.locator("[data-lane-tool]")).toHaveCount(4);
    await expect(page.locator('[data-lane-tool="prs"]')).toHaveAttribute("aria-pressed", "true");
    // The PRs icon carries the number of its Needs You rows: the three Needs You PRs (B29).
    await expect(page.locator('[data-lane-tool="prs"] [data-lane-prs-count]')).toHaveText("3");

    // The groups by whose move it is; each row says why in its second line (B6, B8, B24).
    const groups = work.locator("[data-pr-group]");
    expect(await groups.evaluateAll((sections) => sections.map((section) => section.getAttribute("data-pr-group")))).toEqual(["needs_you", "blocked", "drafts", "bots", "merged"]);
    const rowsOf = (group: string) => work.locator(`[data-pr-group="${group}"] [data-pr]`).evaluateAll((rows) => rows.map((row) => row.getAttribute("data-pr")));
    await expect(work.locator('[data-pr-group="needs_you"] [data-pr]')).toHaveCount(3);
    expect((await rowsOf("needs_you")).sort()).toEqual(["31", "32", "33"]);
    expect(await rowsOf("blocked")).toEqual(["34"]);
    // Drafts, Bots and merged start folded (B7); a group opens from its header.
    for (const group of ["drafts", "bots", "merged"]) await expect(work.locator(`[data-pr-group="${group}"]`)).toHaveAttribute("data-folded", "true");
    await work.locator('[data-pr-group="drafts"] [data-pr-focus="group"]').click();
    expect(await rowsOf("drafts")).toEqual(["35"]);
    const reason = (number: number) => work.locator(`[data-pr="${number}"] [data-pr-reason]`);
    await expect(reason(31)).toHaveAttribute("data-pr-reason", "merge");
    await expect(reason(32)).toHaveAttribute("data-pr-reason", "update");
    await expect(reason(33)).toHaveAttribute("data-pr-reason", "review");
    await expect(reason(34)).toHaveAttribute("data-pr-reason", "conflict");
    await screenshot(page, "work-prs-light");
  });
});

test("the card opens in the 보기 slot and Merge goes out on the head it showed", async ({ page }) => {
  await openWork(page, async ({ gh, work, sent, last }) => {
    // A row opens the card in the 보기 slot and no Files tab (B27, B50), and
    // merge asks once, then goes out conditioned on the head commit the card
    // showed (B37).
    await work.locator('[data-pr-row="31"]').click();
    const card = cardOf(page, 31);
    await expect(card).toBeVisible({ timeout: 30_000 });
    await expect(page.locator('[data-view-tab-bar] [role="tab"]', { hasText: "#31" })).toHaveCount(0);
    await expect(card.locator('[data-pr-card-reason="merge"]')).toBeVisible();
    // The four steps fill to review for a pull request that can merge (B34).
    await expect(card.locator("[data-pr-card-steps]")).toHaveAttribute("data-pr-card-steps", "3");
    await expect(card.locator("[data-pr-card-files]")).toHaveAttribute("data-pr-card-files", "3", { timeout: 30_000 });
    await expect(card.locator("[data-pr-card-outline]")).toBeVisible();
    await unfold(card, "files");
    await expect(card.locator("[data-pr-card-generated]")).toBeVisible();
    await screenshot(page, "work-pr-card-light");
    await chooseTheme(page, "dark");
    await screenshot(page, "work-pr-card-dark");
    await chooseTheme(page, "light");
    const merge = card.locator("[data-pr-card-merge]");
    await expect(merge).toBeEnabled();
    expect(writes(gh.state)).toEqual([]);
    await merge.click();
    await expect(card.locator("[data-pr-card-confirm]")).toBeVisible();
    expect(writes(gh.state)).toEqual([]);
    await card.locator("[data-pr-card-confirm-yes]").click();
    // The merged pull request's buttons, and with them the write line, are
    // gone as soon as the merge is seen, so the write is read from gh's log.
    await expect.poll(() => writes(gh.state), { timeout: 30_000 }).toHaveLength(1);
    expect(sent.get("pr_merge")).toBe(1);
    expect(last.get("pr_merge")).toMatchObject({ pr_number: 31, head: head(gh.state, 31) });
    expect(writes(gh.state)).toEqual([["pr", "merge", "31", "--merge", "--match-head-commit", head(gh.state, 31)]]);
    // The merged pull request's card draws no Merge, and the rail's PRs icon
    // brings the list back with it out of Needs You.
    await expect(card.locator("[data-pr-card-merge]")).toHaveCount(0);
    await page.locator('[data-lane-tool="prs"]').click();
    await expect(work.locator('[data-pr-group="needs_you"] [data-pr]').first()).toBeVisible();
    await expect(work.locator('[data-pr-group="needs_you"] [data-pr="31"]')).toHaveCount(0);
    await work.locator('[data-pr-group="merged"] [data-pr-focus="group"]').click();
    await expect(work.locator('[data-pr-group="merged"] [data-pr="31"]')).toHaveCount(1);
  });
});

test("a branch update on a pull request that moved is refused, looked at again, then goes out on the new head", async ({ page }) => {
  await openWork(page, async ({ gh, work }) => {
    // GitHub refuses, the card says it changed, Look again reads it and the
    // update then goes out on the new head (B32, B33).
    await work.locator('[data-pr-row="32"]').click();
    const behind = cardOf(page, 32);
    await expect(behind).toBeVisible({ timeout: 30_000 });
    await expect(behind.locator('[data-pr-card-reason="update"]')).toBeVisible();
    await expect(behind.locator("[data-pr-card-files]")).toBeVisible({ timeout: 30_000 });
    const shown = head(gh.state, 32);
    moveHead(gh.state, 32, shown.slice(0, 39) + "e");
    await behind.locator("[data-pr-card-update]").click();
    await behind.locator('[data-pr-card-confirm-yes="update"]').click();
    await expect(behind.locator('[data-pr-card-write="failed"]')).toHaveAttribute("data-pr-card-write-reason", "head_moved", { timeout: 30_000 });
    await screenshot(page, "work-pr-card-moved-light");
    // Until Look again has read the card the refused write cannot be repeated.
    await expect(behind.locator("[data-pr-card-update]")).toBeDisabled();
    // Look again reads the list and the card; the update goes out once both
    // show the new head, on the head the card showed (B33).
    await behind.locator("[data-pr-card-look-again]").click();
    await expect(behind.locator("[data-pr-card-title]")).toContainText("(moved)", { timeout: 30_000 });
    await expect(behind).toHaveAttribute("data-pr-card-head", `${shown.slice(0, 39)}e`, { timeout: 30_000 });
    await behind.locator("[data-pr-card-update]").click();
    await behind.locator('[data-pr-card-confirm-yes="update"]').click();
    await expect(behind.locator('[data-pr-card-write="done"]')).toBeVisible({ timeout: 30_000 });
    const updates = writes(gh.state).filter((args) => args[0] === "api");
    expect(updates).toHaveLength(2);
    expect(updates[1]).toEqual(["api", "--method", "PUT", "repos/acme/repo/pulls/32/update-branch", "-f", `expected_head_sha=${shown.slice(0, 39)}e`]);
  });
});

test("the list and the card read without sideways scrolling in each language and both themes", async ({ page }) => {
  await openWork(page, async ({ work }) => {
    await work.locator('[data-pr-row="33"]').click();
    const review = cardOf(page, 33);
    await expect(review).toBeVisible({ timeout: 30_000 });
    const overflow = (target: Locator) => target.evaluate((node) => node.scrollWidth - node.clientWidth);
    // The narrowest lane (B48): the divider's keys narrow it to its minimum.
    // The lane opens unpinned, over the agents; a pinned one would dock and redraw under the keys.
    if ((await page.locator("[data-lane-pin]").getAttribute("data-lane-pin")) !== "off") await page.locator("[data-lane-pin]").click();
    await expect(page.locator("[data-lane-pin]")).toHaveAttribute("data-lane-pin", "off");
    const divider = page.locator('[data-lane-divider="lane"]');
    await divider.focus();
    const reading = (name: string) => divider.evaluate((node, attribute) => Number(node.getAttribute(attribute)), name);
    const narrowest = await reading("aria-valuemin");
    for (let before = await reading("aria-valuenow"); before > narrowest; before = await reading("aria-valuenow")) {
      await page.keyboard.press("ArrowRight");
      await expect(divider).not.toHaveAttribute("aria-valuenow", String(before), { timeout: 10_000 });
    }
    await expect(divider).toHaveAttribute("aria-valuenow", String(narrowest));
    for (const [language, theme] of [["ko", "light"], ["ja", "dark"], ["zh-CN", "light"], ["en", "dark"]] as const) {
      await page.keyboard.press(chord("settings"));
      await page.locator('[data-settings-tab="general"]').click();
      await page.locator("[data-interface-language]").click();
      await page.locator(`[data-language-option="${language}"]`).click();
      await expect(page.locator("html")).toHaveAttribute("lang", language);
      await page.locator(`[data-theme-option="${theme}"]`).click();
      await page.keyboard.press("Escape");
      await expect(page.locator('[data-settings="true"]')).toHaveCount(0);
      // One thing shows at a time: the card in the 보기 slot, then the list in PRs,
      // and the same row brings the card back in front.
      await expect(review).toBeVisible();
      await expect.poll(() => overflow(review)).toBeLessThanOrEqual(0);
      await showTool(page, "prs");
      await expect(work.locator('[data-prs-view="board"]')).toBeVisible();
      await expect.poll(() => overflow(work.locator('[data-prs-view="board"]'))).toBeLessThanOrEqual(0);
      await screenshot(page, `work-prs-${language}-${theme}`);
      await work.locator('[data-pr-row="33"]').click();
    }
  });
});

test("a merged pull request opens its card and draws its parts, with no Merge", async ({ page }) => {
  await openWork(page, async ({ work }) => {
    await work.locator('[data-pr-group="merged"] [data-pr-focus="group"]').click();
    await work.locator('[data-pr-row="37"]').click();
    const card = cardOf(page, 37);
    await expect(card).toBeVisible({ timeout: 30_000 });
    await expect(card.locator("[data-pr-card-files]")).toBeVisible({ timeout: 30_000 });
    await expect(card.locator("[data-pr-card-steps]")).toHaveAttribute("data-pr-card-steps", "4");
    await unfold(card, "body");
    await expect(card.locator("[data-pr-card-preview]")).toBeVisible();
    await expect(card.locator("[data-pr-card-merge]")).toHaveCount(0);
  });
});

test("a card read at another head than the list's cannot merge: the write is refused before gh is asked", async ({ page }) => {
  await openWork(page, async ({ gh, work }) => {
    skewCardHead(gh.state, 31, head(gh.state, 31).slice(0, 39) + "a");
    await work.locator('[data-pr-row="31"]').click();
    const card = cardOf(page, 31);
    await expect(card.locator("[data-pr-card-files]")).toBeVisible({ timeout: 30_000 });
    await card.locator("[data-pr-card-merge]").click();
    await card.locator('[data-pr-card-confirm-yes="merge"]').click();
    await expect(card.locator('[data-pr-card-write="failed"]')).toHaveAttribute("data-pr-card-write-reason", "head_moved", { timeout: 30_000 });
    await expect(card.locator("[data-pr-card-merge]")).toBeDisabled();
    expect(writes(gh.state)).toEqual([]);
  });
});
