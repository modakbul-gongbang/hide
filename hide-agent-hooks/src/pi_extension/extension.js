// Hide's extension for Pi and omp, written by Hide's install kit (hide-agent-hooks).
// An edit is kept and shown as edited in Settings; Reinstall puts Hide's back.
// Outside a Herdr pane, in an agent started from another Pi's or omp's shell, or when its helper is gone, it does
// nothing; inside one it acts for the pane's own TUI session, and its spawn guard for every session in it.
// @ts-nocheck

import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { statSync } from "node:fs";

const HELPER = __HIDE_HELPER__;
const AGENT = __HIDE_AGENT__;
const VERSION = __HIDE_VERSION__;

// The prompt budget is Claude Code's prompt hook's; the tool budget is the spawn guard's. Both hosts wait for
// a handler far longer (omp gives up at 30 s and then blocks the tool), so these are the limits.
const PROMPT_BUDGET_MS = 1850;
const TOOL_BUDGET_MS = 2500;
const START_BUDGET_MS = 8000;
const CONFIRM_BUDGET_MS = 2000;
const COUNT_BUDGET_MS = 2000;
const OUTPUT_LIMIT = 64 * 1024;
// Far inside the helper's 256 KiB input, so a pasted log never costs a prompt its letters.
const PROMPT_TEXT_LIMIT = 32 * 1024;
const RUNNING_LIMIT = 8;
const SESSION_LIMIT = 512;
const CHILD_LIMIT = 64;
const PENDING_LIMIT = 16;
const PENDING_AGE_MS = 10 * 60 * 1000;
const CUSTOM_TYPE = "hide";
// Any spelling of the reminder tag the model reads as one, opening or closing.
const REMINDER_TAG = /<(\s*\/?\s*system-reminder)/gi;

// omp's subagents run in the same process and get their own binding of this factory, sharing the module: the
// counts live here, and the main session's binding reports them.
const shared = { children: new Map(), publish: null, rootSession: null };

function managed(env) {
  if (env.HERDR_ENV !== "1" || !env.HERDR_PANE_ID || !env.HERDR_SOCKET_PATH) return false;
  // omp marks every shell it starts with OMPCODE=1 and Pi with PI_SESSION_ID: an agent started there is not
  // the pane's agent, and its prompts must not take the pane's letters.
  if (env.OMPCODE === "1" || env.PI_SESSION_ID) return false;
  try {
    return statSync(HELPER).isFile();
  } catch {
    return false;
  }
}

/** The helper's answer, or null when it failed, timed out or said nothing usable. */
function helper(state, operation, input, budgetMs) {
  if (state.closed && operation !== "confirm" && operation !== "subagents") return Promise.resolve(null);
  if (state.running.size >= RUNNING_LIMIT) {
    state.lost += 1;
    return Promise.resolve(null);
  }
  // A call skipped at the cap or given up on reaches Hide's diagnostic through the next one.
  input = { ...input, version: VERSION };
  if (state.lost > 0) {
    input.lost = state.lost;
    state.lost = 0;
  }
  return new Promise((resolve) => {
    let child;
    try {
      child = spawn(HELPER, [AGENT, operation], { stdio: ["pipe", "pipe", "ignore"] });
    } catch {
      state.lost += 1;
      resolve(null);
      return;
    }
    child.hideOperation = operation;
    state.running.add(child);
    const chunks = [];
    let size = 0;
    let settled = false;
    let timer = null;
    const finish = (value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      state.running.delete(child);
      resolve(value);
    };
    // A throw in a timer would end omp's session: every callback here catches its own.
    timer = setTimeout(() => {
      try {
        child.kill("SIGKILL");
      } catch {}
      state.lost += 1;
      finish(null);
    }, budgetMs);
    try {
      child.on("error", () => {
        if (!settled) state.lost += 1;
        finish(null);
      });
      child.stdout.on("error", () => finish(null));
      child.stdout.on("data", (chunk) => {
        size += chunk.length;
        if (size > OUTPUT_LIMIT) {
          try {
            child.kill("SIGKILL");
          } catch {}
          finish(null);
          return;
        }
        chunks.push(chunk);
      });
      child.on("close", () => {
        try {
          finish(JSON.parse(Buffer.concat(chunks).toString("utf8")));
        } catch {
          finish(null);
        }
      });
      child.stdin.on("error", () => {});
      child.stdin.end(JSON.stringify(input));
    } catch {
      try {
        child.kill("SIGKILL");
      } catch {}
      finish(null);
    }
  });
}

