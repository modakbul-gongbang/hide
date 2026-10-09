import { expect } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { DatabaseSync } from "node:sqlite";
import type { HerdrFixture } from "./herdr-fixture";
import { copyFixtureShim } from "./shims/build";
import { fixtureExecutable, fixtureProgram } from "./platform-fixture";

export const PI_ID = "fixture-pi-native";
export const PI_TITLE = "요청 보기 - Pi native title";

/** A seed consumed and written by the fake CLI. No session file exists yet. */
export function preparePiWriter(herdr: HerdrFixture, checkout = path.join(herdr.root, "fixture")): string {
  copyFixtureShim("claude-shim", path.join(herdr.root, "bin", fixtureExecutable("pi")));
  const cwd = fs.realpathSync(checkout);
  const encoded = `--${cwd.replace(/^[/\\]/, "").replace(/[/\\:]/g, "-")}--`;
  const session = path.join(herdr.env.HOME!, ".pi", "agent", "sessions", encoded, "timestamp_fixture.jsonl");
  fs.mkdirSync(path.dirname(session), { recursive: true });
  fs.writeFileSync(path.join(herdr.root, "pi-session-path.config"), session);
  const timestamp = "2026-10-03T01:00:00Z";
  const records = [
    { type: "session", version: 3, id: PI_ID, timestamp, cwd },
    { type: "message", id: "a0000001", parentId: null, timestamp, message: { role: "user", content: "요청 보기를 만들어줘" } },
    { type: "message", id: "a0000002", parentId: "a0000001", timestamp, message: { role: "assistant", content: [{ type: "text", text: "HIDE_E2E_LABEL {\"goal\":\"Generated Pi goal\",\"goal_changed\":true,\"line\":\"완료\",\"end\":\"done\"}" }], stopReason: "stop" } },
    { type: "session_info", id: "a0000003", parentId: "a0000002", timestamp, name: PI_TITLE },
  ];
  fs.writeFileSync(path.join(herdr.root, "pi-session-seed.jsonl"), records.map(record => `${JSON.stringify(record)}\n`).join(""));
  return session;
}

/** Explicit synthetic integration provenance, never a native discovery claim. */
export function reportPiWriter(herdr: HerdrFixture, pane: string, session: string): void {
  execFileSync(herdr.bin, ["pane", "report-agent-session", pane, "--source", "herdr:pi", "--agent", "pi", "--agent-session-path", session, "--seq", "2", "--session-start-source", "clear"], { env: herdr.env, timeout: 30_000 });
}

export const OPENCODE_ID = "ses_fixtureopencoderoot";
export const OPENCODE_TITLE = "요청 보기 - OpenCode native title";
export const OPENCODE_CHILD_TITLE = "Explore the code (@explore subagent)";

/**
 * OpenCode 1.18.30's own tables, as its migrations create them for these reads, and the write-ahead log and
 * busy wait it opens its database with, so a reader never blocks its writes.
 */
export const OPENCODE_SCHEMA = `
PRAGMA journal_mode = WAL;
PRAGMA busy_timeout = 5000;
CREATE TABLE IF NOT EXISTS session (id text PRIMARY KEY, project_id text NOT NULL, parent_id text, slug text NOT NULL,
  directory text NOT NULL, title text NOT NULL, version text NOT NULL, time_created integer NOT NULL, time_updated integer NOT NULL);
CREATE TABLE IF NOT EXISTS message (id text PRIMARY KEY, session_id text NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  time_created integer NOT NULL, time_updated integer NOT NULL, data text NOT NULL);
CREATE TABLE IF NOT EXISTS part (id text PRIMARY KEY, message_id text NOT NULL REFERENCES message(id) ON DELETE CASCADE,
  session_id text NOT NULL, time_created integer NOT NULL, time_updated integer NOT NULL, data text NOT NULL);`;

