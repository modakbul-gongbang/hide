// Hide's OpenCode plugin, run in Node the way OpenCode runs it: the plugin the
// helper crate generates (`hide_agent_hooks::opencode::plugin_text`), loaded as
// a module, its hooks called with OpenCode 1.18.30's event and hook shapes
// (`tests/fixtures/opencode/events-1.18.30.json`), and a stand-in helper that
// records each call and answers as the test says (PRD opencode-plugin D-13).
//
// `tests/it/opencode_plugin.rs` writes the plugin and the stand-in and runs
// this file with HIDE_OPENCODE_PLUGIN, HIDE_OPENCODE_HELPER_LOG and
// HIDE_OPENCODE_HELPER_ANSWERS set.

import assert from "node:assert/strict";
import { readFileSync, rmSync, writeFileSync } from "node:fs";
import { test } from "node:test";
import { pathToFileURL } from "node:url";

const SAMPLES = JSON.parse(readFileSync(new URL("../fixtures/opencode/events-1.18.30.json", import.meta.url), "utf8"));
const { HidePlugin } = await import(pathToFileURL(process.env.HIDE_OPENCODE_PLUGIN).href);
const LOG = process.env.HIDE_OPENCODE_HELPER_LOG;
const ANSWERS = process.env.HIDE_OPENCODE_HELPER_ANSWERS;

const HERDR = { HERDR_ENV: "1", HERDR_PANE_ID: "w1:p1", HERDR_SOCKET_PATH: "/tmp/herdr-fixture.sock" };
const sample = (name) => structuredClone(SAMPLES[name]);
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** The helper's answers by operation; `delay_ms` holds an operation back. */
function answer(answers) {
  writeFileSync(ANSWERS, JSON.stringify(answers));
}

function calls() {
  try {
    return readFileSync(LOG, "utf8")
      .split("\n")
      .filter(Boolean)
      .map((line) => JSON.parse(line));
  } catch {
    return [];
  }
}

/** Waits until the stand-in logged `count` calls of `operation` that `matches` accepts. */
async function until(operation, count = 1, matches = () => true) {
  for (let waited = 0; waited < 5000; waited += 20) {
    const found = calls().filter((call) => call.operation === operation && matches(call.input));
    if (found.length >= count) return found;
    await sleep(20);
  }
  assert.fail(`no ${operation} call: ${JSON.stringify(calls())}`);
}

/** A client answering `session.get` from `sessions` and `session.status` from `running`. */
function client(sessions = {}, running = {}) {
  return {
    session: {
      get: async ({ path }) => ({ data: sessions[path.id] }),
      status: async () => ({ data: running }),
    },
  };
}

/** Loads the plugin as OpenCode does, in a process whose environment is `env` for Herdr's variables. */
async function plugin(env = HERDR, opencode = client()) {
  rmSync(LOG, { force: true });
  for (const key of Object.keys(HERDR)) delete process.env[key];
  Object.assign(process.env, env);
  return await HidePlugin({ client: opencode, directory: "/checkouts/fixture" });
}

async function prompt(hooks, input = sample("chat.message").input, parts) {
  const output = sample("chat.message").output;
  output.message.sessionID = input.sessionID;
  if (parts) output.parts = parts;
  await hooks["chat.message"](input, output);
  return output;
}

test("outside a Herdr pane the plugin registers no hook and starts nothing", async () => {
  answer({});
  for (const env of [{}, { HERDR_ENV: "1" }, { ...HERDR, HERDR_ENV: "0" }]) {
    const hooks = await plugin(env);
    assert.deepEqual(Object.keys(hooks), []);
  }
  await sleep(100);
  assert.deepEqual(calls(), []);
});

