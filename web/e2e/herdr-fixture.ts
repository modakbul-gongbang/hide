// An isolated pinned Herdr server with two fake agent panes, for the click
// flow e2e (PRD B7, B8). Nothing here touches the operator's socket: every
// command runs with a private HERDR_SOCKET_PATH, session, config and state.
//
// Sidebar rows exist only for panes Herdr classifies as an agent, and the
// pinned Herdr classifies by the process it started, so each pane runs a
// tiny compiled `claude` that copies stdin to stdout. A copied /bin/cat
// would not do: macOS kills a relocated platform binary. Like a real TUI it
// reads the terminal raw, byte by byte with no line discipline echo and no
// extended input processing (which would swallow ^O and ^V), and it
// appends every byte it reads to `HIDE_E2E_INPUT_LOG`: that file is what the
// PTY received, which is how a test tells a click's mouse report from what
// the shell only sent (PRD S2 B20).
//
// The same binary is the daemon's Claude provider, because hided finds
// `claude` on the same PATH: `claude auth status` answers logged in, and a
// print-mode request (`--json-schema`) answers with the label written after
// the last `HIDE_E2E_LABEL ` in its prompt, after the delay a
// `HIDE_E2E_DELAY_MS ` line before it asks for, and appends the answered
// task to `HIDE_E2E_PROVIDER_LOG` when that is set. `labelAgent` writes that marker
// into a synthetic Claude transcript, so a pane's label comes from the
// core's own transcript read and analysis, never from a token (PRD
// labels-in-hided).

import { execFileSync, spawn, spawnSync, type ChildProcess } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { ownUntilWorkerExit } from "./worker-owned";

export type HerdrFixture = {
  bin: string;
  socket: string;
  env: NodeJS.ProcessEnv;
  /** The fixture root; `fixture/` under it is the first workspace's cwd. */
  root: string;
  workspace: string;
  tab: string;
  panes: [string, string];
  /** Per agent pane, the file its `claude` shim appends every byte the PTY delivered to. */
  inputLogs: [string, string];
  /** The controlled shim and system-tool PATH for the server, panes and daemon. */
  fixturePath: string;
  /** Runs a pinned-herdr CLI command against the private server and parses its JSON. */
  run: (args: string[]) => unknown;
  stop: () => void;
};

const SHIM_SOURCE = `#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <termios.h>
#include <time.h>
#include <unistd.h>
static char prompt[1 << 20];
static int provider(int argc, char **argv) {
  if (argc > 2 && strcmp(argv[1], "auth") == 0 && strcmp(argv[2], "status") == 0) {
    puts("{\\"loggedIn\\":true}");
    return 0;
  }
  size_t len = 0; ssize_t n;
  while (len < sizeof prompt - 1 && (n = read(0, prompt + len, sizeof prompt - 1 - len)) > 0) len += (size_t)n;
  prompt[len] = 0;
  char *label = NULL;
  for (char *at = prompt; (at = strstr(at, "HIDE_E2E_LABEL ")) != NULL; at++) label = at;
  if (!label) {
    puts("{\\"type\\":\\"result\\",\\"is_error\\":true,\\"subtype\\":\\"error_during_execution\\"}");
    return 1;
  }
  char *delay = NULL;
  for (char *at = prompt; (at = strstr(at, "HIDE_E2E_DELAY_MS ")) != NULL && at < label; at++) delay = at;
  if (delay) {
    long ms = atol(delay + strlen("HIDE_E2E_DELAY_MS "));
    struct timespec wait = { ms / 1000, (ms % 1000) * 1000000L };
    nanosleep(&wait, NULL);
  }
  label += strlen("HIDE_E2E_LABEL ");
  char *end = strchr(label, '\\n');
  if (end) *end = 0;
  const char *calls = getenv("HIDE_E2E_PROVIDER_LOG");
  FILE *log = calls ? fopen(calls, "a") : NULL;
  if (log) { fprintf(log, "%s\\n", label); fclose(log); }
  printf("{\\"type\\":\\"result\\",\\"is_error\\":false,\\"structured_output\\":%s}\\n", label);
  return 0;
}
int main(int argc, char **argv) {
  for (int i = 1; i < argc; i++) {
    if (strcmp(argv[i], "--json-schema") == 0 || (i == 1 && strcmp(argv[i], "auth") == 0)) return provider(argc, argv);
  }
  const char *log_path = getenv("HIDE_E2E_INPUT_LOG");
  int log = log_path ? open(log_path, O_WRONLY | O_CREAT | O_APPEND, 0644) : -1;
  struct termios tio;
  if (tcgetattr(0, &tio) == 0) {
    tio.c_lflag &= ~(ICANON | ECHO | IEXTEN);
    tio.c_cc[VMIN] = 1;
    tio.c_cc[VTIME] = 0;
    tcsetattr(0, TCSANOW, &tio);
  }
  char b[4096]; ssize_t n;
  while ((n = read(0, b, sizeof b)) > 0) {
    if (log >= 0 && write(log, b, (size_t)n) < 0) return 1;
    if (write(1, b, (size_t)n) < 0) return 1;
  }
  return 0;
}
`;

