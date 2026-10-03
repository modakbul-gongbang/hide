#!/usr/bin/env node
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { DEFAULT_SPAWN_KIND, HcoordError, LETTER_OPERATIONS, LETTER_SCHEMA, MAX_OUTBOX_LETTERS, REMOTE_PROTOCOL, SPAWN_KINDS, validateSpawnSpec } from "./model";
import { outboxCount, readOutboxRaw, removeLetters, writeLetter } from "./outbox";
import { readHq, writeHq } from "./remote";
import { openWork } from "./service";
import { platformSupport, REMOTE_SETUP, startDaemon } from "./platform";
import { callDaemon, lastDaemonContact, runDaemon, staleRead, type WireResult } from "./transport";
import { readAlert, reconcileAlert, warningLine } from "./health";
import { notifyText } from "./platform";
import { dataDir, loadLedger, stopMarkerPath } from "./store";
import { getAgent, runHerdrCommand } from "../implement/herdr";
import { machineId } from "./identity";
import { HCOORD_VERSION } from "./version";
import { adoptLegacyHome } from "./home";
import { API_VERSION } from "./model";

interface Parsed { words: string[]; flags: Map<string, string | true>; tail: string[] }
function parse(argv: string[]): Parsed {
  const divider = argv.indexOf("--");
  const before = divider < 0 ? argv : argv.slice(0, divider);
  const tail = divider < 0 ? [] : argv.slice(divider + 1);
  const words: string[] = [], flags = new Map<string, string | true>();
  for (let index = 0; index < before.length; index += 1) {
    const token = before[index]!;
    if (!token.startsWith("--")) { words.push(token); continue; }
    const key = token.slice(2), following = before[index + 1];
    if (following === undefined || following.startsWith("--")) flags.set(key, true);
    else { flags.set(key, following); index += 1; }
  }
  return { words, flags, tail };
}
const flag = (args: Parsed, name: string): string | undefined => { const value = args.flags.get(name); return typeof value === "string" ? value : undefined; };
const needed = (args: Parsed, name: string): string => { const value = flag(args, name); if (value === undefined || value.trim() === "") throw new HcoordError("invalid_argument", `--${name} is required`); return value; };
const duration = (value: string): number => {
  const match = /^(\d+)(s|m|h|d)$/.exec(value);
  if (!match) throw new HcoordError("invalid_argument", "duration must use s, m, h, or d, for example 5m");
  return Number(match[1]) * ({ s: 1000, m: 60_000, h: 3_600_000, d: 86_400_000 } as Record<string, number>)[match[2]!]!;
};
const USAGE = "usage: hcoord status | agent register [--check]/spawn/link/list/show/end | watch start/check/assign/stop/list | request send/show/reply/relay/ack/cancel/escalate | inbox | graph | events | daemon start/stop/status | home adopt; hcoord agent spawn --help describes a spawn";
const SPAWN_USAGE = `usage: hcoord agent spawn --parent <participant|here> --name <name> --intent <key> [--kind <kind>] [--session <id>] [--machine <name>] [--repo <path> --branch <branch> [--path <path>]] [--no-watch] [--reconcile-pane <pane>] [--resume-start] [--json] [-- <native args>]

Starts a Herdr agent as a child of --parent and records its lineage.

  --parent here    the agent in this Herdr pane, registered first when needed; its session is the default --session
  --session        the parent's session; required unless --parent here supplies it
  --name           the child's Herdr agent name: a lowercase letter, then lowercase letters, digits, _ or -, up to 32 characters
  --intent         a key for this spawn; rerunning the same command with it resumes this spawn and never starts a second agent
  --kind           ${SPAWN_KINDS.join(", ")} (default ${DEFAULT_SPAWN_KIND})
  --repo --branch  create a new worktree and Herdr workspace for the child; --path places the worktree
  --machine        a saved Herdr machine to start the child on; another machine than the parent's needs --repo and --branch there

Arguments after -- go to the --kind executable itself; do not repeat its name.
  claude: -- <claude flags> "<prompt>"   the prompt is Claude's first message
  codex:  -- <codex flags> ["<task>"]    the task must be the last argument; hcoord submits it as the first turn once Codex is ready

Arguments are checked before anything is created, so a refused spawn leaves no worktree, workspace, or pane.`;
const spawnKind = (args: Parsed): string => flag(args, "kind") ?? DEFAULT_SPAWN_KIND;

