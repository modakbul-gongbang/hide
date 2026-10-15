// Hide's OpenCode plugin and its Pi and omp extension as this build writes
// them, loaded on Windows the way each host loads them and run against the
// real helper, so the path the file names, the spawn with no shell, stdin and
// stdout all cross Windows as they will on an operator's machine.
//
// `tests/it/windows_scripts.rs` writes the file and runs this once per agent
// with HIDE_SCRIPT and HIDE_SCRIPT_AGENT set; it prints one JSON line, the
// texts the script added to its session's prompts, for the test to read.

import { readFileSync } from "node:fs";
import { win32 } from "node:path";
import { pathToFileURL } from "node:url";

const AGENT = process.env.HIDE_SCRIPT_AGENT;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const fixture = (path) => JSON.parse(readFileSync(new URL(`../fixtures/${path}`, import.meta.url), "utf8"));
// A slow runner can miss a prompt's budget, and the script then gives the guidance to a later prompt.
const PROMPTS = 3;

Object.assign(process.env, { HERDR_ENV: "1", HERDR_PANE_ID: "w1:p1", HERDR_SOCKET_PATH: "\\\\.\\pipe\\herdr-fixture" });
const module = await import(pathToFileURL(process.env.HIDE_SCRIPT).href);

async function opencode() {
  const samples = fixture("opencode/events-1.18.30.json");
  const sample = (name) => structuredClone(samples[name]);
  const root = sample("session.created").properties.info;
  const client = {
    session: {
      get: async ({ path }) => ({ data: path.id === root.id ? root : undefined }),
      status: async () => ({ data: {} }),
    },
  };
  const hooks = await module.HidePlugin({ client, directory: process.cwd() });
  await hooks.event({ event: sample("session.created") });
  const added = [];
  for (let attempt = 0; attempt < PROMPTS; attempt += 1) {
    const output = sample("chat.message").output;
    await hooks["chat.message"](sample("chat.message").input, output);
    added.push(...output.parts.slice(1).map((part) => part.text));
    await sleep(1000);
  }
  await hooks.dispose();
  return added;
}

async function extension() {
  const samples = fixture(`pi-extension/${AGENT === "pi" ? "pi-1.0.4.json" : "omp-18.7.0.json"}`).samples;
  const sample = (name) => structuredClone(samples[name]);
  // Both hosts name a session file by a Windows path here, `C:\...`, not the fixture's POSIX one.
  const sessionFile = win32.join(process.cwd(), ".pi", "agent", "sessions", "fixture", "session.jsonl");
  if (!win32.isAbsolute(sessionFile) || sessionFile.startsWith("/")) throw new Error(`not a Windows session file: ${sessionFile}`);
  const context = (name) => {
    const facts = sample(name).ctx;
    return {
      hasUI: true,
      mode: "tui",
      cwd: process.cwd(),
      sessionManager: { getSessionFile: () => sessionFile, getSessionId: () => facts.sessionId },
    };
  };
  const handlers = new Map();
  module.default({
    on(name, handler) {
      handlers.set(name, [...(handlers.get(name) ?? []), handler]);
      return () => {};
    },
  });
  const emit = async (name, event, ctx) => {
    let result;
    for (const handler of handlers.get(name) ?? []) result = await handler(event, ctx);
    return result;
  };
  await emit("session_start", sample("session_start").event, context("session_start"));
  const added = [];
  for (let attempt = 0; attempt < PROMPTS; attempt += 1) {
    const result = await emit("before_agent_start", sample("before_agent_start").event, context("before_agent_start"));
    if (typeof result?.message?.content === "string") added.push(result.message.content);
    await sleep(1000);
  }
  await emit("session_shutdown", sample("session_shutdown").event, context("session_shutdown"));
  return added;
}

const added = AGENT === "opencode" ? await opencode() : await extension();
// A helper call the script gave up on may still be closing; the answer is complete without it.
process.stdout.write(`${JSON.stringify(added)}\n`, () => process.exit(0));