export function pinnedHerdrVersion(): string {
  const manifest = path.resolve("..", "contracts/herdr-bundle.json");
  return (JSON.parse(fs.readFileSync(manifest, "utf8")) as { version: string }).version;
}

export function herdrBinary(): string {
  const candidates = [process.env.HIDE_E2E_HERDR_BIN, process.env.HERDR_BIN_PATH];
  for (const candidate of candidates) {
    if (candidate && fs.existsSync(candidate)) return candidate;
  }
  const onPath = spawnSync("/usr/bin/which", ["herdr"], { encoding: "utf8" }).stdout.trim();
  if (onPath) return onPath;
  throw new Error(
    "no herdr binary: set HIDE_E2E_HERDR_BIN or HERDR_BIN_PATH, or put the pinned herdr on PATH",
  );
}

/**
 * The agent kinds Herdr lists in panes whose cwd is `dir`: what an agent
 * start really left running, not what the start's answer said.
 */
export function agentsIn(fixture: HerdrFixture, dir: string): string[] {
  const real = fs.realpathSync(dir);
  const kinds: string[] = [];
  const visit = (value: unknown): void => {
    if (Array.isArray(value)) value.forEach(visit);
    else if (value && typeof value === "object") {
      const row = value as Record<string, unknown>;
      if (typeof row.pane_id === "string" && typeof row.agent === "string" && typeof row.cwd === "string" && fs.existsSync(row.cwd) && fs.realpathSync(row.cwd) === real) {
        kinds.push(row.agent);
      }
      Object.values(row).forEach(visit);
    }
  };
  visit(fixture.run(["agent", "list"]));
  return kinds;
}

/**
 * Whether Herdr itself has `pane` focused. The shell draws a focus the moment
 * it asks for one and Herdr applies it a little later, and two requests in
 * flight at once can be applied in either order, so a spec that asks for a
 * second focus waits for Herdr to hold the first.
 */
export function herdrHasFocus(fixture: HerdrFixture, pane: string): boolean {
  const listed = fixture.run(["pane", "list"]) as { result: { panes: { pane_id: string; focused: boolean }[] } };
  return listed.result.panes.find((row) => row.pane_id === pane)?.focused === true;
}

function herdr(env: NodeJS.ProcessEnv, bin: string, args: string[]): unknown {
  const out = execFileSync(bin, args, { env, encoding: "utf8", timeout: 30_000 });
  return JSON.parse(out) as unknown;
}