function route(args: Parsed): { operation: string; data: Record<string, unknown> } {
  const [topic, action, target] = args.words;
  if (topic === "status") return { operation: "status", data: {} };
  if (topic === "config" && action === "show") return { operation: "status", data: {} };
  if (topic === "config" && action === "set") return { operation: "config.set", data: { key: needed(args, "key"), value: duration(needed(args, "value")) } };
  if (topic === "agent" && action === "register") return { operation: args.flags.has("check") ? "agent.check" : "agent.register", data: { machine: needed(args, "machine"), hostScope: flag(args, "host-scope") ?? process.env["HERDR_SOCKET_PATH"] ?? "default", session: needed(args, "session"), instance: needed(args, "instance"), name: needed(args, "name"), project: flag(args, "project"), parent: flag(args, "parent"), pane: flag(args, "pane") } };
  if (topic === "agent" && action === "spawn") return { operation: "agent.spawn", data: { parent: needed(args, "parent"), machine: flag(args, "machine"), repo: flag(args, "repo"), branch: flag(args, "branch"), path: flag(args, "path"), session: needed(args, "session"), name: needed(args, "name"), kind: spawnKind(args), intent: needed(args, "intent"), noWatch: args.flags.has("no-watch"), reconcilePane: flag(args, "reconcile-pane"), resumeStart: args.flags.has("resume-start"), nativeArgs: args.tail } };
  if (topic === "agent" && action === "list") return { operation: "agent.list", data: { project: flag(args, "project") } };
  if (topic === "agent" && action === "show") return { operation: "agent.show", data: { id: target } };
  if (topic === "agent" && action === "end") return { operation: "agent.end", data: { id: target, actor: needed(args, "actor") } };
  if (topic === "watch" && action === "start") return { operation: "watch.start", data: { target, observer: needed(args, "observer"), actor: flag(args, "actor") ?? needed(args, "observer"), intervalMs: flag(args, "interval") ? duration(needed(args, "interval")) : undefined, brief: flag(args, "brief") } };
  if (topic === "watch" && action === "assign") return { operation: "watch.assign", data: { target, observer: needed(args, "observer"), actor: needed(args, "actor"), expectedGeneration: flag(args, "expected-generation"), brief: flag(args, "brief") } };
  if (topic === "watch" && action === "stop") return { operation: "watch.stop", data: { target, actor: needed(args, "actor") } };
  if (topic === "watch" && action === "check") return { operation: "watch.check", data: { target, cycle: needed(args, "cycle"), actor: needed(args, "actor") } };
  if (topic === "watch" && action === "list") return { operation: "watch.list", data: {} };
  if (topic === "request" && action === "send") return { operation: "request.send", data: { from: needed(args, "from"), to: needed(args, "to"), body: needed(args, "body"), intent: needed(args, "intent"), intermediary: flag(args, "intermediary"), context: flag(args, "context"), waiting: args.flags.has("waiting"), notifyOnly: args.flags.has("notify-only") } };
  if (topic === "request" && action === "show") return { operation: "request.show", data: { id: target } };
  if (topic === "request" && action === "reply") return { operation: "request.reply", data: { id: target, body: needed(args, "body"), respondent: needed(args, "as"), recordedBy: flag(args, "recorded-by") ?? needed(args, "as") } };
  if (topic === "request" && action === "relay") return { operation: "request.relay", data: { id: target, body: needed(args, "body"), actor: needed(args, "actor") } };
  if (topic === "request" && action === "ack") return { operation: "request.ack", data: { id: target, actor: needed(args, "actor"), delivery: flag(args, "delivery") } };
  if (topic === "request" && action === "cancel") return { operation: "request.cancel", data: { id: target, actor: needed(args, "actor") } };
  if (topic === "request" && action === "escalate") return { operation: "request.escalate", data: { id: target, actor: needed(args, "actor") } };
  if (topic === "inbox") return { operation: "inbox", data: {} };
  if (topic === "graph") return { operation: "graph", data: {} };
  if (topic === "events") return { operation: "events", data: { cursor: flag(args, "cursor") ?? "0" } };
  throw new HcoordError("invalid_argument", USAGE);
}