function withBudget(promise, budgetMs) {
  return new Promise((resolve) => {
    const timer = setTimeout(() => resolve(null), Math.max(0, budgetMs));
    Promise.resolve(promise).then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      () => {
        clearTimeout(timer);
        resolve(null);
      },
    );
  });
}

/** The start of a prompt, enough for Memory's retrieval, cut where JSON can still carry it. */
function clip(text) {
  if (typeof text !== "string") return "";
  // A lone surrogate anywhere would make the JSON the helper reads invalid.
  if (typeof text.toWellFormed === "function") text = text.toWellFormed();
  if (text.length <= PROMPT_TEXT_LIMIT) return text;
  const end = PROMPT_TEXT_LIMIT - (/[\uD800-\uDBFF]/.test(text[PROMPT_TEXT_LIMIT - 1]) ? 1 : 0);
  return text.slice(0, end);
}

/** Whether `ctx` is one of omp's subagent sessions. */
function subagent(ctx) {
  return ctx?.agent?.kind === "sub";
}

/**
 * Whether `ctx` is the pane's own agent: the host's TUI, the one session drawn in the pane. Both hosts' RPC modes
 * report a UI too but draw nothing there, and another program in the pane may start one.
 */
function root(ctx) {
  return !subagent(ctx) && ctx?.mode === "tui";
}

/** The session file Herdr's integration reports for the pane, or null for an unsaved session. */
function sessionFile(ctx) {
  try {
    const file = ctx?.sessionManager?.getSessionFile?.();
    return typeof file === "string" && file.startsWith("/") ? file : null;
  } catch {
    return null;
  }
}

/** The host's own id for the session, which Memory knows it by, or null. */
function nativeSession(ctx) {
  try {
    const id = ctx?.sessionManager?.getSessionId?.();
    return typeof id === "string" && id.length > 0 && id.length <= 4096 ? id : null;
  } catch {
    return null;
  }
}

/** Records `key` as the newest member of a Set of at most SESSION_LIMIT, dropping the oldest. */
function remember(set, key) {
  set.delete(key);
  if (set.size >= SESSION_LIMIT) set.delete(set.values().next().value);
  set.add(key);
}

/**
 * Sends the pane's subagent counts when they changed, or always when `force`. One report runs at a time and only
 * the newest waiting one follows it, so a burst of subagents costs two helper runs, not one per change.
 */
function publishCounts(state, force) {
  let working = 0;
  let done = 0;
  for (const child of shared.children.values()) {
    if (child.busy) working += 1;
    else if (child.ran) done += 1;
  }
  const key = `${working}/${done}`;
  if (!force && key === state.published) return state.counting ?? Promise.resolve();
  // Only a report Herdr took counts as published: a failed one is sent again with the next change, prompt or rest.
  state.published = key;
  state.nextCount = { working, done, key };
  state.counting ??= (async () => {
    try {
      while (state.nextCount) {
        const next = state.nextCount;
        state.nextCount = null;
        const answer = await helper(state, "subagents", { working: next.working, done: next.done }, COUNT_BUDGET_MS);
        if (answer?.reported !== true && state.published === next.key) state.published = null;
      }
    } finally {
      state.counting = null;
    }
  })();
  return state.counting;
}

/** One subagent's line: a spawn the parent dispatched, then its own session's turns. */
function child(id) {
  let entry = shared.children.get(id);
  if (entry) return entry;
  if (shared.children.size >= CHILD_LIMIT) {
    // A child still running is never forgotten: it would leave the working count while it works.
    const done = [...shared.children].find(([, line]) => !line.busy);
    if (!done) return null;
    shared.children.delete(done[0]);
  }
  entry = { busy: false, ran: false };
  shared.children.set(id, entry);
  return entry;
}

function setChild(id, busy) {
  if (typeof id !== "string" || !id) return;
  const entry = child(id);
  if (!entry) return;
  entry.busy = busy;
  entry.ran ||= busy;
  void shared.publish?.(false);
}