function isolatedEnv(root: string, socket: string): NodeJS.ProcessEnv {
  const env = Object.fromEntries(Object.entries(process.env).filter(
    ([key]) => !["HERDR_", "HIDE_", "ELECTRON_", "HCOORD_", "SASU_"].some((prefix) => key.startsWith(prefix)),
  ));
  const config = path.join(root, "herdr-config.toml");
  fs.writeFileSync(config, "[update]\nversion_check = false\nmanifest_check = false\n");
  for (const dir of ["xdg-config", "xdg-state", "home", "fixture", "bin"]) {
    fs.mkdirSync(path.join(root, dir), { recursive: true });
  }
  // Herdr starts the pane shell from SHELL, so the fixture names it: the
  // CI runner's login shell is bash, and only zsh reads this private
  // HOME's .zshrc. A fixed prompt keeps the workstation's user and host
  // name out of screenshots and is what the fixture waits for.
  fs.writeFileSync(path.join(root, "home", ".zshrc"), "PS1='fixture %# '\n");
  return {
    ...env,
    SHELL: "/bin/zsh",
    // Ubuntu's /etc/zsh/zshrc runs compinit before this HOME's .zshrc, and on
    // a Linux runner compinit stops at "insecure directories, continue [y] or
    // abort [n]?", so the prompt never comes. That file skips compinit when
    // this parameter is set. It rides the environment, not a .zshenv, because
    // specs write their own .zshenv to put the fake agent first on PATH and
    // would drop it. macOS has no such file.
    skip_global_compinit: "1",
    HOME: path.join(root, "home"),
    HERDR_SESSION: `hide-e2e-${path.basename(root)}`,
    HERDR_SOCKET_PATH: socket,
    HERDR_CONFIG_PATH: config,
    XDG_CONFIG_HOME: path.join(root, "xdg-config"),
    XDG_STATE_HOME: path.join(root, "xdg-state"),
    HERDR_DISABLE_SOUND: "1",
  };
}

function paneRead(env: NodeJS.ProcessEnv, bin: string, pane: string): { text: string; failure: string | null } {
  const result = spawnSync(bin, ["pane", "read", pane, "--source", "visible", "--format", "text"], {
    env,
    encoding: "utf8",
    timeout: 10_000,
  });
  if (result.status === 0) return { text: result.stdout, failure: null };
  return { text: "", failure: `pane read exited ${result.status}: ${result.stderr.trim()}` };
}

function paneText(env: NodeJS.ProcessEnv, bin: string, pane: string): string {
  return paneRead(env, bin, pane).text;
}

/**
 * `detail` is read once, when the wait fails, so the error names what the
 * predicate last saw; a bare "timed out" cannot say whether the shell
 * printed something else or nothing at all.
 */
async function waitFor(predicate: () => boolean, what: string, ms = 10_000, detail?: () => string): Promise<void> {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    if (predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`timed out waiting for ${what}${detail ? `; last seen: ${detail()}` : ""}`);
}

/** What the fixture provider answers for one transcript. */
export type FixtureLabel = {
  /** 8 to 30 characters: the core refuses a shorter title. */
  task: string;
  /** How long the provider takes to answer. */
  delayMs?: number;
  progress?: string;
  /** The reply the agent asks for; the label's line when given, with `question` its end. */
  reply?: string;
  question?: boolean;
  /** How the turn ended (v5 `end`); a question by default when `question`, else done. */
  end?: "working" | "question" | "done" | "waiting" | "unfinished";
};

/** Where the fixture's Claude transcripts live: the fixture HOME's, which
 * `startHided` links into the daemon's HOME. */
export function claudeProjects(fixture: Pick<HerdrFixture, "root">): string {
  return path.join(fixture.root, "home", ".claude", "projects");
}

/**
 * The daemon reads agent transcripts under its own HOME; the fixture writes
 * them under its own, before or after the daemon starts. One folder for both
 * keeps every label the fixture writes readable by a daemon on `home`.
 */
export function linkFixtureTranscripts(fixture: Pick<HerdrFixture, "root">, home: string): void {
  const own = claudeProjects(fixture);
  const daemon = path.join(home, ".claude", "projects");
  fs.mkdirSync(own, { recursive: true });
  if (path.resolve(daemon) === path.resolve(own) || fs.existsSync(daemon)) return;
  fs.mkdirSync(path.dirname(daemon), { recursive: true });
  fs.symlinkSync(own, daemon);
}

/**
 * A Claude transcript for `sessionId`: one operator turn and the agent's
 * answer, which carries the label the fixture provider returns. Written
 * before the session is declared, because the core reads a session when its
 * reference or state changes, not when its file grows.
 */
/** The text a transcript carries for the fixture provider to answer with. */
export function labelMarker(label: FixtureLabel): string {
  const answer = {
    goal: label.task,
    goal_changed: true,
    // v5 has one line: the reply asked for when there is one, else the progress.
    line: label.reply ?? label.progress ?? "",
    end: label.end ?? (label.question ? "question" : "done"),
  };
  const delay = label.delayMs ? `HIDE_E2E_DELAY_MS ${label.delayMs}\n` : "";
  return `${delay}HIDE_E2E_LABEL ${JSON.stringify(answer)}`;
}