/**
 * An unstable daemon is reported above every command's output, on stderr so
 * JSON stdout stays parseable (PRD B14). A command that reached or failed to
 * reach the daemon re-evaluates the alert; any other command shows the
 * current one.
 */
let warned = false;
function warnIfUnstable(): void {
  if (warned) return;
  warned = true;
  try {
    const home = os.homedir();
    const alert = lastDaemonContact === null ? readAlert(home) : reconcileAlert(home, Date.now(), lastDaemonContact === "answered", notifyText);
    if (alert) process.stderr.write(`${warningLine(alert, home)}\n`);
  } catch (error) {
    // Unreadable health evidence is itself a warning, never a silent "healthy".
    process.stderr.write(`hcoord warning: daemon health is unknown: ${error instanceof HcoordError ? error.message : "health records could not be read"}\n`);
  }
}

/**
 * An exception no caller-facing code classified. The stderr event keeps its
 * own code (the errno when it has one) and message so the cause is findable;
 * the caller gets `internal` plus the errno, which is safe to show. A
 * SyntaxError's message quotes the text it failed to parse, which can be a
 * request body, so only its name is kept.
 */
function unclassifiedFailure(error: unknown): HcoordError {
  const cause = error instanceof Error ? error : new Error(String(error));
  const errno = typeof (cause as NodeJS.ErrnoException).code === "string" ? (cause as NodeJS.ErrnoException).code! : null;
  process.stderr.write(`${JSON.stringify({ event: "hcoord.command_failed", at: new Date().toISOString(), code: errno ?? "internal", name: cause.name, ...(cause instanceof SyntaxError ? {} : { message: cause.message.slice(0, 300) }) })}\n`);
  return new HcoordError("internal", `command failed${errno ? ` (${errno})` : ""}; the cause is on stderr as hcoord.command_failed`);
}

function print(result: WireResult, json: boolean): void {
  warnIfUnstable();
  if (json) { process.stdout.write(`${JSON.stringify(result)}\n`); return; }
  if (!result.ok) { process.stderr.write(`hcoord: ${result.error?.code}: ${result.error?.message}\n`); return; }
  if (result.delivery === "pending") { const value = result.value as { letter: string; reason: string }; process.stdout.write(`pending: letter ${value.letter} waits for the coordinator; ${value.reason}\n`); return; }
  const data = result.value;
  if (Array.isArray(data)) {
    if (data.length === 0) process.stdout.write("No items.\n");
    else for (const item of data) process.stdout.write(`${JSON.stringify(item)}\n`);
  } else process.stdout.write(`${JSON.stringify(data, null, 2)}\n`);
}

/**
 * Every write is saved in this machine's outbox before anything else, so a
 * stopped or busy coordinator delays it instead of losing it (PRD B6, B12).
 * A running local coordinator applies it at once and returns the same result
 * a direct call returned before letters existed (PRD B1).
 */
async function sendLetter(operation: string, data: Record<string, unknown>): Promise<WireResult> {
  const letter = writeLetter(operation, data);
  const pending = (reason: string): WireResult => ({ ok: true, delivery: "pending", value: { letter: letter.id, operation, reason }, observedAt: new Date().toISOString() });
  try {
    return await callDaemon("outbox.collect", { letter: letter.id }, undefined, operation === "agent.spawn" ? 240_000 : 30_000);
  } catch (error) {
    if (!(error instanceof HcoordError)) throw error;
    if (error.code === "daemon_down") return pending("the coordinator daemon is not running; it applies this letter after hcoord daemon start");
    if (error.code === "timeout") return pending("the coordinator did not answer in time; it applies this letter in order, so do not resend it");
    if (error.code === "permission_denied" || error.code === "transport") return pending(`${error.message}; the running coordinator still collects this letter from the outbox`);
    throw error;
  }
}