async function onPrompt(state, event, ctx) {
  if (!root(ctx)) return undefined;
  const session = sessionFile(ctx);
  if (!session) return undefined;
  shared.rootSession = session;
  const deadline = Date.now() + PROMPT_BUDGET_MS;
  // Until the host writes a message that carried it, a session's prompts carry the start guidance, as Claude Code's
  // SessionStart does for a new or resumed session.
  const first = !state.guided.has(session);
  if (first) state.guidance ??= startGuidance(state, ctx);
  const answer = await helper(
    state,
    "prompt",
    { session_id: session, native_session: nativeSession(ctx), prompt: clip(event?.prompt), cwd: ctx?.cwd, first },
    Math.max(0, deadline - Date.now()),
  );
  const sections = [];
  let guided = false;
  if (first && answer !== null) {
    // Guidance still on its way is given to the next prompt instead.
    const guidance = await withBudget(state.guidance, deadline - Date.now());
    if (typeof guidance?.context === "string" && guidance.context) {
      sections.push(guidance.context);
      guided = true;
    }
  }
  if (typeof answer?.context === "string" && answer.context) sections.push(answer.context);
  if (sections.length === 0) return undefined;
  // omp can prepare one submission more than once and keeps only the attempt it delivers: each attempt carries its
  // own id, and only the one whose message the host writes is confirmed.
  const id = randomBytes(8).toString("hex");
  const letters = Array.isArray(answer?.letters) ? answer.letters.filter((letter) => typeof letter === "string") : [];
  const now = Date.now();
  for (const [key, entry] of state.pending) {
    if (now - entry.at > PENDING_AGE_MS) state.pending.delete(key);
  }
  // A letter left out stays pending in Hide and reaches the next prompt, and guidance left out comes again.
  if (state.pending.size >= PENDING_LIMIT) state.pending.delete(state.pending.keys().next().value);
  state.pending.set(id, { session, letters, guided, at: now });
  // Hidden: the host keeps it out of the conversation on screen and out of the title, and the reminder tags tell the
  // model it is not the operator's text. Hide writes those tags itself, so a letter cannot close them.
  const text = `<system-reminder>\n${sections.join("\n\n").replace(REMINDER_TAG, "<\u200b$1")}\n</system-reminder>`;
  return { message: { customType: CUSTOM_TYPE, content: text, display: false, details: { hide: id } } };
}

/** A message the host finished: Hide's hidden message is written, and the reply after it means the turn is recorded. */
function onMessageEnd(state, event, ctx) {
  if (!root(ctx)) return;
  const message = event?.message;
  if (message?.role === "custom" && message.customType === CUSTOM_TYPE) {
    const id = message.details?.hide;
    const pending = typeof id === "string" ? state.pending.get(id) : null;
    if (pending) {
      state.pending.delete(id);
      if (pending.guided) remember(state.guided, pending.session);
      if (pending.letters.length > 0) state.written.push(pending);
    }
    return;
  }
  if (message?.role !== "assistant" || state.written.length === 0) return;
  // The host wrote the prompt and Hide's message before the reply: only now are the letters received.
  const written = state.written;
  state.written = [];
  for (const { session, letters } of written) {
    const fresh = letters.filter((letter) => !state.confirmed.has(letter) && !state.confirming.has(letter));
    if (fresh.length === 0) continue;
    for (const letter of fresh) state.confirming.add(letter);
    // Only a confirmation Hide answered counts: a letter it did not take stays pending there, rides the next prompt
    // again and is confirmed then.
    void helper(state, "confirm", { session_id: session, letters: fresh }, CONFIRM_BUDGET_MS).then((answer) => {
      const confirmed = Array.isArray(answer?.confirmed) ? answer.confirmed : [];
      for (const letter of fresh) {
        state.confirming.delete(letter);
        if (confirmed.includes(letter)) remember(state.confirmed, letter);
      }
    });
  }
}

async function onTool(state, event, ctx) {
  const tool = event?.toolName;
  let request;
  if (tool === "bash") {
    const command = event?.input?.command;
    // Every shell call comes here: only one that can start an agent through Herdr asks the helper.
    if (typeof command !== "string" || !(command.includes("herdr") || command.includes("HERDR_BIN_PATH"))) return null;
    const cwd = typeof event.input.cwd === "string" && event.input.cwd ? event.input.cwd : ctx?.cwd;
    request = { tool, command, cwd };
  } else if (tool === "ask" && AGENT === "omp") {
    // The core knows the pane by the root session Herdr's integration reports, also for a subagent's call.
    const session = root(ctx) ? sessionFile(ctx) : shared.rootSession;
    if (!session) return null;
    request = { session_id: session, tool };
  } else {
    return null;
  }
  const answer = await helper(state, "tool", request, TOOL_BUDGET_MS);
  return typeof answer?.deny === "string" && answer.deny ? answer.deny : null;
}

