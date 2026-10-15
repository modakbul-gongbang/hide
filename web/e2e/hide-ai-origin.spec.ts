// What Hide AI started, told from what the operator started, on a real core
// through an isolated pinned Herdr, hided and browser (PRD hide-ai-origin-mark
// B5, B12, B13; PRD hide-ai-work-home B1, B3, B20). The fixture is a scratch repository with no remote;
// the ledger is seeded in the daemon's own state folder while it is down, the
// way a Memory promotion's spawn leaves it, and never an operator's.

import { expect, test, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { sessionOf, spawnAgent, startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";

test.describe.configure({ timeout: 150_000 });

const digest = (value: string) => createHash("sha256").update(value).digest("hex");
/** The operator's git configuration (signing, hooks) never reaches the scratch repository. */
const git = (cwd: string, ...args: string[]) =>
  execFileSync("git", ["-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null", ...args], {
    cwd,
    env: { ...process.env, GIT_CONFIG_GLOBAL: "/dev/null", GIT_CONFIG_NOSYSTEM: "1" },
    stdio: ["ignore", "ignore", "inherit"],
  });

/** The coordination record Hide leaves when it starts an agent for a card: a handoff whose origin is its own identity. */
function ledgerOf(node: string, started: { pane: string; session: string; path: string; branch: string; task: { kind: string; target: string } | null }[]): string {
  const hide = {
    id: "agent-1", name: "hide:memory", machine: node, host_scope: "hide", native_machine: "hide", session: "hide:memory", instance: "hide:memory", pane: "hide:memory",
    parent: null, origin: null, project: null, ended: false, forked: false,
    actor: { pane_id: "hide:memory", name: "hide:memory", kind: "hide", device_id: node, session: digest("hide:memory") },
  };
  const agents = [hide];
  const spawns: object[] = [];
  started.forEach((one, index) => {
    const name = `hide-ai-${index}`;
    const id = `agent-${index + 2}`;
    agents.push({
      id, name, machine: node, host_scope: "local", native_machine: node, session: one.session, instance: one.pane, pane: one.pane,
      parent: null, origin: "agent-1", project: one.path, ended: false, forked: false,
      actor: { pane_id: one.pane, name, kind: "claude", device_id: node, session: digest(one.session) },
    } as never);
    spawns.push({
      id: `spawn-${index + 20}`, parent: "agent-1", mode: "handoff", machine: null, intent: `memory-promote-${index}`, name, kind: "claude", repo: path.dirname(one.path),
      branch: one.branch, requested_path: null, args: [], path: one.path, pane: one.pane, terminal: null, child: id, completed: true, auto_watch_id: null,
      ...(one.task ? { task: one.task } : {}),
    });
  });
  return JSON.stringify({ version: 1, next_id: 100, letters: [], watches: [], agents, spawns, node_clocks: {} });
}

type Stack = { herdr: HerdrFixture; daemon: Daemon; repo: string; operatorPane: string; hideAiPane: string; hideAiPath: string };

/** A project whose main checkout has the operator's agent, a worktree with an agent Hide started, and a worktree the operator made whose branch is named like Hide AI's. */
async function start(): Promise<Stack> {
  const herdr = await startHerdr({ agents: false });
  fs.mkdirSync(path.join(herdr.root, "scratch"));
  const repo = fs.realpathSync(path.join(herdr.root, "scratch"));
  git(repo, "init", "-q", "-b", "main");
  git(repo, "-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "commit", "-q", "--allow-empty", "-m", "first");
  const hideAiPath = `${repo}.worktrees/hide-memory-m43-fixture`;
  const lookalikePath = `${repo}.worktrees/hide-memory-m9-by-hand`;
  git(repo, "worktree", "add", "-q", "-b", "hide-memory-m43-fixture", hideAiPath);
  git(repo, "worktree", "add", "-q", "-b", "hide-memory-m9-by-hand", lookalikePath);
  const operatorPane = await spawnAgent(herdr, "scratch", null, repo, { task: "사이드바 가독성 개선" });
  const hideAiPane = await spawnAgent(herdr, "m43", null, hideAiPath, { task: "m43 규칙을 문서로 옮기는 중" });
  await spawnAgent(herdr, "m9", null, lookalikePath, { task: "m9 손으로 만든 작업" });
  const daemon = await startHided(herdr, "hide-ai-origin");
  return { herdr, daemon, repo, operatorPane, hideAiPane, hideAiPath };
}

async function open(page: Page, daemon: Daemon): Promise<void> {
  await page.goto("about:blank");
  await page.goto(`${daemon.origin}/#token=${daemon.token}`);
}

/** Restarts the daemon with Hide's record of its start in the ledger, as a promotion leaves it. */
async function seed(stack: Stack, task: { kind: string; target: string } | null): Promise<void> {
  const session = sessionOf(stack.herdr, stack.hideAiPane);
  stack.daemon = await stack.daemon.restart((dir) => {
    const file = path.join(dir, "delivery-ledger.json");
    fs.writeFileSync(file, ledgerOf(stack.daemon.node, [{ pane: stack.hideAiPane, session, path: stack.hideAiPath, branch: "hide-memory-m43-fixture", task }]), { mode: 0o600 });
  });
}

test("an agent Hide AI started for a home leaves the project tree with no head, and a branch named like Hide AI's stays the operator's", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const stack = await start();
  try {
    await open(page, stack.daemon);
    const project = page.locator("[data-project]", { hasText: "scratch" });
    await expect(project.locator("[data-checkout-row]").first()).toBeVisible({ timeout: 20_000 });
    // Before Hide AI's record exists, every checkout is the operator's and its agent is drawn (B5).
    await expect(project.locator("[data-checkout-row]", { hasText: "hide-memory-m43" })).toHaveCount(1);
    await expect(page.locator(`[data-pane="${stack.hideAiPane}"]`)).toHaveCount(1);

    await seed(stack, { kind: "memory_promote", target: "m43" });
    await open(page, stack.daemon);
    // The head is gone, and so is the agent's row: its home, the Memory tab, shows its work (B1).
    await expect(page.locator(`[data-pane="${stack.hideAiPane}"]`)).toHaveCount(0, { timeout: 30_000 });
    await expect(page.locator("[data-hide-ai-group]")).toHaveCount(0);
    // Nothing of the operator's is in the checkout Hide AI made and it is not in front, so the tree does not draw it (B3).
    await expect(project.locator("[data-checkout-row]", { hasText: "hide-memory-m43" })).toHaveCount(0);
    // The operator's checkouts, the lookalike branch included, and the operator's agent stay.
    await expect(project.locator("[data-checkout-row]", { hasText: "hide-memory-m9-by-hand" })).toHaveCount(1);
    await expect(page.locator(`[data-pane="${stack.operatorPane}"]`)).toHaveCount(1);
    await expect(page.locator(`[data-pane="${stack.operatorPane}"] [data-agent-task]`)).toHaveCount(0);
  } finally {
    stack.daemon.stop();
    stack.herdr.stop();
  }
});

test("a start whose kind this build has no home for stays an operator's row naming its task, and it all holds across a restart of hided", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const stack = await start();
  try {
    // A record with no task, or one a newer build wrote, loads with the rest of the ledger (B20).
    await seed(stack, null);
    await open(page, stack.daemon);
    const row = page.locator(`[data-pane="${stack.hideAiPane}"]`);
    await expect(row.locator("[data-agent-task]")).toHaveText("Hide AI", { timeout: 30_000 });

    stack.daemon = await stack.daemon.restart((dir) => {
      const file = path.join(dir, "delivery-ledger.json");
      const session = sessionOf(stack.herdr, stack.hideAiPane);
      fs.writeFileSync(file, ledgerOf(stack.daemon.node, [{ pane: stack.hideAiPane, session, path: stack.hideAiPath, branch: "hide-memory-m43-fixture", task: { kind: "future_kind", target: "m43" } }]), { mode: 0o600 });
    });
    await open(page, stack.daemon);
    await expect(row.locator("[data-agent-task]")).toHaveText("future_kind · m43", { timeout: 30_000 });
    await expect(page.locator("[data-hide-ai-group]")).toHaveCount(0);
  } finally {
    stack.daemon.stop();
    stack.herdr.stop();
  }
});
