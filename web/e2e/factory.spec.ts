// The Factory screens (PRD software-factory-ui) against a private Herdr and
// hided, on a Factory the stage-1 CLI made: the screen shows the engine's
// state and every action it sends is the engine's command, so each test
// reads the engine back through `hide factory` rather than trusting the
// screen. One stack per test; each test asserts one contract.

import { expect, test, type Page } from "@playwright/test";
import { chord } from "./chords";
import { openFactory, seedDag, startFactoryStack, type FactoryStack } from "./factory-fixture";

test.describe.configure({ timeout: 150_000 });

async function withStack(page: Page, label: string, run: (stack: FactoryStack) => Promise<void>) {
  const stack = await startFactoryStack(page, label);
  try {
    await run(stack);
  } finally {
    stack.stop();
  }
}

type Status = { my_turn: number; factories: { id: string; closed: boolean; columns: { column: string; cards: { task: string; state: string }[] }[]; cancelled: { task: string }[] }[]; inbox: { task: string; question: string | null }[] };

async function status(stack: FactoryStack): Promise<Status> {
  return (await stack.cli("status")) as unknown as Status;
}

test("an empty machine offers only Create Factory, and ⌘⇧F and ⌘K open the Factory (B1, B2, B21)", async ({ page }) => {
  await withStack(page, "factory-empty", async () => {
    const row = page.locator("[data-sidebar-factory]");
    await expect(row).toBeVisible({ timeout: 20_000 });
    // No item, no badge; no Factory, no secretary row (B1, B12).
    await expect(page.locator("[data-factory-badge]")).toHaveCount(0);
    await expect(page.locator("[data-sidebar-secretary]")).toHaveCount(0);

    await page.keyboard.press(chord("factory_open"));
    await expect(page.locator('[data-factory-empty="none"]')).toBeVisible({ timeout: 20_000 });
    await expect(page.locator('[data-factory-empty="none"] button')).toHaveCount(1);
    await expect(row).toHaveAttribute("aria-current", "page");

    await page.locator("[data-sidebar-overview]").click();
    await expect(page.locator("[data-factory-screen]")).toHaveCount(0);
    await page.keyboard.press(chord("search"));
    await page.keyboard.type("Factory");
    await page.locator('[data-palette-row="command:factory-open"]').click();
    await expect(page.locator('[data-factory-empty="none"]')).toBeVisible();
  });
});

test("the create sheet asks three things, writes nothing before Create, and a cancel leaves no Factory (B3-B6)", async ({ page }) => {
  await withStack(page, "factory-create", async (stack) => {
    await openFactory(page);
    await page.locator("[data-factory-create-open]").click();
    const sheet = page.locator("[data-factory-create]");
    await sheet.locator("[data-factory-create-project]").click();
    await page.locator('[data-factory-create-project-option="fixture"]').click();
    // The engine's preview: the verify command it found, a local project with no GitHub step (B3, B4).
    await expect(sheet.locator('[data-factory-create-verification="commands"]')).toBeVisible({ timeout: 30_000 });
    await expect(sheet.locator('[data-factory-create-command="npm test"]')).toBeVisible();
    await expect(sheet.locator("[data-factory-create-github]")).toHaveCount(0);
    await expect(sheet.locator('[data-factory-create-merge="auto"]')).toBeEnabled();
    // No verification: auto cannot be chosen and says why in its place (B4).
    await sheet.locator('[data-factory-create-verification="none"]').click();
    await expect(sheet.locator("[data-factory-create-auto-blocked]")).toBeVisible({ timeout: 30_000 });
    await expect(sheet.locator('[data-factory-create-merge="auto"]')).toBeDisabled();
    await sheet.locator("[data-factory-create-cancel]").click();
    await expect(sheet).toHaveCount(0);
    expect((await status(stack)).factories, "a cancel leaves no Factory (B5)").toEqual([]);

    await page.locator("[data-factory-create-open]").click();
    await sheet.locator("[data-factory-create-project]").click();
    await page.locator('[data-factory-create-project-option="fixture"]').click();
    await expect(sheet.locator('[data-factory-create-verification="commands"]')).toBeChecked({ timeout: 30_000 });
    await expect(sheet.locator("[data-factory-create-confirm]")).toContainText("fixture");
    await sheet.locator("[data-factory-create-confirm]").click();
    await expect(sheet).toHaveCount(0, { timeout: 30_000 });
    // Made: the intake line and no button to add a Task (B6).
    await expect(page.locator('[data-factory-turn-empty="intake"]')).toBeVisible();
    const made = await status(stack);
    expect(made.factories).toHaveLength(1);
    // The secretary row stands under the Factory row once a Factory exists (B23).
    await expect(page.locator("[data-sidebar-secretary]")).toBeVisible();
  });
});