test("a new root session's first prompt carries the guidance and the letters, which are confirmed once OpenCode stored it", async () => {
  answer({
    start: { context: "HIDE-GUIDANCE" },
    prompt: { context: "Hide letter letter-1 from lead (claude) [request]\nBODY-1\n", letters: ["letter-1"] },
    confirm: { confirmed: ["letter-1"] },
    subagents: { reported: true },
  });
  const hooks = await plugin();
  await until("start");
  await hooks.event({ event: sample("session.created") });
  const before = Date.now();
  const output = await prompt(hooks);

  const [call] = await until("prompt");
  assert.equal(call.input.session_id, "ses_root");
  assert.equal(call.input.first, true);
  assert.equal(call.input.prompt, "Fix the failing parser test");
  assert.equal(call.input.cwd, "/checkouts/fixture");
  assert.equal(call.env.HERDR_PANE_ID, "w1:p1");
  assert.equal(output.parts.length, 2);
  const added = output.parts[1];
  assert.equal(added.type, "text");
  assert.equal(added.synthetic, true);
  assert.equal(added.sessionID, "ses_root");
  assert.equal(added.messageID, "msg_fixture");
  assert.match(added.id, /^prt_[0-9a-f]{12}[0-9A-Za-z]{14}$/);
  // OpenCode orders parts by id, whose first 12 hex digits are the low six
  // bytes of (milliseconds * 0x1000 + counter): Hide's sorts after any made before it.
  const earlier = (BigInt(before - 1) * 0x1000n) & 0xffffffffffffn;
  assert.ok(added.id.slice(4, 16) > earlier.toString(16).padStart(12, "0"), added.id);
  assert.match(added.text, /^<system-reminder>\nHIDE-GUIDANCE\n\nHide letter letter-1/);
  assert.ok(added.text.endsWith("</system-reminder>"));
  // Nothing is confirmed before OpenCode wrote the part.
  await sleep(100);
  assert.equal(calls().filter((c) => c.operation === "confirm").length, 0);

  const stored = sample("message.part.updated");
  stored.properties.part = added;
  await hooks.event({ event: stored });
  const [confirm] = await until("confirm");
  assert.deepEqual(confirm.input, { session_id: "ses_root", letters: ["letter-1"] });

  // The next prompt is not the session's first, and nothing waits.
  answer({ prompt: { context: "", letters: [] } });
  const next = await prompt(hooks);
  const prompts = await until("prompt", 2);
  assert.equal(prompts[1].input.first, false);
  assert.equal(next.parts.length, 1);
});

test("a letter whose prompt OpenCode never stored is never confirmed", async () => {
  answer({ start: { context: "" }, prompt: { context: "BODY", letters: ["letter-2"] } });
  const hooks = await plugin();
  await hooks.event({ event: sample("session.created") });
  const output = await prompt(hooks);
  assert.equal(output.parts.length, 2);
  // Another part's write, and none for Hide's, confirm nothing.
  await hooks.event({ event: sample("message.part.updated") });
  await sleep(300);
  assert.equal(calls().filter((c) => c.operation === "confirm").length, 0);
});

test("a child session's prompt and a prompt of only synthetic text get nothing", async () => {
  answer({ start: { context: "HIDE-GUIDANCE" }, prompt: { context: "BODY", letters: ["letter-3"] } });
  const hooks = await plugin();
  await hooks.event({ event: sample("session.created") });
  await hooks.event({ event: sample("session.created.child") });

  const child = await prompt(hooks, { ...sample("chat.message").input, sessionID: "ses_child" });
  assert.equal(child.parts.length, 1);
  const synthetic = await prompt(hooks, undefined, [
    { id: "prt_x", sessionID: "ses_root", messageID: "msg_fixture", type: "text", text: "Summarize", synthetic: true },
  ]);
  assert.equal(synthetic.parts.length, 1);
  await sleep(100);
  assert.equal(calls().filter((c) => c.operation === "prompt").length, 0);
});

test("a session first seen in a prompt is asked about once and a child found that way gets nothing", async () => {
  answer({ start: { context: "" }, prompt: { context: "BODY", letters: [] } });
  const sessions = {
    ses_root: sample("session.created").properties.info,
    ses_child: sample("session.created.child").properties.info,
  };
  const hooks = await plugin(HERDR, client(sessions));
  const child = await prompt(hooks, { ...sample("chat.message").input, sessionID: "ses_child" });
  assert.equal(child.parts.length, 1);
  const root = await prompt(hooks);
  assert.equal(root.parts.length, 2);
});

test("a slow, failing or garbled helper passes the prompt unchanged within its budget", async () => {
  // The helper holds out far past the 1.85 s budget; the bound only proves the prompt did not wait for it.
  for (const prompt_answer of [{ delay_ms: 15000, context: "LATE" }, { exit: 3 }, { raw: "not json" }]) {
    answer({ start: { context: "" }, prompt: prompt_answer });
    const hooks = await plugin();
    await hooks.event({ event: sample("session.created") });
    const started = Date.now();
    const output = await prompt(hooks);
    const spent = Date.now() - started;
    assert.equal(output.parts.length, 1, JSON.stringify(prompt_answer));
    assert.ok(spent < 3500, `${spent} ms for ${JSON.stringify(prompt_answer)}`);
  }
});

test("a shell call that starts an agent through Herdr is refused with the helper's reason, and other calls run", async () => {
  answer({ start: { context: "" }, tool: { deny: "Not run: use hide agent spawn" } });
  const hooks = await plugin();
  const launch = sample("tool.execute.before.bash");
  await assert.rejects(hooks["tool.execute.before"](launch.input, launch.output), {
    message: "Not run: use hide agent spawn",
  });
  const [call] = await until("tool");
  assert.deepEqual(call.input, {
    session_id: "ses_root",
    tool: "bash",
    command: "herdr agent start --kind claude --name helper",
    cwd: "/checkouts/fixture",
  });

  // A call that cannot start an agent never reaches the helper.
  await hooks["tool.execute.before"]({ ...launch.input }, { args: { command: "ls -la" } });
  await hooks["tool.execute.before"]({ ...launch.input, tool: "read" }, { args: { filePath: "/x" } });
  await sleep(100);
  assert.equal(calls().filter((c) => c.operation === "tool").length, 1);
});