// Run as `opencode` in a pane of the private Herdr: a first launch writes its
// session into OpenCode's database the way OpenCode does (a root session in
// the pane's checkout and a subagent's child session under it), a `-s <id>`
// launch continues it without writing, and each tells Herdr its session id
// the way Herdr's OpenCode integration does. Every argument list is logged.
const OPENCODE_DOUBLE = String.raw`
const fs = require("node:fs");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
// Node 22 calls its SQLite module experimental; the pane shows OpenCode, not that notice.
process.removeAllListeners("warning");
const { DatabaseSync } = require("node:sqlite");
const config = JSON.parse(fs.readFileSync(path.join(path.dirname(process.argv[1]), "opencode-double.json"), "utf8"));
const argv = process.argv.slice(2);
fs.appendFileSync(config.launches, JSON.stringify(argv) + "\n");
if (!(argv[0] === "-s" && argv[1] === config.id && argv.length === 2)) {
  fs.mkdirSync(path.dirname(config.database), { recursive: true });
  const db = new DatabaseSync(config.database);
  db.exec(config.schema);
  const at = Date.now();
  const session = db.prepare("INSERT INTO session VALUES (?, 'prj_fixture', ?, ?, ?, ?, '1.18.30', ?, ?)");
  const message = db.prepare("INSERT INTO message VALUES (?, ?, ?, ?, ?)");
  const part = db.prepare("INSERT INTO part VALUES (?, ?, ?, ?, ?, ?)");
  const directory = fs.realpathSync(process.cwd());
  session.run(config.id, null, "brave-otter", directory, config.title, at, at + 2);
  message.run("msg_01", config.id, at, at, JSON.stringify({ role: "user", time: { created: at } }));
  part.run("prt_01", "msg_01", config.id, at, at, JSON.stringify({ type: "text", text: "요청 보기를 만들어줘" }));
  message.run("msg_02", config.id, at + 1, at + 2, JSON.stringify({ role: "assistant", time: { created: at + 1, completed: at + 2 } }));
  part.run("prt_02", "msg_02", config.id, at + 1, at + 1, JSON.stringify({ type: "text", text: config.answer }));
  session.run(config.child, config.id, "quiet-heron", directory, config.childTitle, at + 1, at + 1);
  message.run("msg_c1", config.child, at + 1, at + 1, JSON.stringify({ role: "user", time: { created: at + 1 } }));
  part.run("prt_c1", "msg_c1", config.child, at + 1, at + 1, JSON.stringify({ type: "text", text: "subagent task" }));
  db.close();
}
const pane = process.env.HERDR_PANE_ID;
const listed = () => JSON.parse(execFileSync(config.herdr, ["agent", "list"], { encoding: "utf8", timeout: 10000 }))
  .result.agents.find((agent) => agent.pane_id === pane)?.agent_session?.value;
for (let attempt = 1; listed() !== config.id; attempt += 1) {
  if (attempt > 50) throw new Error("Herdr never took the double's session");
  const seq = Date.now() * 1000 + attempt * 2;
  execFileSync(config.herdr, ["pane", "report-agent", pane, "--source", "herdr:opencode", "--agent", "opencode",
    "--state", "idle", "--agent-session-id", config.id, "--seq", String(seq)], { timeout: 10000 });
  execFileSync(config.herdr, ["pane", "report-agent-session", pane, "--source", "herdr:opencode", "--agent", "opencode",
    "--agent-session-id", config.id, "--session-start-source", "select", "--seq", String(seq + 1)], { timeout: 10000 });
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 100);
}
process.stdout.write("opencode fixture ready\n");
setInterval(() => {}, 1 << 30);
`;

/** Installs the OpenCode double as `opencode`; answers its database's path. */
export function prepareOpenCodeWriter(herdr: HerdrFixture): string {
  const database = path.join(herdr.env.HOME!, ".local", "share", "opencode", "opencode.db");
  const bin = path.join(herdr.root, "bin");
  fs.writeFileSync(path.join(bin, "opencode-double.json"), JSON.stringify({
    herdr: herdr.bin, database, schema: OPENCODE_SCHEMA, id: OPENCODE_ID, title: OPENCODE_TITLE,
    child: `${OPENCODE_ID}child`, childTitle: OPENCODE_CHILD_TITLE, launches: path.join(herdr.root, "opencode-launches.jsonl"),
    answer: "HIDE_E2E_LABEL {\"goal\":\"Generated OpenCode goal\",\"goal_changed\":true,\"line\":\"완료\",\"end\":\"done\"}",
  }));
  fixtureProgram(bin, "opencode", OPENCODE_DOUBLE);
  return database;
}

/** What OpenCode writes while its `question` tool waits, and once it is answered. */
export function writeOpenCodeQuestion(database: string, state: "running" | "completed"): void {
  const db = new DatabaseSync(database);
  db.exec(OPENCODE_SCHEMA);
  const at = Date.now();
  const input = { questions: [{ question: "어느 브랜치에 올릴까요?", header: "Branch", options: [{ label: "main", description: "기본" }, { label: "release", description: "배포" }] }] };
  const time = state === "running" ? { created: at } : { created: at - 1, completed: at };
  const toolState = state === "running" ? { status: "running", input, time: { start: at } } : { status: "completed", input, output: "main", title: "", metadata: {}, time: { start: at - 1, end: at } };
  db.prepare("INSERT INTO message VALUES ('msg_03', ?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET data = excluded.data, time_updated = excluded.time_updated")
    .run(OPENCODE_ID, at, at, JSON.stringify({ role: "assistant", time }));
  db.prepare("INSERT INTO part VALUES ('prt_03', 'msg_03', ?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET data = excluded.data, time_updated = excluded.time_updated")
    .run(OPENCODE_ID, at, at, JSON.stringify({ type: "tool", tool: "question", callID: "call_q1", state: toolState }));
  db.close();
}

let reported = 0;

/**
 * Herdr's OpenCode integration telling Herdr the pane's state, as OpenCode moves, once Herdr lists it. Its
 * sequence is time-based like the integration's, and rises within one millisecond, since Herdr keeps only a
 * newer report.
 */
export async function reportOpenCodeState(herdr: HerdrFixture, pane: string, state: "working" | "idle"): Promise<void> {
  reported = Math.max(reported + 1, Date.now() * 1000);
  execFileSync(herdr.bin, ["pane", "report-agent", pane, "--source", "herdr:opencode", "--agent", "opencode", "--state", state,
    "--agent-session-id", OPENCODE_ID, "--seq", String(reported)], { env: herdr.env, timeout: 30_000 });
  type Listed = { result: { agents: { pane_id: string; agent_status: string }[] } };
  await expect.poll(() => (herdr.run(["agent", "list"]) as Listed).result.agents.find((agent) => agent.pane_id === pane)?.agent_status,
    { message: `Herdr lists ${pane} ${state}`, timeout: 10_000 }).toMatch(state === "working" ? /^working$/ : /^(idle|done)$/);
}