export function writeFixtureTranscript(projects: string, sessionId: string, label: FixtureLabel): string {
  const records = [
    { type: "user", sessionId, timestamp: "2026-10-01T09:00:00Z", origin: { kind: "human" }, message: { role: "user", content: `${label.task} 진행해줘` } },
    { type: "assistant", sessionId, timestamp: "2026-10-01T09:00:01Z", message: { role: "assistant", content: [{ type: "text", text: labelMarker(label) }] } },
  ];
  const dir = path.join(projects, "e2e");
  fs.mkdirSync(dir, { recursive: true });
  const file = path.join(dir, `${sessionId}.jsonl`);
  fs.writeFileSync(file, records.map((record) => `${JSON.stringify(record)}\n`).join(""));
  return file;
}

/**
 * Appends a turn carrying `label` to the transcript of `sessionId`: the same
 * session, so a parent relationship declared for it still holds. The core
 * reads the new turn on the agent's next state change; the request carries
 * the label too, so the analysis at the turn's start answers it as well.
 */
export function continueFixtureTranscript(fixture: Pick<HerdrFixture, "root">, sessionId: string, label: FixtureLabel): void {
  const records = [
    { type: "user", sessionId, timestamp: "2026-10-01T09:01:00Z", origin: { kind: "human" }, message: { role: "user", content: `${label.task} 이어서 진행해줘\n${labelMarker(label)}` } },
    { type: "assistant", sessionId, timestamp: "2026-10-01T09:01:01Z", message: { role: "assistant", content: [{ type: "text", text: labelMarker(label) }] } },
  ];
  fs.appendFileSync(path.join(claudeProjects(fixture), "e2e", `${sessionId}.jsonl`), records.map((record) => `${JSON.stringify(record)}\n`).join(""));
}

/**
 * The native session id the fixture gives `pane`'s agent. A native id is
 * letters, digits, `.`, `_` and `-`, and the core's transcript read refuses
 * any other, so the pane id's `:` becomes `-`.
 */
export function fixtureSessionId(pane: string): string {
  return `fixture-${pane.replaceAll(":", "-")}`;
}

/** The native session id `pane`'s agent reports now. */
export function sessionOf(fixture: Pick<HerdrFixture, "run">, pane: string): string {
  type Agent = { pane_id: string; agent_session?: { value: string } };
  const listed = fixture.run(["agent", "list"]) as { result: { agents: Agent[] } };
  const value = listed.result.agents.find((agent) => agent.pane_id === pane)?.agent_session?.value;
  if (!value) throw new Error(`pane ${pane} reports no agent session`);
  return value;
}

/** The last session sequence each pane declared; a replacement must be newer. */
const sessionSeqs = new Map<string, number>();

/** Controlled fake-agent provenance. This publishes the explicit test session,
 * not a transcript-discovery claim; native isolation tests use raw CLI reports. */
function declareFixtureSession(env: NodeJS.ProcessEnv, bin: string, pane: string, kind: string, sessionId: string, replacing = false): void {
  const key = `${env.HERDR_SOCKET_PATH}|${pane}`;
  const seq = replacing ? (sessionSeqs.get(key) ?? 1) + 1 : 1;
  sessionSeqs.set(key, seq);
  execFileSync(bin, ["pane", "report-agent-session", pane, "--source", `herdr:${kind}`, "--agent", kind, "--agent-session-id", sessionId, "--seq", String(seq), ...(replacing ? ["--session-start-source", "clear"] : [])], { env, timeout: 30_000 });
}

/**
 * Declares `child` as spawned by `parent` the way hided writes it: the
 * parent's pane and the digest of each pane's session, which Hide compares with
 * the session each pane reports now (docs: docs/status-model.md#where-a-parent-comes-from).
 * Both panes must already hold an agent session; `herdr agent list` names it.
 */