async function resolveParentHere(args: Parsed): Promise<void> {
  if (args.words[0] !== "agent" || args.words[1] !== "spawn" || flag(args, "parent") !== "here") return;
  const pane = process.env["HERDR_PANE_ID"];
  if (!pane) throw new HcoordError("parent_here_unavailable", "--parent here requires HERDR_PANE_ID from a Herdr agent pane; no child was created");
  const observed = getAgent(pane);
  if (observed.kind !== "found") throw new HcoordError("parent_here_unavailable", `--parent here could not confirm an agent in ${pane}: ${observed.detail}; no child was created`);
  const agent = observed.agent;
  if (agent.paneId !== pane || agent.sessionId === null || agent.terminalId === null || agent.name === null) {
    throw new HcoordError("parent_here_unavailable", `--parent here requires the pane's reported agent session, terminal, and name; ${pane} is incomplete and no child was created`);
  }
  const registered = await sendLetter("agent.register", { machine: "local", hostScope: process.env["HERDR_SOCKET_PATH"] ?? "default", session: agent.sessionId, instance: agent.terminalId, name: agent.name, pane });
  if (registered.delivery === "pending") throw new HcoordError("parent_pending", "the current pane registration is queued because the daemon did not answer; no child was created, so retry the same --parent here intent after the daemon is ready");
  if (!registered.ok) throw new HcoordError(registered.error?.code ?? "parent_here_unavailable", `${registered.error?.message ?? "current pane registration failed"}; no child was created`);
  const participant = registered.value as { id?: unknown };
  if (typeof participant.id !== "string") throw new HcoordError("parent_here_unavailable", "current pane registration returned no participant id; no child was created");
  args.flags.set("parent", participant.id);
  if (flag(args, "session") === undefined) args.flags.set("session", agent.sessionId);
}

