// A private stack with a Factory on a local Git project (PRD
// software-factory-ui), made with the stage-1 `hide factory` CLI from a pane
// of the private Herdr, as a person or their agent makes one. The engine's
// judgments go to the fixture `claude`, which answers every one with the
// line the spec last wrote to `answerFile`, so a Task's review is decided by
// the spec. Starts are held by a disk floor no machine meets, so no worker
// ever starts and every state the spec puts a Task in stays put.

import { expect, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fixtureExecutable } from "./platform-fixture";
import { runInPane, sessionOf, startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { copyFixtureShim } from "./shims/build";

export type FactoryStack = {
  herdr: HerdrFixture;
  daemon: Daemon;
  /** The fixture project's folder, a Git repository with a `test` script. */
  project: string;
  /** Runs `hide factory <args> --json` in the fixture pane and returns its answer. */
  cli: (...args: string[]) => Promise<Record<string, unknown>>;
  /** What the engine's next judgments answer. */
  review: (answer: Review) => void;
  stop: () => void;
};

type Question = { text: string; suggestion: string; default_action: string };
export type Review = { summary: string; questions: Question[]; dependencies: string[]; split: []; flags: string[]; fits_scope: null };

/** A review that leaves nothing open: the Task is Ready. */
export const READY: Review = { summary: "Fixture task summary", questions: [], dependencies: [], split: [], flags: [], fits_scope: null };

/** A review that asks one question, so the Task drafts with an item in 내 차례. */
export function asking(text: string, suggestion: string, defaultAction: string): Review {
  return { ...READY, questions: [{ text, suggestion, default_action: defaultAction }] };
}

let sequence = 0;

export async function startFactoryStack(page: Page, label: string): Promise<FactoryStack> {
  await page.setViewportSize({ width: 1440, height: 900 });
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | null = null;
  try {
    const project = path.join(herdr.root, "fixture");
    const git = (...args: string[]) => execFileSync("git", ["-C", project, ...args], { encoding: "utf8", env: { ...process.env, GIT_AUTHOR_NAME: "fixture", GIT_AUTHOR_EMAIL: "fixture@example.invalid", GIT_COMMITTER_NAME: "fixture", GIT_COMMITTER_EMAIL: "fixture@example.invalid" } });
    fs.writeFileSync(path.join(project, "package.json"), JSON.stringify({ name: "fixture", scripts: { test: "true" } }));
    git("init", "-q", "-b", "main");
    git("add", ".");
    git("commit", "-q", "-m", "fixture");
    const answerFile = path.join(herdr.root, "judgment.json");
    fs.writeFileSync(answerFile, JSON.stringify(READY));
    daemon = await startHided(herdr, label, undefined, { HIDE_E2E_PROVIDER_ANSWER: answerFile });
    const started = daemon;
    const hide = path.resolve("..", "target", "debug", fixtureExecutable("hide"));
    await page.goto(`${started.origin}/#token=${started.token}`);
    // The pane's CLI is bound to its checkout, which the core registers from
    // Herdr's workspace; a `hide factory` before the sidebar lists it is refused.
    await expect(page.locator("[data-checkout]").first()).toBeVisible({ timeout: 20_000 });
    return {
      herdr,
      daemon: started,
      project: fs.realpathSync(project),
      cli: async (...args) => {
        sequence += 1;
        const ran = await runInPane(herdr, herdr.panes[0], `factory-${sequence}`, { env: { HIDE_STATE_DIR: started.stateDir }, argv: [hide, "factory", ...args, "--json"], stdout: true });
        expect(ran.stdout, ran.stderr).not.toBe("");
        return JSON.parse(ran.stdout) as Record<string, unknown>;
      },
      review: (answer) => fs.writeFileSync(answerFile, JSON.stringify(answer)),
      stop: () => {
        started.stop();
        herdr.stop();
      },
    };
  } catch (error) {
    daemon?.stop();
    herdr.stop();
    throw error;
  }
}

/** The ids the seeded DAG's Tasks got, by the name the spec gave them. */
export type Dag = { a: string; b: string; c: string; d: string; e: string };

/**
 * One Factory with the DAG every spec reads: A asks a question; B waits on
 * A; C waits on B and on A (an edge the graph leaves out); D waits on A and
 * asks a question; E stands alone, Ready.
 */
export async function seedDag(stack: FactoryStack): Promise<Dag> {
  const init = await stack.cli("init", stack.project, "--verify", "npm test", "--merge", "manual", "--confirm");
  expect(init.ok, JSON.stringify(init)).toBe(true);
  const held = await stack.cli("config", "--project", stack.project, "--set", "disk_floor_gb=1000000");
  expect(held.ok, JSON.stringify(held)).toBe(true);
  const add = async (title: string, review: Review, after: string[] = []) => {
    stack.review(review);
    const answer = await stack.cli("add", "--project", stack.project, "--title", title, "--goal", `${title}의 목표`, "--criterion", `${title}이 끝난다`, ...after.flatMap((task) => ["--after", task]));
    // The fixture's answer decided the review, so `add` answers it at once rather than `pending`.
    expect(answer.result, JSON.stringify(answer)).toBe(review.questions.length > 0 ? "needs_answers" : "ready");
    return (answer.task as { id: string }).id;
  };
  const a = await add("정렬 API 응답 형식", asking("응답에 updated_at을 넣을까요?", "넣는다", "넣고 진행"));
  const b = await add("보드에 정렬 추가", READY, [a]);
  const c = await add("보드에서 Task 상세 열기", READY, [b, a]);
  const d = await add("정렬 상태 기억", asking("정렬을 어디에 둘까요?", "ui_state", "ui_state에 둔다"), [a]);
  const e = await add("문서 링크 정리", READY);
  stack.review(READY);
  return { a, b, c, d, e };
}

/** Opens the Factory screen from the sidebar and waits for the engine's summary. */
export async function openFactory(page: Page): Promise<void> {
  await page.locator("[data-sidebar-factory]").click();
  await expect(page.locator('[data-factory-screen="ready"]')).toBeVisible({ timeout: 20_000 });
}

export type QuestionHookReplies = Record<"AskUserQuestion" | "ExitPlanMode", string>;

/**
 * Opts this private stack's interactive Claude panes into the compiled
 * spawn-provider runner, retaining claude-shim's normal judgment replies.
 * Its bounded owned child invokes the actual hook from the actual pane.
 * No Factory state is seeded: the spec must init, configure and add a Task.
 */
export function factoryQuestionFixture(stack: FactoryStack): {
  hooks: (pane: string) => Promise<QuestionHookReplies>;
  state: (pane: string, state: "working" | "idle") => Promise<void>;
} {
  const root = stack.herdr.root;
  const runner = path.join(root, "worker-bin", fixtureExecutable("claude"));
  fs.mkdirSync(path.dirname(runner));
  copyFixtureShim("spawn-provider", runner);
  fs.writeFileSync(path.join(root, "factory-question-runner.config"), runner);
  const command = path.join(root, "factory-question-command.json");
  const completed = path.join(root, "factory-question-command.status");
  const replies = path.join(root, "factory-question-replies.json");
  const driver = path.join(root, "factory-question-driver.cjs");
  const hook = path.resolve("..", "target", "debug", fixtureExecutable("hide-agent-hooks"));
  fs.writeFileSync(driver, `
const fs = require("node:fs");
const { execFileSync } = require("node:child_process");
const request = JSON.parse(fs.readFileSync(${JSON.stringify(command)}, "utf8"));
if (request.pane !== process.env.HERDR_PANE_ID) throw new Error("question fixture pane mismatch");
if (request.state) {
  process.stdout.write(request.state === "working" ? "\\x1b]0;⠋ Working\\x07" : "\\x1b]0;✳ Claude Code\\x07");
} else {
  const replies = {};
  for (const tool_name of ["AskUserQuestion", "ExitPlanMode"]) {
    const tool_input = tool_name === "AskUserQuestion"
      ? { questions: [{ question: "Choose a target", header: "Target", options: [{ label: "Preview", description: "Review deployment" }, { label: "Production", description: "Public deployment" }], multiSelect: false }] }
      : { plan: "Use the preview deployment" };
    replies[tool_name] = execFileSync(${JSON.stringify(hook)}, ["hook", "--runtime", "claude-code", "--event", "PreToolUse"], {
      env: { ...process.env, HIDE_STATE_DIR: ${JSON.stringify(stack.daemon.stateDir)} },
      input: JSON.stringify({ session_id: request.session, tool_name, tool_input }),
      encoding: "utf8", timeout: 10000, maxBuffer: 65536,
    });
  }
  fs.writeFileSync(${JSON.stringify(replies)}, JSON.stringify(replies));
}
`);
  fs.writeFileSync(path.join(root, "provider.config"), [stack.herdr.bin, completed, process.execPath, driver, ""].join("\n"));
  const invoke = async (pane: string, state?: "working" | "idle") => {
    fs.writeFileSync(command, JSON.stringify({ pane, session: sessionOf(stack.herdr, pane), state }));
    fs.rmSync(completed, { force: true });
    // Native send-text succeeds with empty stdout, unlike the JSON queries
    // HerdrFixture.run reads. Keep this one bounded send on its private env.
    execFileSync(stack.herdr.bin, ["pane", "send-text", pane, "!"], { env: stack.herdr.env, timeout: 10_000 });
    await expect.poll(() => fs.existsSync(completed) ? fs.readFileSync(completed, "utf8").trim() : null,
      { message: `owned question command completed in ${pane}`, timeout: 20_000 }).toBe("0");
  };
  return {
    hooks: async (pane) => {
      await invoke(pane);
      return JSON.parse(fs.readFileSync(replies, "utf8")) as QuestionHookReplies;
    },
    state: async (pane, state) => {
      await invoke(pane, state);
      await expect.poll(() => {
        const listed = stack.herdr.run(["agent", "list"]) as { result: { agents: { pane_id: string; agent_status: string }[] } };
        return listed.result.agents.find((agent) => agent.pane_id === pane)?.agent_status;
      }, { message: `native question fixture state ${state}`, timeout: 10_000 }).toMatch(state === "working" ? /^working$/ : /^(idle|done)$/);
    },
  };
}