test("the question tool is refused only when the helper says the pane is a Factory worker", async () => {
  const question = sample("tool.execute.before.question");
  answer({ start: { context: "" }, tool: { deny: "Ask through hide factory ask" } });
  let hooks = await plugin();
  await assert.rejects(hooks["tool.execute.before"](question.input, question.output), {
    message: "Ask through hide factory ask",
  });
  for (const tool_answer of [{}, { exit: 3 }, { delay_ms: 4000, deny: "late" }]) {
    answer({ start: { context: "" }, tool: tool_answer });
    hooks = await plugin();
    await hooks["tool.execute.before"](question.input, question.output);
  }
});

test("child sessions move the pane's subagent counts, and a background child stays counted after the turn", async () => {
  answer({ start: { context: "" }, prompt: { context: "", letters: [] }, subagents: { reported: true } });
  const running = {};
  const hooks = await plugin(HERDR, client({}, running));
  await hooks.event({ event: sample("session.created") });
  await prompt(hooks);
  await hooks.event({ event: sample("session.created.child") });
  await hooks.event({ event: sample("session.status.busy") });
  await until("subagents", 1, (input) => input.working === 1 && input.done === 0);

  // The root goes idle while the child still runs in the background.
  running.ses_child = { type: "busy" };
  await hooks.event({ event: { type: "session.status", properties: { sessionID: "ses_root", status: { type: "idle" } } } });
  await sleep(200);
  assert.deepEqual(calls().filter((c) => c.operation === "subagents").at(-1).input, { working: 1, done: 0 });

  await hooks.event({ event: sample("session.status.idle") });
  await until("subagents", 1, (input) => input.working === 0 && input.done === 1);
});

test("a subagent's question call asks the Factory guard about the pane's root session", async () => {
  answer({ start: { context: "" }, tool: {} });
  const hooks = await plugin();
  await hooks.event({ event: sample("session.created") });
  await hooks.event({ event: sample("session.created.child") });
  const question = sample("tool.execute.before.question");
  await hooks["tool.execute.before"]({ ...question.input, sessionID: "ses_child" }, question.output);
  const [call] = await until("tool");
  assert.deepEqual(call.input, { session_id: "ses_root", tool: "question" });
});

test("past its session limit the plugin forgets finished children, keeps a running one counted and guides once", async () => {
  answer({ start: { context: "HIDE-GUIDANCE" }, prompt: { context: "", letters: [] }, subagents: { reported: true } });
  const running = { ses_long: { type: "busy" } };
  const hooks = await plugin(HERDR, client({}, running));
  await until("start");
  await hooks.event({ event: sample("session.created") });
  await prompt(hooks);
  const child = (id) => ({ type: "session.created", properties: { sessionID: id, info: { ...sample("session.created.child").properties.info, id } } });
  const status = (id, type) => ({ type: "session.status", properties: { sessionID: id, status: { type } } });
  // A background subagent that runs all along, then more finished subagents than the plugin keeps.
  await hooks.event({ event: child("ses_long") });
  await hooks.event({ event: status("ses_long", "busy") });
  // OpenCode publishes events without waiting for a plugin's handling, as here.
  for (let index = 0; index < 560; index += 1) {
    void hooks.event({ event: child(`ses_old${index}`) });
    void hooks.event({ event: status(`ses_old${index}`, "busy") });
    void hooks.event({ event: status(`ses_old${index}`, "idle") });
  }
  // The turn ends: the event settles once its report is sent, so the last report is the state now.
  await hooks.event({ event: { type: "session.status", properties: { sessionID: "ses_root", status: { type: "idle" } } } });
  const reports = calls().filter((call) => call.operation === "subagents");
  assert.deepEqual(reports.at(-1).input, { working: 1, done: 510 });
  // A burst of changes costs a few reports, not one per change.
  assert.ok(reports.length < 50, `${reports.length} reports`);

  // A subagent started now is still counted.
  await hooks.event({ event: child("ses_new") });
  await hooks.event({ event: status("ses_new", "busy") });
  assert.deepEqual(calls().filter((call) => call.operation === "subagents").at(-1).input, { working: 2, done: 509 });

  // The root's guidance was given once; its next prompt is not its first.
  const next = await prompt(hooks);
  const prompts = await until("prompt", 2);
  assert.equal(prompts[1].input.first, false);
  assert.equal(next.parts.length, 1);
});