/** Registers a Hide fork only after both exact Herdr executions exist. */
async function linkFork(args: Parsed): Promise<WireResult> {
  const parentPane = needed(args, "parent-pane"), childPane = needed(args, "child-pane");
  const observe = async (pane: string, wait: boolean): Promise<Extract<ReturnType<typeof getAgent>, { kind: "found" }>> => {
    const deadline = Date.now() + (wait ? 10_000 : 0);
    while (true) {
      const observed = getAgent(pane);
      if (observed.kind === "found" && observed.agent.paneId === pane && observed.agent.sessionId !== null && observed.agent.terminalId !== null && observed.agent.name !== null) return observed;
      if (Date.now() >= deadline) throw new HcoordError("runtime_unavailable", `Herdr did not report a complete execution in ${pane}; the fork remains open as a root agent`);
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
  };
  const parent = (await observe(parentPane, false)).agent;
  const child = (await observe(childPane, true)).agent;
  const hostScope = process.env["HERDR_SOCKET_PATH"] ?? "default";
  const parentResult = await callDaemon("agent.register", { machine: "local", hostScope, session: parent.sessionId, instance: parent.terminalId, name: parent.name, pane: parentPane });
  if (!parentResult.ok) throw new HcoordError(parentResult.error?.code ?? "registration_failed", parentResult.error?.message ?? "parent registration failed");
  const parentId = (parentResult.value as { id?: unknown }).id;
  if (typeof parentId !== "string") throw new HcoordError("registration_failed", "parent registration returned no participant id");
  const childResult = await callDaemon("agent.register", { machine: "local", hostScope, session: child.sessionId, instance: child.terminalId, name: child.name, pane: childPane, parent: parentId });
  if (!childResult.ok) throw new HcoordError(childResult.error?.code ?? "registration_failed", childResult.error?.message ?? "child registration failed");
  return ok({ parent: parentResult.value, child: childResult.value });
}

function coordinatorIdentity(result: WireResult): WireResult {
  result.value = { ...(result.value as object), hcoordVersion: HCOORD_VERSION, apiVersion: API_VERSION, machineId: machineId() };
  return result;
}

function compareVersions(left: string, right: string): number {
  const parts = (value: string): number[] | null => {
    const match = /^(\d+)\.(\d+)\.(\d+)(?:[-+].*)?$/.exec(value);
    return match ? match.slice(1).map(Number) : null;
  };
  const a = parts(left), b = parts(right);
  if (a === null || b === null) return left === right ? 0 : -1;
  for (let index = 0; index < 3; index += 1) {
    const difference = a[index]! - b[index]!;
    if (difference !== 0) return difference;
  }
  return 0;
}

function daemonNeedsReplacement(value: unknown): boolean {
  if (value === null || typeof value !== "object") return true;
  const status = value as { hcoordVersion?: unknown; runtime?: { executable?: unknown; cli?: unknown } };
  if (typeof status.hcoordVersion !== "string") return true;
  const comparison = compareVersions(status.hcoordVersion, HCOORD_VERSION);
  if (comparison > 0) return false;
  if (comparison < 0) return true;
  return status.runtime?.executable !== process.execPath || status.runtime?.cli !== path.resolve(__dirname, "cli.js");
}

const ok = (value: unknown): WireResult => ({ ok: true, value, observedAt: new Date().toISOString() });

/** This machine still coordinates work, so it cannot become another HQ's remote (PRD D-15). */
async function refuseWhileCoordinating(action: string): Promise<void> {
  const work = openWork(loadLedger());
  if (work.requests.length || work.watches.length) {
    throw new HcoordError("hq_busy", `${action} is refused while this HQ has ${work.requests.length} unresolved request(s) and ${work.watches.length} active watch(es); finish, cancel, or stop them first`, work);
  }
}

/**
 * The HQ reaches a remote only through these subcommands over the saved
 * machine's SSH target. They touch only the outbox and the HQ marker; the
 * remote keeps no conversation record (PRD D-10).
 */
async function remoteSide(args: Parsed): Promise<WireResult> {
  const action = args.words[1];
  const base = { protocol: REMOTE_PROTOCOL, letterSchema: LETTER_SCHEMA, host: os.hostname(), machineId: machineId() };
  if (action === "identity") return ok(base);
  if (action === "hello") {
    const hq = needed(args, "hq"), current = readHq();
    if (current !== "local" && current !== hq) throw new HcoordError("hq_conflict", `this machine reports to HQ ${current}; run hcoord config set hq local here before ${hq} can use it`);
    if (current === "local") {
      let running = false;
      try { running = (await callDaemon("status")).ok; } catch (error) { if (!(error instanceof HcoordError) || error.code !== "daemon_down") throw error; }
      if (running) throw new HcoordError("hq_conflict", `this machine runs its own coordinator daemon; stop it or move its HQ before ${hq} can use it`);
      await refuseWhileCoordinating(`joining HQ ${hq}`);
      writeHq(hq);
    }
    return ok({ ...base, hq, outbox: outboxCount() });
  }
  if (action === "herdr") {
    if (args.tail.length === 0) throw new HcoordError("invalid_argument", "remote herdr requires arguments after --");
    const env = { ...process.env };
    const session = flag(args, "session");
    delete env["HERDR_SOCKET_PATH"];
    if (session) env["HERDR_SESSION"] = session;
    const result = runHerdrCommand(args.tail, 30_000, env);
    return ok({ ...base, status: result.status, stdout: result.stdout, stderr: result.stderr, errorCode: result.errorCode ?? null });
  }
  if (action === "take") {
    const limit = Math.min(Number(flag(args, "limit") ?? "64"), MAX_OUTBOX_LETTERS);
    return ok({ ...base, letters: readOutboxRaw(Number.isSafeInteger(limit) && limit > 0 ? limit : 64, undefined, 4 * 1024 * 1024) });
  }
  if (action === "drop") return ok({ ...base, removed: removeLetters(args.words.slice(2)) });
  throw new HcoordError("invalid_argument", "remote subcommands are identity, hello, herdr, take, and drop");
}

/** `hcoord config set hq <local|machine>` (PRD B17). */
async function setHq(value: string | undefined): Promise<WireResult> {
  if (value === undefined || value.trim() === "") throw new HcoordError("invalid_argument", "usage: hcoord config set hq <local|machine name>");
  const current = readHq();
  if (value === current) return ok({ hq: current, changed: false });
  const waiting = outboxCount();
  if (current !== "local" && waiting > 0) throw new HcoordError("hq_busy", `${waiting} letter(s) still wait for HQ ${current}; let it collect them before moving`, { letters: waiting });
  if (current === "local") {
    await refuseWhileCoordinating(`moving the HQ to ${value}`);
    try {
      const stopped = await callDaemon("daemon.stop");
      if (stopped.ok) fs.writeFileSync(stopMarkerPath(), `${new Date().toISOString()}\n`, { mode: 0o600 });
    } catch (error) { if (!(error instanceof HcoordError) || error.code !== "daemon_down") throw error; }
  }
  writeHq(value);
  return ok({ hq: value, changed: true, previous: current });
}

export async function main(argv: string[]): Promise<number> {
  const args = parse(argv), json = args.flags.has("json");
  try {
    if (args.flags.has("help") || args.words[0] === "help") {
      const usage = args.words[0] === "agent" && args.words[1] === "spawn" ? SPAWN_USAGE : USAGE;
      if (json) print(ok({ usage }), json); else process.stdout.write(`${usage}\n`);
      return 0;
    }
    // A refused spawn creates nothing: not the parent registration, the letter, the worktree, or the pane (#237).
    if (args.words[0] === "agent" && args.words[1] === "spawn") validateSpawnSpec(needed(args, "name"), spawnKind(args), args.tail);
    if (args.words[0] === "version") { print(ok({ hcoordVersion: HCOORD_VERSION, apiVersion: API_VERSION }), json); return 0; }
    // Before anything reads the home: it is the move into it (PRD hide-home-layout D-09).
    if (args.words[0] === "home" && args.words[1] === "adopt") {
      if (args.flags.has("from")) throw new HcoordError("invalid_argument", "home adopt takes no --from: it moves only this HOME's ~/.hcoord");
      print(ok({ ...adoptLegacyHome() }), json); return 0;
    }
    if (args.words[0] === "remote") { const result = await remoteSide(args); process.stdout.write(`${JSON.stringify(result)}\n`); return 0; }
    if (args.words[0] === "config" && args.words[1] === "set" && args.words[2] === "hq") { const result = await setHq(args.words[3]); print(result, json); return 0; }
    const hq = readHq();
    if (hq !== "local") {
      const { operation, data } = args.words[0] === "daemon" ? { operation: `${args.words[0]}.${args.words[1]}`, data: {} } : route(args);
      // A remote runs no daemon of its own, so its converged state is already reached; hide's kit asks every machine to converge.
      if (operation === "daemon.ensure") {
        print({ ok: true, value: { running: false, changed: false, hq, reason: `this machine reports to HQ ${hq}, whose daemon coordinates it` }, observedAt: new Date().toISOString() }, json);
        return 0;
      }
      if (!LETTER_OPERATIONS.has(operation)) throw new HcoordError("hq_only", `${operation} runs only at the coordinator HQ (${hq}); this machine keeps no conversation record`, { hq });
      const letter = writeLetter(operation, data);
      print({ ok: true, delivery: "pending", value: { letter: letter.id, operation, reason: `the coordinator at ${hq} applies it when it next collects this machine's letters; nothing else to do`, hq }, observedAt: new Date().toISOString() }, json);
      return 0;
    }
    if (args.words[0] === "daemon") {
      const action = args.words[1];
      if (action === "run") {
        if (await runDaemon() === "manual_stop") print({ ok: true, value: { running: false, manualStop: true, next: "hcoord daemon start clears the manual stop" }, observedAt: new Date().toISOString() }, json);
        return 0;
      }
      if (action === "start") { const value = startDaemon(); print({ ok: true, value, observedAt: new Date().toISOString() }, json); return 0; }
      if (action === "ensure") {
        if (fs.existsSync(stopMarkerPath())) { print({ ok: true, value: { running: false, manualStop: true, changed: false }, observedAt: new Date().toISOString() }, json); return 0; }
        try {
          const running = await callDaemon("status");
          if (!daemonNeedsReplacement(running.value)) {
            running.value = { ...(running.value as object), running: true, changed: false };
            print(running, json);
          } else {
            const previous = (running.value as { hcoordVersion?: unknown }).hcoordVersion;
            await callDaemon("daemon.stop");
            print({ ok: true, value: { ...startDaemon(), running: true, changed: true, replacedVersion: typeof previous === "string" ? previous : null }, observedAt: new Date().toISOString() }, json);
          }
        } catch (error) {
          if (!(error instanceof HcoordError) || error.code !== "daemon_down") throw error;
          print({ ok: true, value: { ...startDaemon(), running: true, changed: true }, observedAt: new Date().toISOString() }, json);
        }
        return 0;
      }
      if (action === "stop") {
        const result = await callDaemon("daemon.stop");
        print(result, json);
        if (result.ok) {
          fs.mkdirSync(dataDir(), { recursive: true, mode: 0o700 });
          fs.writeFileSync(stopMarkerPath(), `${new Date().toISOString()}\n`, { mode: 0o600 });
        }
        return result.ok ? 0 : 1;
      }
      if (action === "status") {
        let result: WireResult;
        try { result = await callDaemon("status"); }
        catch (error) { if (!(error instanceof HcoordError) || error.code !== "daemon_down") throw error; result = staleRead("status"); }
        result = coordinatorIdentity(result);
        result.value = { ...(result.value as object), platform: platformSupport(), remote: { hq: readHq(), setup: REMOTE_SETUP, limits: "a new remote worktree may show the agent's own folder-trust prompt, which a person answers; remote letters arrive at the next collection (about 5 s)" } };
        print(result, json);
        return 0;
      }
      throw new HcoordError("invalid_argument", "use daemon run, start, ensure, stop, or status");
    }
    if (args.words[0] === "agent" && args.words[1] === "link") {
      const result = await linkFork(args);
      print(result, json);
      return 0;
    }
    await resolveParentHere(args);
    const { operation, data } = route(args);
    if (args.words[0] === "events" && args.flags.has("follow")) {
      let cursor = Number(data["cursor"]);
      while (true) {
        const result = await callDaemon("events", { cursor });
        if (!result.ok) { print(result, json); return 1; }
        const stream = result.value as { events: unknown[]; cursor: number; hasMore: boolean };
        for (const entry of stream.events) process.stdout.write(`${JSON.stringify(entry)}\n`);
        cursor = stream.cursor;
        if (!stream.hasMore) await new Promise((resolve) => setTimeout(resolve, 1000));
      }
    }
    if (LETTER_OPERATIONS.has(operation)) {
      const result = await sendLetter(operation, data);
      print(result, json);
      return result.ok ? 0 : 1;
    }
    let result: WireResult;
    try { result = await callDaemon(operation, data); }
    catch (error) { if (!(error instanceof HcoordError) || error.code !== "daemon_down") throw error; result = staleRead(operation, data); }
    print(result, json);
    return result.ok ? 0 : 1;
  } catch (error) {
    const reason = error instanceof HcoordError ? error : unclassifiedFailure(error);
    print({ ok: false, error: { code: reason.code, message: reason.message, ...(reason.detail ? { detail: reason.detail } : {}) }, observedAt: new Date().toISOString() }, json);
    return reason.code === "invalid_argument" ? 2 : 1;
  }
}

if (require.main === module) void main(process.argv.slice(2)).then((code) => { process.exitCode = code; });
