// Closing an agent that spawned others (PRD close-agent-subtree): the close
// sheet lists the descendants, Escape sends nothing, Enter closes the whole
// subtree deepest first with the target last, and Close only leaves the
// children running as the operator's own rows. Delete worktree asks the same
// question about the agents its checkout spawned outside it.

import { expect, test, type Page } from "@playwright/test";
import { execFile, execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { promisify } from "node:util";
import { spawnAgent, startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, enterWorkspace, screenshot } from "./wire";

test.describe.configure({ timeout: 180_000 });
test.use({ actionTimeout: 15_000 });

const run = promisify(execFile);

/** Every pane id the private Herdr lists right now. */
async function livePanes(herdr: HerdrFixture): Promise<Set<string>> {
  const { stdout } = await run(herdr.bin, ["pane", "list"], { env: herdr.env, encoding: "utf8", timeout: 10_000 });
  const ids = new Set<string>();
  const visit = (value: unknown): void => {
    if (Array.isArray(value)) value.forEach(visit);
    else if (value && typeof value === "object") {
      const row = value as Record<string, unknown>;
      if (typeof row.pane_id === "string") ids.add(row.pane_id);
      Object.values(row).forEach(visit);
    }
  };
  visit(JSON.parse(stdout));
  return ids;
}

/**
 * Watches Herdr until every pane in `watched` is gone and returns them in
 * the order they went. Each tree node waits for Herdr to confirm the one
 * below it, so a poll this fast sees every step.
 */
async function closeOrder(herdr: HerdrFixture, watched: string[]): Promise<string[]> {
  const order: string[] = [];
  const deadline = Date.now() + 60_000;
  while (order.length < watched.length && Date.now() < deadline) {
    const live = await livePanes(herdr);
    const gone = watched.filter((pane) => !live.has(pane) && !order.includes(pane));
    // Two gone in one poll would make the order unreadable; the core never
    // starts a parent before its child is confirmed gone.
    expect(gone.length, `closed together: ${gone.join(", ")}`).toBeLessThanOrEqual(1);
    order.push(...gone);
  }
  return order;
}

async function agentsMode(page: Page): Promise<void> {
  await page.locator('[data-sidebar-mode="agents"]').click();
}

test("the close sheet closes the whole subtree deepest first, and Close only keeps the children", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const [keeper, target] = herdr.panes;
    // target spawned child, which spawned grandchild; keeper spawned kept.
    const child = await spawnAgent(herdr, "child", target);
    const grandchild = await spawnAgent(herdr, "grandchild", child);
    const kept = await spawnAgent(herdr, "kept", keeper);

    daemon = await startHided(herdr, "close-subtree");
    const last = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, last);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    await agentsMode(page);
    // The sheet reads the core's close lists, so wait until the lineage is in.
    await page.locator(`[data-agent-tree-toggle="${target}"]`).click({ timeout: 30_000 });
    await expect(page.locator(`[data-agent-tree-toggle="${child}"]`)).toBeVisible({ timeout: 30_000 });
    await expect(page.locator(`[data-agent-tree-toggle="${keeper}"]`)).toBeVisible({ timeout: 30_000 });

    const sheet = page.locator("[data-confirm-subtree]");
    await page.locator(`[data-terminal-host="${target}"]`).click();
    await page.keyboard.press("Alt+KeyW");
    await expect(sheet).toBeVisible();
    await expect(sheet.getByRole("heading")).toHaveText("이 에이전트와 자식 2개를 닫을까요?");
    await expect(sheet.locator("[data-subtree-row]")).toHaveCount(3);
    await expect(sheet.locator("[data-subtree-row]").first()).toHaveAttribute("data-subtree-row", target);
    await expect(sheet.locator("[data-subtree-row]").nth(1)).toHaveAttribute("data-subtree-row", child);
    await expect(sheet.locator("[data-subtree-row]").nth(2)).toHaveAttribute("data-subtree-row", grandchild);
    await expect(sheet.locator("[data-subtree-consequence]")).toHaveText("이것만 닫기: 자식은 계속 실행되고 내 목록으로 올라옵니다.");
    // Enter's default is Close all (D-18).
    await expect(sheet.locator("[data-subtree-close-all]")).toBeFocused();
    await screenshot(page, "close-subtree-sheet");

    // Escape sends nothing and leaves every pane as it was.
    await page.keyboard.press("Escape");
    await expect(sheet).toHaveCount(0);
    expect(sent.get("close_tree") ?? 0).toBe(0);
    expect(sent.get("close_pane") ?? 0).toBe(0);
    const before = await livePanes(herdr);
    for (const pane of [target, child, grandchild]) expect(before.has(pane)).toBe(true);

    // Enter closes the subtree as one event, deepest first, target last.
    await page.locator(`[data-terminal-host="${target}"]`).click();
    await page.keyboard.press("Alt+KeyW");
    await expect(sheet.locator("[data-subtree-close-all]")).toBeFocused();
    const order = closeOrder(herdr, [target, child, grandchild]);
    await page.keyboard.press("Enter");
    expect(await order).toEqual([grandchild, child, target]);
    expect(sent.get("close_tree")).toBe(1);
    expect(last.get("close_tree")).toMatchObject({ target: { kind: "pane", pane_id: target }, pane_ids: [grandchild, child], confirmed: true });
    for (const pane of [target, child, grandchild]) await expect(page.locator(`[data-pane="${pane}"]`)).toHaveCount(0, { timeout: 20_000 });
    await screenshot(page, "close-subtree-all-closed");

    // Close only closes just the target; its child becomes the operator's.
    await page.locator(`[data-terminal-host="${keeper}"]`).click();
    await page.keyboard.press("Alt+KeyW");
    await expect(sheet.getByRole("heading")).toHaveText("이 에이전트와 자식 1개를 닫을까요?");
    await sheet.locator("[data-subtree-close-only]").click();
    await expect.poll(async () => (await livePanes(herdr)).has(keeper), { timeout: 20_000 }).toBe(false);
    expect((await livePanes(herdr)).has(kept)).toBe(true);
    expect(sent.get("close_tree")).toBe(1);
    await expect(page.locator(`[data-pane="${kept}"]`)).toHaveAttribute("data-depth", "0", { timeout: 20_000 });
    await screenshot(page, "close-subtree-close-only");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});

