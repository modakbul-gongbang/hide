// hide-opencode-plugin@1 sha256=b904bf3490c7b14195aa10199640fbba2bf8051544d44fd1d8dbbc7668220f03
// Hide's OpenCode plugin, written by Hide's install kit (hide-agent-hooks).
// An edit is kept and shown as edited in Settings; Reinstall puts Hide's back.
// Outside a Herdr pane Hide manages, or when its helper is gone, it does nothing.

import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { statSync } from "node:fs";

const HELPER = "/kit path/it's/hide-agent-hooks";

// The prompt budget is Claude Code's prompt hook's; the tool budget is the
// spawn guard's. OpenCode puts no limit on a hook, so these are the limits.
const PROMPT_BUDGET_MS = 1850;
const TOOL_BUDGET_MS = 2500;
const START_BUDGET_MS = 8000;
const CONFIRM_BUDGET_MS = 2000;
const COUNT_BUDGET_MS = 2000;
const LOOKUP_BUDGET_MS = 300;
const OUTPUT_LIMIT = 64 * 1024;
// Far inside the helper's 256 KiB input, so a pasted log never costs a prompt its letters.
const PROMPT_TEXT_LIMIT = 32 * 1024;
const RUNNING_LIMIT = 8;
const SESSION_LIMIT = 512;
const PENDING_LIMIT = 16;
const PENDING_AGE_MS = 10 * 60 * 1000;

function managed(env) {
  if (env.HERDR_ENV !== "1" || !env.HERDR_PANE_ID || !env.HERDR_SOCKET_PATH) return false;
  try {
    return statSync(HELPER).isFile();
  } catch {
    return false;
  }
}