/** The session guidance, read once; a read that failed is asked again by the next prompt that needs it. */
function startGuidance(state, ctx) {
  const reading = helper(state, "start", { cwd: ctx?.cwd }, START_BUDGET_MS).then((answer) => {
    if (answer === null && state.guidance === reading) state.guidance = null;
    return answer;
  });
  return reading;
}

function onSessionStart(state, ctx) {
  if (subagent(ctx)) {
    setChild(ctx.agent.id, true);
    return;
  }
  if (!root(ctx)) return;
  state.root = true;
  shared.rootSession = sessionFile(ctx);
  state.guidance ??= startGuidance(state, ctx);
  if (AGENT === "omp") {
    // A new session in the pane starts its counts at zero, as Claude Code's SessionStart does.
    shared.children.clear();
    shared.publish = (force) => publishCounts(state, force);
    void publishCounts(state, true);
  }
}

/** Runs `handler` so that nothing it throws or rejects with reaches the host. */
function quiet(handler) {
  return async (event, ctx) => {
    try {
      return await handler(event, ctx);
    } catch {
      return undefined;
    }
  };
}

export default function hide(pi) {
  if (!managed(process.env) || typeof pi?.on !== "function") return;
  const state = {
    running: new Set(),
    lost: 0,
    closed: false,
    root: false,
    guidance: null,
    guided: new Set(),
    pending: new Map(),
    written: [],
    confirmed: new Set(),
    confirming: new Set(),
    published: null,
    counting: null,
    nextCount: null,
  };
  const on = (name, handler) => {
    try {
      pi.on(name, quiet(handler));
    } catch {}
  };
  on("session_start", (_event, ctx) => onSessionStart(state, ctx));
  // omp reports a session the operator switched to with its own event.
  on("session_switch", (_event, ctx) => onSessionStart(state, ctx));
  // Past the budget the prompt goes on unchanged, and letters it would have carried stay pending in Hide.
  on("before_agent_start", async (event, ctx) => (await withBudget(onPrompt(state, event, ctx), PROMPT_BUDGET_MS)) ?? undefined);
  on("message_end", (event, ctx) => onMessageEnd(state, event, ctx));
  on("tool_call", async (event, ctx) => {
    let reason = null;
    try {
      reason = await onTool(state, event, ctx);
    } catch {}
    // A refusal is the one answer this extension blocks with: the host hands its reason to the model.
    return reason ? { block: true, reason } : undefined;
  });
  on("agent_start", (_event, ctx) => {
    if (subagent(ctx)) setChild(ctx.agent.id, true);
  });
  on("agent_end", (_event, ctx) => {
    if (subagent(ctx)) {
      setChild(ctx.agent.id, false);
      return;
    }
    if (!root(ctx)) return;
    // A turn whose reply never came leaves its letters pending in Hide for the next prompt.
    state.written = [];
    // Each turn's end reports again, as Claude Code's Stop does, so a Herdr that lost the pane's counts gets them back.
    if (AGENT === "omp") void publishCounts(state, true);
  });
  on("before_subagent_spawn", (event, ctx) => {
    if (!root(ctx)) return undefined;
    // The spawn's key is the subagent's id: it counts as working from dispatch.
    setChild(event?.spawnKey, true);
    return undefined;
  });
  on("session_shutdown", (_event, ctx) => {
    if (subagent(ctx)) {
      setChild(ctx.agent.id, false);
      return;
    }
    if (!state.root) return;
    state.closed = true;
    if (shared.publish && AGENT === "omp") shared.publish = null;
    // A prompt or tool call still asking has no one to answer; a confirmation or a count finishes on its own budget.
    for (const running of state.running) {
      if (["prompt", "tool", "start"].includes(running.hideOperation)) {
        try {
          running.kill("SIGKILL");
        } catch {}
      }
    }
  });
}