function git(cwd: string, args: string[]): string {
  return execFileSync("git", ["-c", "user.name=e2e", "-c", "user.email=e2e@example.invalid", "-c", "init.defaultBranch=main", ...args], { cwd, encoding: "utf8" });
}

test("Delete worktree closes the agents its checkout spawned outside it before the folder goes", async ({ page }) => {
  await page.setViewportSize({ width: 1400, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const branch = "feature/spawner";
    const repo = path.join(herdr.root, "repo");
    fs.mkdirSync(repo);
    git(repo, ["init"]);
    fs.writeFileSync(path.join(repo, "README.md"), "# repo\n");
    git(repo, ["add", "README.md"]);
    git(repo, ["commit", "-m", "initial"]);
    git(repo, ["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(repo, ["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"]);
    const worktree = path.join(herdr.root, "repo-spawner");
    git(repo, ["worktree", "add", "-b", branch, worktree]);
    // The worktree's agent spawned one in another folder.
    const spawner = await spawnAgent(herdr, "spawner", null, worktree);
    const outside = await spawnAgent(herdr, "outside", spawner);

    daemon = await startHided(herdr, "close-subtree-worktree");
    const last = new Map<string, Record<string, unknown>>();
    countSent(page, last);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await expect(page.locator("[data-main-screen]")).toBeVisible({ timeout: 20_000 });
    await agentsMode(page);
    await expect(page.locator(`[data-agent-tree-toggle="${spawner}"]`)).toBeVisible({ timeout: 30_000 });
    await page.locator('[data-sidebar-mode="projects"]').click();

    const feature = page.locator("[data-checkout-row]").filter({ has: page.locator(`[data-checkout][aria-label^="${branch}"]`) });
    await expect(feature).toBeVisible({ timeout: 30_000 });
    await feature.locator("[data-checkout-menu]").click({ button: "right" });
    await page.getByRole("menu", { name: `${branch} actions` }).locator('[data-menu-item="delete_worktree"]').click();

    const dialog = page.locator("[data-delete-worktree]");
    await expect(dialog.locator("[data-removal-subtree]")).toContainText("1 agent spawned from this worktree runs outside it:", { timeout: 30_000 });
    await expect(dialog.locator(`[data-subtree-row="${outside}"]`)).toBeVisible();
    const withOutside = dialog.locator('[data-delete-confirm="with-outside"]');
    await expect(withOutside).toHaveText("Close 1 agent outside, then delete");
    await expect(dialog.locator('[data-delete-confirm="only"]')).toHaveText("Delete only this worktree");
    // Neither action holds the keyboard when the dialog opens (D-35).
    await expect(withOutside).not.toBeFocused();
    await expect(dialog.locator('[data-delete-confirm="only"]')).not.toBeFocused();
    await screenshot(page, "close-subtree-delete-worktree");

    await withOutside.click();
    expect(last.get("remove_worktree")).toMatchObject({ close_descendant_pane_ids: [outside] });
    await expect(dialog.locator('[data-delete-result="finished"]')).toBeVisible({ timeout: 60_000 });
    const live = await livePanes(herdr);
    expect(live.has(outside)).toBe(false);
    expect(live.has(spawner)).toBe(false);
    expect(fs.existsSync(worktree)).toBe(false);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