test("Enter answers the top item, the arrows open another, and the badge stays the tab's count (B8-B12)", async ({ page }) => {
  await withStack(page, "factory-turn", async (stack) => {
    await seedDag(stack);
    await openFactory(page);
    const items = page.locator("[data-factory-item]");
    await expect(items).toHaveCount(2);
    await expect(page.locator("[data-factory-badge]")).toHaveText("2");
    await expect(page.locator("[data-factory-turn-count]")).toHaveText("2");
    // The top item opens with the suggestion chosen and the send button focused (B8, B11).
    const first = items.nth(0);
    await expect(first).toHaveAttribute("data-factory-item-open", "true");
    await expect(first.locator('[data-factory-choice="1"]')).toHaveAttribute("aria-checked", "true");
    await expect(first.locator("[data-factory-send]")).toBeFocused();
    const firstKey = await first.getAttribute("data-factory-item");

    await page.keyboard.press("ArrowDown");
    await expect(items.nth(1)).toHaveAttribute("data-factory-item-open", "true");
    await page.keyboard.press("ArrowUp");
    await expect(items.nth(0)).toHaveAttribute("data-factory-item-open", "true");
    await page.keyboard.press("2");
    await expect(items.nth(0).locator('[data-factory-choice="2"]')).toHaveAttribute("aria-checked", "true");
    await page.keyboard.press("1");

    await page.keyboard.press("Enter");
    // Taken: the item leaves and the next opens; the badge and the tab follow (B10, B12).
    await expect(page.locator(`[data-factory-item="${firstKey}"]`)).toHaveCount(0, { timeout: 20_000 });
    await expect(items).toHaveCount(1);
    await expect(items.nth(0)).toHaveAttribute("data-factory-item-open", "true");
    await expect(page.locator("[data-factory-badge]")).toHaveText("1");
    await expect(page.locator("[data-factory-turn-count]")).toHaveText("1");
    const after = await status(stack);
    expect(after.my_turn).toBe(1);
    expect(after.inbox.map((item) => `${after.factories[0]!.id}/${item.task}/${item.question}`)).not.toContain(firstKey);

    // The agents' request view has one line back to the Factory while 내 차례 has items (B13).
    await page.locator("[data-home-destination]").click();
    const main = page.locator("[data-main-screen]");
    await main.locator('[data-main-tab="requests"]').click();
    const line = main.locator("[data-requests-factory-line]");
    await expect(line).toHaveAttribute("data-requests-factory-line", "1");
    await line.getByRole("button").click();
    await expect(page.locator('[data-factory-screen="ready"]')).toBeVisible();
  });
});

test("movement columns match the engine, and a card answer uses the inbox command (board B1, B2, B17, B20)", async ({ page }) => {
  await withStack(page, "factory-board", async (stack) => {
    const dag = await seedDag(stack);
    await openFactory(page);
    await page.locator('[data-factory-tab="board"]').click();
    const before = page.locator('[data-factory-column="before"]');
    await expect(before.locator("[data-factory-card]")).toHaveCount(5);
    await expect(before.locator("[data-factory-card]").first()).toHaveAttribute("data-factory-card-turn", "true");
    expect((await status(stack)).factories[0]!.columns.map((column) => column.column)).toEqual(["before", "moving", "stuck", "done"]);
    // The never-run tasks are all before; filtering widens their own cards.
    await page.locator('[data-factory-flow-cell="before"]').click();
    await expect(page.locator("[data-factory-column]")).toHaveCount(1);
    await expect(before.locator(`[data-factory-card="${dag.c}"] [data-factory-problem]`)).toBeVisible();
    const answer = before.locator(`[data-factory-card="${dag.a}"] [data-factory-card-send]`);
    await answer.click();
    await expect(before.locator(`[data-factory-card="${dag.a}"] [data-factory-card-send]`)).toHaveCount(0, { timeout: 20_000 });
    const after = await status(stack);
    expect(after.inbox.some((item) => item.task === dag.a)).toBe(false);
    expect(after.factories[0]!.columns[0]!.cards.find((card) => card.task === dag.a)?.state).toBe("waiting");
    await page.locator("[data-factory-column-filter]").click();
    await expect(page.locator("[data-factory-column]")).toHaveCount(4);
  });
});

test("a cancel asks nothing, leaves 되살리기 in its place, and the Task is revived from 취소됨 (B16)", async ({ page }) => {
  await withStack(page, "factory-cancel", async (stack) => {
    const dag = await seedDag(stack);
    await openFactory(page);
    await page.locator('[data-factory-tab="board"]').click();
    await page.locator(`[data-factory-card="${dag.c}"]`).click();
    await page.locator('[data-factory-action="cancel"]').click();
    await expect(page.locator(`[data-factory-revive="${dag.c}"]`)).toBeVisible({ timeout: 20_000 });
    await page.locator("[data-factory-back]").click();
    await expect(page.locator(`[data-factory-board] [data-factory-card="${dag.c}"]`)).toHaveCount(0);
    await page.locator("[data-factory-cancelled-filter]").click();
    await page.locator(`[data-factory-revive="${dag.c}"]`).click();
    await expect(page.locator(`[data-factory-cancelled="${dag.c}"]`)).toHaveCount(0, { timeout: 20_000 });
    expect((await status(stack)).factories[0]!.cancelled).toEqual([]);
  });
});