export function declareParent(fixture: Pick<HerdrFixture, "bin" | "env">, child: string, parent: string): void {
  type Agent = { pane_id: string; agent_session?: { value: string } };
  const listed = JSON.parse(execFileSync(fixture.bin, ["agent", "list"], { env: fixture.env, encoding: "utf8", timeout: 30_000 })) as { result: { agents: Agent[] } };
  const session = (pane: string): string => {
    const value = listed.result.agents.find((agent) => agent.pane_id === pane)?.agent_session?.value;
    if (!value) throw new Error(`pane ${pane} reports no agent session, so its relationship could not be written`);
    return crypto.createHash("sha256").update(value, "utf8").digest("hex");
  };
  execFileSync(fixture.bin, ["pane", "report-metadata", child, "--source", "e2e-lineage", "--token", `parent_pane=${parent}`, "--token", `child_session=${session(child)}`, "--token", `parent_session=${session(parent)}`], { env: fixture.env, timeout: 30_000 });
}

export function setFixtureSession(fixture: HerdrFixture, pane: string, sessionId: string): void {
  declareFixtureSession(fixture.env, fixture.bin, pane, "claude", sessionId, true);
}

let labelSessions = 0;

/**
 * Gives `pane` a new Claude session whose transcript the core analyzes into
 * `label`. A new session, because the same session is read again only when
 * the agent's state changes.
 */
export function labelAgent(fixture: HerdrFixture, pane: string, label: FixtureLabel): string {
  const sessionId = `label-${process.pid}-${(labelSessions += 1)}`;
  writeFixtureTranscript(claudeProjects(fixture), sessionId, label);
  setFixtureSession(fixture, pane, sessionId);
  return sessionId;
}

/** Drive the fake Claude's actual screen detector while retaining its native
 * session identity. Lifecycle hook reports may be ignored after declaration.
 *
 * `idle` ends the turn: Herdr reports `done` when the pane's tab is not the
 * one its clients show, and `idle` when it is, so either answers it. */
export async function setFixtureLifecycle(fixture: HerdrFixture, pane: string, state: "working" | "blocked" | "idle"): Promise<string> {
  type Agent = { pane_id: string; agent_status: string; agent_session?: { source: string; agent: string; kind: string; value: string } };
  const current = () => (fixture.run(["agent", "list"]) as { result: { agents: Agent[] } }).result.agents.find((agent) => agent.pane_id === pane);
  const reference = current()?.agent_session;
  if (!reference || reference.source !== "herdr:claude" || reference.agent !== "claude") {
    throw new Error("lifecycle fixture requires its declared Claude native session");
  }
  // The existing raw-mode shim echoes these bytes, so pinned Herdr's
  // osc_title_working / bash_permission_prompt rules see a controlled TUI.
  // Leaving a permission prompt clears it off the screen, where Herdr would
  // still read it as blocked; nothing else touches the pane's text.
  const clear = reference && current()?.agent_status === "blocked" ? "\x1b[2J\x1b[H" : "";
  const screen = state === "working"
    ? `${clear}\x1b]0;\u280b Working\x07`
    : state === "idle"
      ? `${clear}\x1b]0;\u2733 Claude Code\x07`
      : "\x1b]0;Fixture\x07\x1b[2J\x1b[Hdo you want to proceed?\n"
        + "bash command\n❯ 1. Yes\n2. No\n";
  const reached = (status: string | undefined) => (state === "idle" ? status === "idle" || status === "done" : status === state);
  execFileSync(fixture.bin, ["pane", "send-text", pane, screen], { env: fixture.env, timeout: 30_000 });
  await waitFor(() => reached(current()?.agent_status), `native fixture state ${state}`, 10_000,
    () => JSON.stringify({ expected: state, observed: current()?.agent_status }));
  const observed = current()!;
  if (JSON.stringify(observed.agent_session) !== JSON.stringify(reference)) {
    throw new Error("lifecycle fixture changed its declared native session");
  }
  // Detection provenance is run evidence, never product source.
  const explanation = execFileSync(fixture.bin, ["agent", "explain", pane, "--json"], { env: fixture.env, encoding: "utf8", timeout: 30_000 });
  const evidence = process.env.HIDE_E2E_SCREENSHOT_DIR;
  if (evidence) {
    fs.mkdirSync(evidence, { recursive: true });
    fs.writeFileSync(path.join(evidence, `lifecycle-${pane.replaceAll(":", "-")}-${state}.json`),
      JSON.stringify({ expected: state, observed: observed.agent_status, nativeSessionUnchanged: true, explanation: JSON.parse(explanation) }));
  }
  return observed.agent_status;
}