/** The helper's answer, or null when it failed, timed out or said nothing usable. */
function helper(state, operation, input, budgetMs) {
  if (state.running.size >= RUNNING_LIMIT) {
    state.lost += 1;
    return Promise.resolve(null);
  }
  // A call skipped at the cap or given up on reaches Hide's diagnostic through the next one.
  if (state.lost > 0) {
    input = { ...input, lost: state.lost };
    state.lost = 0;
  }
  return new Promise((resolve) => {
    let child;
    try {
      child = spawn(HELPER, ["opencode", operation], { stdio: ["pipe", "pipe", "ignore"] });
    } catch {
      resolve(null);
      return;
    }
    state.running.add(child);
    const chunks = [];
    let size = 0;
    let settled = false;
    const finish = (value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      state.running.delete(child);
      resolve(value);
    };
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      state.lost += 1;
      finish(null);
    }, budgetMs);
    child.on("error", () => finish(null));
    child.stdout.on("data", (chunk) => {
      size += chunk.length;
      if (size > OUTPUT_LIMIT) {
        child.kill("SIGKILL");
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
  });
}

/** A part id in OpenCode's own ascending form (`prt_` + 12 hex + 14 base62), so it sorts after the prompt's parts. */
function partId(state) {
  const now = Date.now();
  state.idCounter = now === state.idTime ? state.idCounter + 1 : 1;
  state.idTime = now;
  const value = BigInt(now) * 0x1000n + BigInt(state.idCounter);
  let hex = "";
  for (let index = 0; index < 6; index += 1) {
    hex += Number((value >> BigInt(40 - 8 * index)) & 0xffn).toString(16).padStart(2, "0");
  }
  const alphabet = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
  let random = "";
  for (const byte of randomBytes(14)) random += alphabet[byte % 62];
  return `prt_${hex}${random}`;
}

/** The start of a prompt, enough for Memory's retrieval, cut where JSON can still carry it. */
function clip(text) {
  if (text.length <= PROMPT_TEXT_LIMIT) return text;
  const end = PROMPT_TEXT_LIMIT - (/[\uD800-\uDBFF]/.test(text[PROMPT_TEXT_LIMIT - 1]) ? 1 : 0);
  return text.slice(0, end);
}

function withBudget(promise, budgetMs) {
  return new Promise((resolve) => {
    const timer = setTimeout(() => resolve(null), budgetMs);
    promise.then(
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

/**
 * Records `key` as the newest entry of `map`, a Map or a Set of at most SESSION_LIMIT sessions. At the limit the
 * oldest entry the first matching rule of `evictable` accepts goes, so a long-lived OpenCode keeps tracking its
 * newest sessions; when none may go the new one is not recorded and the loss reaches the diagnostic through the
 * next helper call.
 */
function remember(state, map, key, value, evictable = [() => true]) {
  map.delete(key);
  if (map.size >= SESSION_LIMIT) {
    const entries = [...map.entries()];
    const oldest = evictable.map((rule) => entries.find(([, entry]) => rule(entry))).find(Boolean);
    if (!oldest) {
      state.lost += 1;
      return;
    }
    map.delete(oldest[0]);
  }
  if (map instanceof Set) map.add(key);
  else map.set(key, value);
}

/** A finished child's line goes before a root's, and a root's only when no child's is left. */
const PARENT_EVICTION = [(parent) => parent !== null, () => true];

/** Whether `sessionID` is a root session: known from `session.created`, else asked once. A prompted root becomes the newest entry. */
async function rootSession(state, client, sessionID) {
  if (state.parents.has(sessionID)) {
    const parent = state.parents.get(sessionID);
    remember(state, state.parents, sessionID, parent, PARENT_EVICTION);
    return parent === null;
  }
  const answer = await withBudget(client.session.get({ path: { id: sessionID } }), LOOKUP_BUDGET_MS);
  const info = answer?.data;
  if (!info || info.id !== sessionID) return false;
  remember(state, state.parents, sessionID, info.parentID ?? null, PARENT_EVICTION);
  return !info.parentID;
}

/** The root session above `sessionID`, or null when its line is not known. */
function rootOf(state, sessionID) {
  let current = sessionID;
  for (let depth = 0; depth < SESSION_LIMIT; depth += 1) {
    if (!state.parents.has(current)) return null;
    const parent = state.parents.get(current);
    if (parent === null) return current;
    current = parent;
  }
  return null;
}

/** Sends the pane's subagent counts when they changed, or always when `force`; calls run one at a time, in order. */
function publishCounts(state, force) {
  let working = 0;
  let done = 0;
  for (const [child, status] of state.children) {
    if (rootOf(state, child) !== state.root) continue;
    if (status.busy) working += 1;
    else if (status.ran) done += 1;
  }
  const key = `${working}/${done}`;
  if (!force && key === state.published) return state.counting;
  // Only a report Herdr took counts as published: a failed one is sent again with the next change, prompt or rest.
  state.published = key;
  state.counting = state.counting.then(async () => {
    const answer = await helper(state, "subagents", { working, done }, COUNT_BUDGET_MS);
    if (answer?.reported !== true && state.published === key) state.published = null;
  });
  return state.counting;
}

/** At root idle, finished children leave the working count; a background child still busy stays. */
async function sweep(state, client) {
  const busy = [...state.children].filter(([child, status]) => status.busy && rootOf(state, child) === state.root);
  if (busy.length === 0) return;
  const answer = await withBudget(client.session.status(), LOOKUP_BUDGET_MS);
  const running = answer?.data;
  if (!running || typeof running !== "object") return;
  for (const [child, status] of busy) {
    const now = running[child]?.type;
    if (now !== "busy" && now !== "retry") status.busy = false;
  }
  await publishCounts(state, false);
}

function onEvent(state, client, event) {
  const type = event?.type;
  const properties = event?.properties ?? {};
  if (type === "session.created") {
    const info = properties.info;
    if (!info?.id) return;
    remember(state, state.parents, info.id, info.parentID ?? null, PARENT_EVICTION);
    // A child still running is never forgotten: it would leave the working count while it works.
    if (info.parentID) remember(state, state.children, info.id, { busy: false, ran: false }, [(child) => !child.busy]);
    return;
  }
  if (type === "session.status") {
    const sessionID = properties.sessionID;
    const kind = properties.status?.type;
    const child = state.children.get(sessionID);
    if (child) {
      const busy = kind === "busy" || kind === "retry";
      child.busy = busy;
      child.ran ||= busy;
      void publishCounts(state, false);
    } else if (sessionID === state.root && kind === "idle") {
      // Each turn's end reports again, as Claude Code's Stop does, so a Herdr that lost the pane's counts gets them back.
      void sweep(state, client).then(() => publishCounts(state, true));
    }
    return;
  }
  if (type === "message.part.updated") {
    const id = properties.part?.id;
    const pending = id && state.pending.get(id);
    if (!pending) return;
    state.pending.delete(id);
    // OpenCode wrote the prompt carrying the letters: only now are they received.
    void helper(state, "confirm", { session_id: pending.session, letters: pending.letters }, CONFIRM_BUDGET_MS);
  }
}

async function onPrompt(state, client, directory, input, output) {
  const sessionID = input?.sessionID;
  const message = output?.message;
  const parts = output?.parts;
  if (!sessionID || !message?.id || !Array.isArray(parts)) return;
  // A prompt that is only text OpenCode or a plugin wrote is no one's turn.
  const typed = parts.filter((part) => !(part?.type === "text" && part.synthetic));
  if (typed.length === 0) return;
  const deadline = Date.now() + PROMPT_BUDGET_MS;
  if (!(await rootSession(state, client, sessionID))) return;
  // A session's first prompt this plugin sees carries the start guidance and the session-start Memory, as
  // Claude Code's SessionStart does for a new or resumed session; event order cannot hide it.
  const first = !state.guided.has(sessionID);
  state.root = sessionID;
  void publishCounts(state, true);
  const prompt = clip(
    typed
      .filter((part) => part.type === "text" && typeof part.text === "string")
      .map((part) => part.text)
      .join("\n"),
  );
  const answer = await helper(state, "prompt", { session_id: sessionID, prompt, cwd: directory, first }, Math.max(0, deadline - Date.now()));
  const sections = [];
  if (first) {
    const guidance = await withBudget(state.guidance, Math.max(0, deadline - Date.now()));
    if (typeof guidance?.context === "string" && guidance.context) sections.push(guidance.context);
    // Guidance still on its way is given to the next prompt instead.
    // A prompt whose helper answered nothing has no session-start Memory yet: the next one asks again.
    if (state.started && answer !== null) remember(state, state.guided, sessionID);
  }
  if (typeof answer?.context === "string" && answer.context) sections.push(answer.context);
  if (sections.length === 0) return;
  const id = partId(state);
  // Synthetic: OpenCode's screen leaves it out of the operator's message, and the reminder tags tell the model it is not the operator's text.
  const text = `<system-reminder>\n${sections.join("\n\n")}\n</system-reminder>`;
  parts.push({ id, sessionID, messageID: message.id, type: "text", text, synthetic: true });
  const letters = Array.isArray(answer?.letters) ? answer.letters.filter((letter) => typeof letter === "string") : [];
  if (letters.length === 0) return;
  const now = Date.now();
  for (const [key, entry] of state.pending) {
    if (now - entry.at > PENDING_AGE_MS) state.pending.delete(key);
  }
  // A letter left out stays pending in Hide and reaches the next prompt.
  if (state.pending.size >= PENDING_LIMIT) state.pending.delete(state.pending.keys().next().value);
  state.pending.set(id, { session: sessionID, letters, at: now });
}

async function onTool(state, sessionID, directory, input, output) {
  const tool = input?.tool;
  let request;
  if (tool === "bash") {
    const command = output?.args?.command;
    // Every shell call comes here: only one that can start an agent through Herdr asks the helper.
    if (typeof command !== "string" || !(command.includes("herdr") || command.includes("HERDR_BIN_PATH"))) return null;
    const workdir = output.args.workdir;
    request = { session_id: sessionID, tool, command, cwd: typeof workdir === "string" && workdir ? workdir : directory };
  } else if (tool === "question") {
    // The core knows the pane by the root session Herdr's integration reports, also for a subagent's call.
    request = { session_id: rootOf(state, sessionID) ?? sessionID, tool };
  } else {
    return null;
  }
  const answer = await helper(state, "tool", request, TOOL_BUDGET_MS);
  return typeof answer?.deny === "string" && answer.deny ? answer.deny : null;
}

export const HidePlugin = async ({ client, directory }) => {
  if (!managed(process.env)) return {};
  const state = {
    running: new Set(),
    lost: 0,
    parents: new Map(),
    children: new Map(),
    guided: new Set(),
    started: false,
    pending: new Map(),
    root: null,
    published: null,
    counting: Promise.resolve(),
    idTime: 0,
    idCounter: 0,
    guidance: null,
  };
  state.guidance = helper(state, "start", { cwd: directory }, START_BUDGET_MS).then((answer) => {
    state.started = true;
    return answer;
  });
  void publishCounts(state, true);
  return {
    event: async ({ event }) => {
      try {
        onEvent(state, client, event);
      } catch {}
    },
    "chat.message": async (input, output) => {
      // A throw here would end the operator's prompt: every failure passes the prompt unchanged.
      try {
        await onPrompt(state, client, directory, input, output);
      } catch {}
    },
    "tool.execute.before": async (input, output) => {
      let reason = null;
      try {
        reason = await onTool(state, input?.sessionID, directory, input, output);
      } catch {}
      // The refusal is the one error this plugin raises: OpenCode hands its text to the model.
      if (reason) throw new Error(reason);
    },
    dispose: async () => {
      for (const child of state.running) child.kill("SIGKILL");
      state.running.clear();
    },
  };
};