test("the graph draws A to B to C without the A to C arrow, and a node opens its page (B17)", async ({ page }) => {
  await withStack(page, "factory-graph", async (stack) => {
    const dag = await seedDag(stack);
    await openFactory(page);
    await page.locator('[data-factory-tab="graph"]').click();
    const graph = page.locator("[data-factory-graph] [data-dependency-graph]");
    const factory = (await status(stack)).factories[0]!.id;
    const edge = (from: string, to: string) => graph.locator(`[data-dependency-edge="${factory}/${from}>${factory}/${to}"]`);
    await expect(edge(dag.a, dag.b)).toHaveCount(1);
    await expect(edge(dag.b, dag.c)).toHaveCount(1);
    await expect(edge(dag.a, dag.d)).toHaveCount(1);
    await expect(edge(dag.a, dag.c)).toHaveCount(0);
    await expect(graph).toHaveAttribute("data-dependency-edges", "3");
    // The layered layout comes from its worker in a real browser; the columns are only its fallback.
    await expect(graph).toHaveAttribute("data-dependency-layout", "layered");
    const dimensions = await graph.locator("[data-factory-card]").evaluateAll((cards) => cards.map((card) => ({ width: card.getBoundingClientRect().width, height: card.getBoundingClientRect().height })));
    expect(dimensions.every((size) => size.width < 240)).toBe(true);
    expect(new Set(dimensions.map((size) => size.height)).size).toBe(1);
    await expect(edge(dag.a, dag.b)).toHaveCount(1);
    await expect(page.locator(`[data-factory-graph-unrelated] [data-factory-card="${dag.e}"]`)).toBeVisible();
    await graph.locator(`[data-factory-card="${dag.c}"]`).click();
    await expect(page.locator(`[data-factory-task-page="${dag.c}"]`)).toBeVisible();
  });
});

test("a Task page offers only its state's actions and sends its question to 내 차례 (B18, B19)", async ({ page }) => {
  await withStack(page, "factory-task", async (stack) => {
    const dag = await seedDag(stack);
    await openFactory(page);
    const open = async (task: string) => {
      await page.locator('[data-factory-tab="board"]').click();
      await page.locator(`[data-factory-card="${task}"]`).click();
      await expect(page.locator(`[data-factory-task-page="${task}"] [data-factory-task-state]`)).toBeVisible({ timeout: 20_000 });
    };
    await open(dag.b);
    await expect(page.locator("[data-factory-actions]")).toHaveAttribute("data-factory-actions", "priority cancel");
    await expect(page.locator(`[data-factory-chain-card="${dag.a}"]`)).toBeVisible();
    await expect(page.locator(`[data-factory-chain-card="${dag.c}"]`)).toBeVisible();
    await expect(page.locator("[data-factory-verification]")).toHaveAttribute("data-factory-verification", /\/3$/);
    await page.locator("[data-factory-back]").click();

    await open(dag.a);
    await expect(page.locator("[data-factory-actions]")).toHaveAttribute("data-factory-actions", "edit cancel");
    await page.locator("[data-factory-answer-in-turn]").click();
    await expect(page.locator('[data-factory-body="turn"]')).toBeVisible();
    await expect(page.locator(`[data-factory-item^="${(await status(stack)).factories[0]!.id}/${dag.a}/"]`)).toHaveAttribute("data-factory-item-open", "true");

    // Removing a dependency is the person's: C no longer waits on A in the engine (B19).
    await open(dag.c);
    await page.locator(`[data-factory-dep-remove="${dag.a}"]`).click();
    await expect(page.locator(`[data-factory-chain-card="${dag.a}"]`)).toHaveCount(0, { timeout: 20_000 });
  });
});

test("settings show the engine's values, a change reaches the engine, and Close Factory empties the screen (B22)", async ({ page }) => {
  await withStack(page, "factory-settings", async (stack) => {
    await seedDag(stack);
    await openFactory(page);
    await page.locator('[data-factory-tab="settings"]').click();
    const deadline = page.locator('[data-factory-setting="question_deadline_hours"]');
    await expect(deadline).toHaveValue("24", { timeout: 20_000 });
    await deadline.fill("12");
    await deadline.press("Enter");
    await expect(page.locator("[data-factory-settings-write]")).toHaveAttribute("data-factory-settings-write", "taken", { timeout: 20_000 });
    const config = (await stack.cli("config", "--project", stack.project)).config as { question_deadline_ms: number };
    expect(config.question_deadline_ms).toBe(12 * 3_600_000);
    for (const group of ["run", "verification", "merge", "thresholds", "checks", "keep", "autonomy", "advanced"]) {
      await expect(page.locator(`[data-factory-settings-group="${group}"]`)).toBeVisible();
    }
    // No Task runs, so the Factory can be closed; the screen then offers only Create (B1).
    await page.locator("[data-factory-close]").click();
    await expect(page.locator('[data-factory-empty="none"]')).toBeVisible({ timeout: 20_000 });
    expect((await status(stack)).factories.every((view) => view.closed)).toBe(true);
  });
});