/** A new tab in the fixture's workspace, which no agent runs in: Herdr's
 * clients look at it while a turn ends elsewhere, so that turn ends unseen. */
export function elsewhereTab(fixture: HerdrFixture): string {
  const created = fixture.run(["tab", "create", "--workspace", fixture.workspace, "--label", "elsewhere", "--env", `PATH=${fixture.fixturePath}`, "--no-focus"]) as {
    result: { tab: { tab_id: string } };
  };
  return created.result.tab.tab_id;
}

/**
 * A finished turn the operator has not seen: the pane works, then stops
 * while its tab is not the one Herdr's clients show, which Herdr reports as
 * `done`. `elsewhere` is another tab; focusing it moves Herdr's clients off
 * the pane's tab first.
 */
export async function finishFixtureTurn(fixture: HerdrFixture, pane: string, elsewhere: string): Promise<void> {
  fixture.run(["tab", "focus", elsewhere]);
  await setFixtureLifecycle(fixture, pane, "working");
  const status = await setFixtureLifecycle(fixture, pane, "idle");
  if (status !== "done") throw new Error(`pane ${pane} stopped as ${status}, not done: its tab is still shown`);
}

export async function startHerdr({ agents = true }: { agents?: boolean } = {}): Promise<HerdrFixture> {
  const bin = herdrBinary();
  const version = execFileSync(bin, ["--version"], { encoding: "utf8" }).trim().split(/\s+/)[1];
  const pinned = pinnedHerdrVersion();
  if (version !== pinned) {
    throw new Error(`herdr ${version} at ${bin} is not the pinned ${pinned}`);
  }
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hide-e2e-herdr-"));
  // Unix socket paths are short; keep the node directly under /tmp.
  const socket = `/tmp/hide-e2e-${crypto.randomBytes(4).toString("hex")}.sock`;
  const env = isolatedEnv(root, socket);
  try {
    fs.writeFileSync(path.join(root, "shim.c"), SHIM_SOURCE);
    execFileSync("cc", ["-O1", "-o", path.join(root, "bin", "claude"), path.join(root, "shim.c")]);
  } catch (error) {
    fs.rmSync(root, { recursive: true, force: true });
    throw error;
  }
  // Keep host-installed providers out while retaining tools such as lsof.
  const fixturePath = `${path.join(root, "bin")}:/usr/bin:/bin:/usr/sbin:/sbin`;
  // Workspaces created later inherit the server's PATH, not hided's PATH.
  // Keep every pane on the same fake agent binary, including new workspaces.
  env.PATH = fixturePath;

  const log = fs.openSync(path.join(root, "herdr-server.log"), "w");
  const server: ChildProcess = spawn(bin, ["server"], { env, stdio: ["ignore", log, log] });
  const { stop } = ownUntilWorkerExit(() => {
    spawnSync(bin, ["server", "stop"], { env, timeout: 10_000 });
    if (server.exitCode === null) server.kill("SIGKILL");
    for (const file of [socket, socket.replace(/\.sock$/, "-client.sock")]) {
      fs.rmSync(file, { force: true });
    }
    // Run evidence for a reviewer, next to the screenshots: the server's log
    // and what each agent pane's PTY received.
    const keep = process.env.HIDE_E2E_SCREENSHOT_DIR;
    if (keep) {
      for (const name of ["herdr-server.log", "input-one.log", "input-two.log"]) {
        const file = path.join(root, name);
        if (fs.existsSync(file)) fs.copyFileSync(file, path.join(keep, `${path.basename(root)}-${name}`));
      }
      // The server's own session log, where a pane whose shell never printed
      // a prompt says what it ran and how that ended.
      const sessions = path.join(root, "xdg-config", "herdr", "sessions");
      if (fs.existsSync(sessions)) {
        for (const session of fs.readdirSync(sessions)) {
          const file = path.join(sessions, session, "herdr-server.log");
          if (fs.existsSync(file)) fs.copyFileSync(file, path.join(keep, `${path.basename(root)}-session.log`));
        }
      }
    }
    fs.rmSync(root, { recursive: true, force: true });
  });
  try {
    await waitFor(() => fs.existsSync(socket), `herdr socket ${socket}`);
    const snapshot = herdr(env, bin, ["api", "snapshot"]) as {
      result?: { snapshot?: { workspaces?: unknown[] } };
    };
    const workspaces = snapshot.result?.snapshot?.workspaces ?? [];
    if (workspaces.length !== 0) throw new Error("private herdr server already has workspaces");

    const inputLogs: [string, string] = [path.join(root, "input-one.log"), path.join(root, "input-two.log")];
    const created = herdr(env, bin, [
      "workspace",
      "create",
      "--cwd",
      path.join(root, "fixture"),
      "--label",
      "e2e",
      "--env",
      `PATH=${fixturePath}`,
      "--env",
      `HIDE_E2E_INPUT_LOG=${inputLogs[0]}`,
      "--focus",
    ]) as { result: { workspace: { workspace_id: string }; tab: { tab_id: string }; root_pane: { pane_id: string } } };
    const first = created.result.root_pane.pane_id;
    const split = herdr(env, bin, [
      "pane",
      "split",
      first,
      "--direction",
      "right",
      "--env",
      `PATH=${fixturePath}`,
      "--env",
      `HIDE_E2E_INPUT_LOG=${inputLogs[1]}`,
      "--no-focus",
    ]) as { result: { pane: { pane_id: string } } };
    const second = split.result.pane.pane_id;
    // The shell must have printed its prompt before agent start accepts the
    // pane; the fixture .zshrc makes that prompt a fixed string.
    for (const pane of [first, second]) {
      await waitFor(
        () => paneText(env, bin, pane).includes("fixture %"),
        `a prompt in pane ${pane}`,
        10_000,
        () => JSON.stringify(paneRead(env, bin, pane)),
      );
    }
    if (agents) {
      herdr(env, bin, ["agent", "start", "one", "--kind", "claude", "--pane", first]);
      herdr(env, bin, ["agent", "start", "two", "--kind", "claude", "--pane", second]);
      // Distinct row labels, made by the core from each pane's transcript.
      for (const [pane, task] of [
        [first, "Agent one"],
        [second, "Agent two"],
      ]) {
        writeFixtureTranscript(path.join(root, "home", ".claude", "projects"), fixtureSessionId(pane!), { task: task! });
        declareFixtureSession(env, bin, pane!, "claude", fixtureSessionId(pane!));
      }
    }
    return {
      bin,
      socket,
      env,
      root,
      workspace: created.result.workspace.workspace_id,
      tab: created.result.tab.tab_id,
      panes: [first, second],
      inputLogs,
      fixturePath,
      run: (args) => {
        const result = herdr(env, bin, args);
        if (args[0] === "agent" && args[1] === "start") {
          const pane = args[args.indexOf("--pane") + 1];
          const kind = args[args.indexOf("--kind") + 1];
          if (args.includes("--pane") && ["claude", "codex"].includes(kind!))
            declareFixtureSession(env, bin, pane!, kind!, fixtureSessionId(pane!));
        }
        return result;
      },
      stop,
    };
  } catch (error) {
    stop();
    throw error;
  }
}

/**
 * A workspace of its own at `cwd` with one agent in it, recorded as spawned
 * by `parent` the way hided records it (`declareParent`) when one is named.
 * With `named`, the agent runs a session labelled so before the relationship
 * is written for it, since a later session is no longer that child's.
 * Returns the new pane.
 */
export async function spawnAgent(fixture: HerdrFixture, label: string, parent: string | null, cwd = path.join(fixture.root, label), named?: FixtureLabel): Promise<string> {
  fs.mkdirSync(cwd, { recursive: true });
  const created = fixture.run(["workspace", "create", "--cwd", cwd, "--label", label, "--env", `PATH=${fixture.fixturePath}`, "--no-focus"]) as {
    result: { root_pane: { pane_id: string } };
  };
  const pane = created.result.root_pane.pane_id;
  await waitFor(() => paneText(fixture.env, fixture.bin, pane).includes("fixture %"), `a prompt in pane ${pane}`, 20_000, () => JSON.stringify(paneRead(fixture.env, fixture.bin, pane)));
  fixture.run(["agent", "start", label, "--kind", "claude", "--pane", pane]);
  if (named) labelAgent(fixture, pane, named);
  if (parent) declareParent(fixture, pane, parent);
  return pane;
}
