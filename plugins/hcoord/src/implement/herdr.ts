import { spawnSync } from "node:child_process";

/**
 * The entire surface the harness is allowed to touch herdr through.
 *
 * Three holes for the run: start an implementor, read what its pane says,
 * ask whether it is still alive. Two more for the supervisor: read one
 * agent's identity and lifecycle (`agent get`) and submit a wake to it
 * (`agent prompt`). Everything else the old observer did - routing
 * contracts, PRD gate checks, dispatch, a 250ms wait loop and then a
 * one-shot `await` waiter - is now either a CLI command or gone, because
 * wake-up is owned by the supervisor tick that re-reads state.json and
 * herdr every interval rather than by terminal text or a background
 * command's exit (D-05, D-13).
 *
 * herdr is a convenience, never a contract: the harness must work in a bare
 * terminal, under launchd, and in CI. So every hole reports itself
 * unavailable rather than throwing, and the run degrades to "no pane
 * diagnosis" instead of stopping (R9).
 *
 * Every argv below was measured against herdr 0.8.2 on 2026-09-07, after all
 * three holes were found dead against a herdr that was answering normally.
 * See AGENT_LIST_ARGV for the single flag that caused it. `agent get` and
 * `agent prompt` were measured against herdr 0.9.1 on 2026-09-18.
 */
export type HerdrHole = "spawn" | "read" | "alive";

/**
 * The pane this process occupies. A dispatch reads it to learn which agent
 * kind the supervisor runs, so the implementor is the same kind by default;
 * that makes it the spawn hole's own precondition and nothing else's: a
 * missing value closes `spawn` alone and leaves pane diagnosis and liveness
 * open.
 */
const PANE_ID_ENV_KEY = "HERDR_PANE_ID";

/**
 * The structural Implementor marker, injected when the pane is created.
 *
 * It cannot move into the handoff text. An unmarked pane routes as a
 * supervisor and may dispatch recursively, and a marker that lives only in a
 * prompt is a request for discipline rather than a guard (AGENTS.md Review
 * Guide 7). This is what forces the create-then-start dispatch below.
 */
const ROLE_ENV_MARKER = "SASU_HERDR_ROLE=implementor";

/**
 * One argv for listing agents, shared by diagnostics, kind detection and
 * the liveness hole.
 *
 * Both used to spell it themselves and both spelled it `agent list --json`,
 * which herdr 0.8.2 rejects with exit 2: `agent list` already prints JSON and
 * defines no `--json` flag (measured 2026-09-07). Because the probe is what
 * every hole consults first, that one wrong flag reported spawn, read AND
 * alive unavailable in a session where herdr was answering fine - the whole
 * adapter was dead and its unit tests, which mock `run`, could not see it.
 * One constant so the two callers cannot drift apart again.
 */
const AGENT_LIST_ARGV = ["agent", "list"];

/**
 * The herdr this adapter's argv was measured against. Reported only when herdr
 * rejects a call, so the failure can name what moved instead of leaving a
 * reader to diff two CLIs by hand.
 */
const ADAPTER_TARGET = { version: "0.9.1", protocol: "22" };

/**
 * herdr's own signal that it did not understand the call.
 *
 * This distinction is the guard. The probe used to call every non-zero exit
 * "present but not answering", so when `agent list --json` was rejected -
 * herdr answering perfectly, this adapter's flag wrong - the harness blamed
 * herdr, `sasu implement status` repeated the blame, and nobody suspected the
 * adapter. An adapter has to be able to say "I am the one who is out of date"
 * (2026-09-07). Measured: argv rejection exits 2 with a `usage:` line, a
 * working call exits 0, and neither is how an absent server fails.
 */
function isArgvRejection(probe: { status: number | null; stdout: string; stderr: string }): boolean {
  if (probe.status !== 2) return false;
  return /(^|\n)\s*usage:/i.test(`${probe.stderr}${probe.stdout}`);
}

/**
 * Read the installed herdr's version and protocol. Called only on the
 * rejection path: the happy path must not pay two extra subprocesses for a
 * string nobody reads.
 */
function installedHerdr(run: NonNullable<HerdrEnvironment["run"]>): string {
  const version = run(["--version"]).stdout.trim();
  const protocol = /protocol:\s*(\S+)/.exec(run(["api", "schema"]).stdout)?.[1] ?? "an unreported protocol";
  return `${version === "" ? "an unreported version" : version}, protocol ${protocol}`;
}

export const HERDR_TIMEOUT_KILL_SIGNAL = "SIGKILL";

export interface HerdrCapabilities {
  available: boolean;
  /** Per-hole availability, so status can name what specifically is missing. */
  holes: Record<HerdrHole, boolean>;
  /** Why the unavailable holes are unavailable, in one human-readable line. */
  reason: string | null;
}

export interface HerdrEnvironment {
  env?: NodeJS.ProcessEnv;
  run?: (args: string[], cwd?: string, timeoutMs?: number) => { status: number | null; stdout: string; stderr: string; errorCode?: string };
  /** Wall clock and blocking sleep; injected by tests, so a 30 s wait costs a test nothing. */
  clock?: HerdrClock;
}

export interface HerdrClock {
  now(): number;
  sleep(ms: number): void;
}

function defaultRun(args: string[], cwd?: string, env: NodeJS.ProcessEnv = process.env, timeoutMs = 15_000): { status: number | null; stdout: string; stderr: string; errorCode?: string } {
  // A 2026-09-21 deadline probe stayed blocked until a SIGTERM-ignoring child
  // exited on its own. Deadline-bound adapter children are disposable CLI
  // calls, so non-cooperative termination is required to release their owner.
  const executed = spawnSync(env["HERDR_BIN_PATH"] || "herdr", args, {
    cwd,
    env,
    encoding: "utf8",
    shell: false,
    // Official agent start may use its 30 s readiness window. Give that call
    // a matching process deadline while ordinary probes retain their 15 s default.
    timeout: Math.max(1, Math.min(35_000, Math.floor(timeoutMs))),
    killSignal: HERDR_TIMEOUT_KILL_SIGNAL,
  });
  if (executed.error !== undefined) return { status: null, stdout: "", stderr: String(executed.error), errorCode: (executed.error as NodeJS.ErrnoException).code };
  return { status: executed.status, stdout: executed.stdout ?? "", stderr: executed.stderr ?? "" };
}

/** Share the measured Herdr argv/process boundary with the coordinator. */
export function runHerdrCommand(args: string[], timeoutMs = 15_000, env: NodeJS.ProcessEnv = process.env): { status: number | null; stdout: string; stderr: string; errorCode?: string } {
  return defaultRun(args, undefined, env, timeoutMs);
}

const environmentRun = (environment: HerdrEnvironment): NonNullable<HerdrEnvironment["run"]> =>
  environment.run ?? ((args, cwd, timeoutMs) => defaultRun(args, cwd, environment.env, timeoutMs));

const defaultClock: HerdrClock = {
  now: () => Date.now(),
  // The adapter is synchronous end to end (spawnSync), so the wait between
  // two start attempts is a blocking sleep, the same idiom gates/store.ts
  // uses for its lock backoff.
  sleep: (ms) => { Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms); },
};

/** The structured code herdr prints on stderr when it refuses a call, or null when it printed none. */
export function herdrErrorCode(stderr: string): string | null {
  const parsed = parseJson(stderr) as { error?: { code?: unknown } } | null;
  return typeof parsed?.error?.code === "string" ? parsed.error.code : null;
}

/**
 * How long a dispatch keeps retrying `agent start` on `agent_pane_busy`.
 *
 * A new pane's zsh takes a few seconds to read its profile and print a
 * prompt, and herdr accepts a pane as an agent target only once it has seen
 * that interactive prompt. Measured 2026-09-10 (herdr 0.8.2): that moment
 * comes several seconds after `pane process-info` first reports a bare shell,
 * so a start issued straight after the creation was refused twice in a row with
 * `agent_pane_busy: agent target pane is not an available shell`, and the
 * dispatch closed an empty pane that would have been ready moments later.
 * herdr's own acceptance is the only reliable readiness signal, so exactly
 * that refusal is retried once a second for up to 30 s; any other failure
 * surfaces at once. Task Factory's dispatcher started its pilot Observer the
 * same day with this shape and never missed.
 */
const AGENT_START_BUSY_RETRY_MS = 1_000;
const AGENT_START_BUSY_TIMEOUT_MS = 30_000;
const CODEX_INITIALIZATION_TIMEOUT_MS = 30_000;
const CODEX_INITIALIZATION_POLL_MS = 250;

/** Retry only Herdr's definite new-shell busy refusal, as dispatch already does. */
export function startAgentWhenPaneReady(argv: string[], environment: HerdrEnvironment = {}, cwd?: string): { result: ReturnType<NonNullable<HerdrEnvironment["run"]>>; busyRetries: number; elapsedMs: number } {
  const run = environmentRun(environment);
  const clock = environment.clock ?? defaultClock;
  const firstStartAt = clock.now();
  let result = run(argv, cwd, 35_000);
  let busyRetries = 0;
  while (result.status !== 0 && herdrErrorCode(result.stderr) === "agent_pane_busy"
    && clock.now() - firstStartAt < AGENT_START_BUSY_TIMEOUT_MS) {
    clock.sleep(AGENT_START_BUSY_RETRY_MS);
    busyRetries += 1;
    result = run(argv, cwd, 35_000);
  }
  return { result, busyRetries, elapsedMs: clock.now() - firstStartAt };
}

// Codex 0.156.0 creates/reports its session at its first turn, not at the
// empty composer (observed 2026-09-24). Submit this only after Herdr has
// finished agent start; model work inside its readiness wait can lose the name.
export const CODEX_INITIALIZATION_PROMPT = "Session initialization only. Reply with READY and end this turn. Do not use tools, read or edit files, run commands, start implementation, or dispatch agents. The actual task will arrive separately after the coordinator verifies your session identity.";

/** An official ready agent can still be showing a first-run menu. Clear only
 * the known update offer before the first model turn; hook consent is external
 * authority and must never be silently skipped. */
export function prepareCodexFirstTurn(prepared: Pick<PreparedSpawn, "paneId" | "name">, environment: HerdrEnvironment, advanceUpdate = true): AgentLookup {
  const run = environmentRun(environment);
  const clock = environment.clock ?? defaultClock;
  const deadline = clock.now() + CODEX_INITIALIZATION_TIMEOUT_MS;
  let initialTerminal: string | null = null;
  let advanced = false;
  while (clock.now() < deadline) {
    const observed = getAgent(prepared.paneId, environment, Math.min(2000, deadline - clock.now()));
    if (observed.kind !== "found") return observed;
    const agent = observed.agent;
    if (agent.paneId !== prepared.paneId || agent.name !== prepared.name || agent.kind !== "codex" || agent.terminalId === null
      || (initialTerminal !== null && agent.terminalId !== initialTerminal)) return { kind: "unavailable", detail: "Codex first-turn target changed before submission" };
    initialTerminal = agent.terminalId;
    if (agent.status === "blocked" || agent.interactiveReady !== true || (agent.status !== "idle" && agent.status !== "done")) return { kind: "unavailable", detail: "Codex first-turn target is not interactive-ready" };
    if (agent.sessionId !== null) return observed;
    const screen = run(["agent", "read", prepared.paneId, "--source", "visible"], undefined, Math.min(2000, deadline - clock.now()));
    if (screen.status !== 0) return { kind: "unavailable", detail: "Codex first-turn screen could not be inspected" };
    if (/Hooks need review/.test(screen.stdout)) return { kind: "unavailable", detail: "Codex hooks need explicit fixture or user consent before session initialization" };
    if (/Updating Codex via/.test(screen.stdout)) return { kind: "unavailable", detail: "Codex is updating; first-turn submission was withheld" };
    const hasComposer = /›\s*Ask Codex to do anything/.test(screen.stdout);
    const hasUpdateMenu = /Update available[\s\S]*2\. Skip(?:\s|$)/.test(screen.stdout);
    if (hasComposer && hasUpdateMenu) return { kind: "unavailable", detail: "Codex first-turn screen has conflicting composer and update signals; no input was sent" };
    if (hasUpdateMenu) {
      if (!advanceUpdate || advanced) return { kind: "unavailable", detail: "Codex update screen is not safely cleared before the first turn" };
      const current = getAgent(prepared.paneId, environment, Math.min(2000, deadline - clock.now()));
      const repeated = run(["agent", "read", prepared.paneId, "--source", "visible"], undefined, Math.min(2000, deadline - clock.now()));
      if (current.kind !== "found" || current.agent.name !== prepared.name || current.agent.terminalId !== initialTerminal
        || current.agent.sessionId !== null || current.agent.interactiveReady !== true || repeated.status !== 0 || repeated.stdout !== screen.stdout) return { kind: "unavailable", detail: "Codex update screen changed before Skip could be selected" };
      const selected = run(["agent", "send-keys", prepared.paneId, "2", "enter"], undefined, Math.min(2000, deadline - clock.now()));
      if (selected.status !== 0) return { kind: "unavailable", detail: "Codex update screen could not be skipped" };
      advanced = true;
      clock.sleep(CODEX_INITIALIZATION_POLL_MS);
      continue;
    }
    // Herdr reports idle and interactive_ready for a Codex startup menu too.
    // The visible composer is the only affirmative TUI signal available in
    // official 0.9.1, so an unknown first-run screen cannot receive Enter.
    if (!hasComposer) return { kind: "unavailable", detail: "Codex first-turn composer is not visible; no prompt was submitted" };
    return observed;
  }
  return { kind: "unavailable", detail: "Codex first-turn readiness did not settle in 30 seconds" };
}

export function environmentCapabilities(environment: HerdrEnvironment): HerdrCapabilities {
  const env = environment.env ?? process.env;
  if (env["HERDR_ENV"] !== "1") {
    return {
      available: false,
      holes: { spawn: false, read: false, alive: false },
      reason: "not running under herdr (HERDR_ENV is not 1); the supervisor keeps its event wake-up and verb channel, and loses pane diagnosis",
    };
  }
  if (paneId(env) === "") {
    return {
      available: false,
      holes: { spawn: false, read: true, alive: true },
      reason: `herdr is configured but ${PANE_ID_ENV_KEY} is unset, so a dispatch cannot tell which agent kind it is dispatching from; pane diagnosis and liveness still work, and starting a replacement is the supervisor's to perform by hand`,
    };
  }
  return { available: true, holes: { spawn: true, read: true, alive: true }, reason: null };
}

/** Diagnostic probe only; no operation depends on another operation succeeding. */
export function herdrCapabilities(environment: HerdrEnvironment = {}): HerdrCapabilities {
  const base = environmentCapabilities(environment);
  if (!base.holes.read) return base;
  const run = environmentRun(environment);
  const probe = run([...AGENT_LIST_ARGV]);
  if (probe.status !== 0) {
    const call = `herdr ${AGENT_LIST_ARGV.join(" ")}`;
    const reason = isArgvRejection(probe)
      ? `herdr rejected this adapter's own call (\`${call}\` exited 2 with a usage line), so the herdr contract moved and THIS ADAPTER is out of date, not herdr: cli/src/implement/herdr.ts was measured against herdr ${ADAPTER_TARGET.version} protocol ${ADAPTER_TARGET.protocol} and the installed herdr reports ${installedHerdr(run)}. Re-measure its argv against \`herdr <subcommand> --help\`; the unit suite's contract test makes the same comparison`
      : `herdr is present but not answering (\`${call}\` exited ${probe.status ?? "without status"}); the supervisor keeps its event wake-up and verb channel, and loses agent-list liveness`;
    return { available: false, holes: { ...base.holes, alive: false }, reason: `agent listing unavailable: ${reason}; read and startup keep their own preconditions` };
  }
  const listed = parseJson(probe.stdout) as { result?: { agents?: unknown } } | null;
  if (!Array.isArray(listed?.result?.agents)) {
    return { available: false, holes: { ...base.holes, alive: false }, reason: "agent listing unavailable: invalid agent list; read and startup keep their own preconditions" };
  }
  return base;
}

function paneId(env: NodeJS.ProcessEnv): string {
  return env[PANE_ID_ENV_KEY]?.trim() ?? "";
}

export interface HoleResult<T> {
  ok: boolean;
  value: T | null;
  /** Set when the hole was unavailable or the call failed. */
  problem: string | null;
}

function parseJson(stdout: string): unknown {
  const trimmed = stdout.trim();
  if (trimmed === "") return null;
  try {
    return JSON.parse(trimmed);
  } catch {
    return null;
  }
}

/**
 * One agent as herdr 0.8.2 reports it.
 *
 * `name` appears only on agents started under a name, and the status field is
 * spelled `agent_status` - not `status`, which this adapter used to read and
 * therefore always found undefined.
 */
interface AgentEntry {
  name?: string;
  agent?: string;
  agent_status?: string;
  pane_id?: string;
}

function listAgents(run: NonNullable<HerdrEnvironment["run"]>): { agents: AgentEntry[]; problem: string | null } {
  const executed = run([...AGENT_LIST_ARGV]);
  if (executed.status !== 0) {
    return { agents: [], problem: `herdr ${AGENT_LIST_ARGV.join(" ")} failed (${executed.status ?? "no status"})` };
  }
  const parsed = parseJson(executed.stdout) as { result?: { agents?: AgentEntry[] } } | null;
  const agents = parsed?.result?.agents;
  if (!Array.isArray(agents) || agents.some((entry) => entry === null || typeof entry !== "object"
    || (entry.name !== undefined && typeof entry.name !== "string")
    || (entry.pane_id !== undefined && typeof entry.pane_id !== "string")
    || (entry.agent !== undefined && typeof entry.agent !== "string"))) {
    return { agents: [], problem: "herdr agent list returned an invalid agent list; liveness is unknown" };
  }
  return { agents, problem: null };
}

/**
 * Native launch arguments for the agent kind being started.
 *
 * herdr passes everything after `--` straight to the agent executable, so the
 * translation is per-CLI and measured rather than guessed (2026-09-07):
 * `claude --model <m> --effort <level>`, `codex --model <m> -c
 * model_reasoning_effort="<level>"`.
 */
function nativeAgentArgs(kind: string, model?: string, effort?: string): string[] {
  const args: string[] = [];
  if (model !== undefined && model !== "") args.push("--model", model);
  if (effort !== undefined && effort !== "") {
    if (kind === "codex") args.push("--config", `model_reasoning_effort="${effort}"`);
    else args.push("--effort", effort);
  }
  return args.length === 0 ? [] : ["--", ...args];
}

export function initializedCodex(prepared: Pick<PreparedSpawn, "paneId" | "name">, environment: HerdrEnvironment, requireSettled = true): AgentLookup {
  const clock = environment.clock ?? defaultClock;
  const deadline = clock.now() + CODEX_INITIALIZATION_TIMEOUT_MS;
  let terminalId: string | null = null;
  let sessionId: string | null = null;
  while (clock.now() < deadline) {
    const observed = getAgent(prepared.paneId, environment, deadline - clock.now());
    if (observed.kind !== "found") return observed;
    const agent = observed.agent;
    if (agent.paneId !== prepared.paneId || agent.name !== prepared.name || agent.kind !== "codex") {
      return { kind: "unavailable", detail: "Codex initialization target is missing or no longer matches the created pane" };
    }
    terminalId ??= agent.terminalId;
    if ((terminalId !== null && agent.terminalId !== terminalId) || (sessionId !== null && agent.sessionId !== sessionId)) {
      return { kind: "unavailable", detail: "Codex initialization identity changed; refusing the replacement" };
    }
    sessionId ??= agent.sessionId;
    if (agent.status === "blocked") return { kind: "unavailable", detail: "Codex initialization is blocked; inspect the retained pane" };
    if (sessionId !== null && (!requireSettled || agent.status === "idle" || agent.status === "done")) return observed;
    if (terminalId !== null && sessionId === null && agent.status === "idle") {
      const run = environmentRun(environment);
      const screen = run(["agent", "read", prepared.paneId, "--source", "visible"], undefined, Math.min(2000, deadline - clock.now()));
      if (screen.status === 0) {
        if (/Hooks need review/.test(screen.stdout)) return { kind: "unavailable", detail: "Codex hooks need explicit consent; session integration cannot be skipped" };
        if (/Updating Codex via|Update available[\s\S]*2\. Skip(?:\s|$)/.test(screen.stdout)) return { kind: "unavailable", detail: "Codex startup screen appeared after first-turn submission; inspect the retained pane without further input" };
      }
    }
    const remaining = deadline - clock.now();
    if (remaining > 0) clock.sleep(Math.min(CODEX_INITIALIZATION_POLL_MS, remaining));
  }
  return { kind: "unavailable", detail: "Codex initialization did not produce a settled, identified session within 30 seconds; inspect the retained pane" };
}

/**
 * Where the implementor's pane is created.
 *
 * Never a split of the supervisor's pane. Hide groups every pane under the
 * Herdr workspace that owns it, so an implementor split beside the Observer
 * was listed under the Observer's checkout (the root worktree) however far
 * away its worktree was, and it sat in the operator's own layout. A run
 * isolated into a worktree gets a workspace of its own on that path, which
 * is the only way hide can list the agent under the checkout it edits; an
 * in-place run gets a new tab in the workspace whose tree it edits.
 */
export type SpawnPlacement =
  | { kind: "workspace"; cwd: string; label: string }
  | { kind: "tab"; workspaceId: string; cwd: string; label: string };

export interface SpawnRequest {
  name: string;
  placement: SpawnPlacement;
  prompt: string;
  /** Defaults to the kind of the agent occupying the dispatching pane. */
  kind?: string;
  model?: string;
  effort?: string;
  /** Extra variables for the new pane's shell, on top of PATH and the role marker. */
  env?: Record<string, string>;
  /** Persists the exact created pane before an agent process is started. */
  afterCreate?: (prepared: PreparedSpawn) => void;
  /** Persists the exact started identity before any handoff bytes are submitted. */
  beforePrompt?: (started: SpawnResult) => void;
  /** Revalidates run authority after the final blocking identity lookup. */
  beforeSubmit?: (started: SpawnResult) => void;
}

/** The variable name of the role marker, so a caller cannot smuggle a second value for it. */
const ROLE_ENV_KEY = ROLE_ENV_MARKER.slice(0, ROLE_ENV_MARKER.indexOf("="));

/**
 * `--env` pairs for the new pane, beyond the role marker.
 *
 * A new pane's shell starts from the login PATH, not the dispatcher's.
 * Measured 2026-09-10: an Observer running a locally built sasu ahead of its
 * PATH dispatched an Implementor that could not see that build, and the
 * supervisor fell back to a hand-typed `herdr pane split --env PATH=...`. So
 * the dispatcher's own PATH always travels, a caller's pairs travel with it
 * (a caller's PATH wins over the inherited one, explicit over implicit), and
 * the marker is never among them: the recursion guard is not a pair anyone
 * gets to set.
 */
function paneEnvironment(processEnv: NodeJS.ProcessEnv, extra: Record<string, string>): { argv: string[]; problem: string | null } {
  if (ROLE_ENV_KEY in extra) {
    return { argv: [], problem: `${ROLE_ENV_KEY} is the role marker the dispatch sets itself; it cannot be passed as an extra variable` };
  }
  const inheritedPath = processEnv["PATH"] ?? "";
  const pairs: Record<string, string> = { ...(inheritedPath === "" ? {} : { PATH: inheritedPath }), ...extra };
  const argv = ["--env", ROLE_ENV_MARKER];
  for (const [key, value] of Object.entries(pairs)) argv.push("--env", `${key}=${value}`);
  return { argv, problem: null };
}

/**
 * The pane metadata token that names an agent's parent pane.
 *
 * herdr has no lineage of its own in its stable release, and its `agent new
 * --from-pane` (a fork-only method) cannot carry the role marker. So lineage
 * is declared the way every other app-level fact about a pane is declared:
 * as a display-only pane token, written by whoever created the pane. hide
 * reads `parent_pane` as the parent of the row and draws the child beneath
 * it; the token dies with the pane, so a closed implementor leaves no stale
 * edge behind. Any orchestrator can write the same token by hand:
 * `herdr pane report-metadata <child> --source <you> --token parent_pane=<parent>`.
 */
export const PARENT_PANE_TOKEN = "parent_pane";
const METADATA_SOURCE = "sasu";

export interface SpawnLineage {
  parentPaneId: string;
  /** Null when the token was written; otherwise why the row will show as a root. */
  problem: string | null;
}

export interface SpawnResult {
  paneId: string;
  workspaceId: string;
  tabId: string;
  name: string;
  kind: string;
  lineage: SpawnLineage;
  sessionId: string;
  terminalId: string;
  hostScope: string;
  recordedAt: string;
}

interface CreatedPane {
  paneId: string;
  workspaceId: string;
  tabId: string;
  /** Closes exactly what the creation made, and nothing the supervisor owns. */
  closeArgv: string[];
}

export interface PreparedSpawn {
  paneId: string;
  workspaceId: string;
  tabId: string;
  cwd: string;
  name: string;
  kind: string;
  placement: "workspace" | "tab";
  hostScope: string;
  parentPaneId: string;
  preparedAt: string;
}

/**
 * Create the pane the placement names: a workspace on the run's worktree, or
 * a tab in an existing workspace. Both answer with their root pane, which is
 * the shell the agent starts in (measured against herdr 0.9.0-preview
 * 2026-09-18: `workspace create` returns `.result.workspace`, `.result.tab`
 * and `.result.root_pane`; `tab create` returns the last two).
 */
function createPane(
  run: NonNullable<HerdrEnvironment["run"]>,
  placement: SpawnPlacement,
  environmentArgv: string[],
): { created: CreatedPane | null; problem: string | null } {
  const argv = placement.kind === "workspace"
    ? ["workspace", "create", "--cwd", placement.cwd, "--label", placement.label, ...environmentArgv, "--no-focus"]
    : ["tab", "create", "--workspace", placement.workspaceId, "--cwd", placement.cwd, "--label", placement.label, ...environmentArgv, "--no-focus"];
  const made = run(argv, placement.cwd);
  const call = `herdr ${argv[0]} ${argv[1]}`;
  if (made.status !== 0) {
    return { created: null, problem: `${call} at ${placement.cwd} failed (${made.status ?? "no status"}): ${(made.stderr || made.stdout).trim()}` };
  }
  const parsed = parseJson(made.stdout) as { result?: {
    workspace?: { workspace_id?: string }; tab?: { tab_id?: string }; root_pane?: { pane_id?: string };
  } } | null;
  const paneId = parsed?.result?.root_pane?.pane_id ?? "";
  const tabId = parsed?.result?.tab?.tab_id ?? "";
  const workspaceId = placement.kind === "workspace" ? (parsed?.result?.workspace?.workspace_id ?? "") : placement.workspaceId;
  if (paneId === "" || tabId === "" || workspaceId === "") {
    return { created: null, problem: `${call} reported no root pane, tab and workspace id; refusing to start an implementor into an unknown pane` };
  }
  // Recovery owns the root pane, not the workspace or tab that contains it.
  // Closing the container could destroy unrelated panes created after this
  // dispatch, so every cleanup path targets the exact pane it recorded.
  const closeArgv = ["pane", "close", paneId];
  return { created: { paneId, workspaceId, tabId, closeArgv }, problem: null };
}

/**
 * Hole 1: start an implementor agent in a pane of its own.
 *
 * Three calls, because herdr splits the two things a dispatch needs across
 * two commands (measured 2026-09-07, still true of 0.9.0-preview): `agent
 * new` is atomic and can record parent lineage with `--from-pane`, but
 * defines no `--env`; only `workspace create`, `tab create` and `pane split`
 * can set an environment variable on the launched shell. The role marker is
 * a correctness guard and lineage is an audit convenience, so the marker
 * wins and the implementor is created by create-then-start. The cost is real
 * and known: the dispatched agent does not appear under its supervisor in
 * herdr's agent tree.
 *
 * The pane is created first and the agent started second, so a failure after
 * the creation may leave either an empty shell or a blocked live agent. Only
 * a positively observed empty shell is closed automatically, because the
 * supervisor needs to look at it.
 */
export function spawnImplementor(
  input: SpawnRequest,
  environment: HerdrEnvironment = {},
): HoleResult<SpawnResult> {
  const capabilities = environmentCapabilities(environment);
  if (!capabilities.holes.spawn) return { ok: false, value: null, problem: `spawn unavailable: ${capabilities.reason}` };
  const run = environmentRun(environment);
  const dispatcher = paneId(environment.env ?? process.env);

  // The kind is the dispatching pane's own agent unless overridden: a
  // supervisor dispatches its own kind by default, and herdr requires --kind
  // on both `agent new` and `agent start`, so there is no "detect it for me".
  let kind = input.kind ?? "";
  if (kind === "") {
    const listed = listAgents(run);
    if (listed.problem !== null) return { ok: false, value: null, problem: listed.problem };
    kind = listed.agents.find((entry) => entry.pane_id === dispatcher)?.agent ?? "";
    if (kind === "") {
      return { ok: false, value: null, problem: `cannot detect the agent kind of the dispatching pane ${dispatcher}; pass an explicit kind` };
    }
  }

  const environmentArgv = paneEnvironment(environment.env ?? process.env, input.env ?? {});
  if (environmentArgv.problem !== null) return { ok: false, value: null, problem: environmentArgv.problem };
  const made = createPane(run, input.placement, environmentArgv.argv);
  if (made.created === null) return { ok: false, value: null, problem: made.problem };
  const cwd = input.placement.cwd;
  const created = made.created.paneId;
  const prepared: PreparedSpawn = {
    paneId: created, workspaceId: made.created.workspaceId, tabId: made.created.tabId, cwd,
    name: input.name, kind, placement: input.placement.kind,
    hostScope: (environment.env ?? process.env)["HERDR_SOCKET_PATH"]?.trim() || "default",
    parentPaneId: dispatcher, preparedAt: new Date().toISOString(),
  };
  try { input.afterCreate?.(prepared); }
  catch (error) {
    const closed = run(made.created.closeArgv, cwd);
    const cleanup = closed.status === 0 ? "the empty pane was closed" : `the empty pane could not be closed (${closed.status ?? "no status"})`;
    return { ok: false, value: null, problem: `pane ${created} was created, but pre-start persistence failed: ${error instanceof Error ? error.message : String(error)}; no agent was started and ${cleanup}` };
  }

  const startArgv = ["agent", "start", input.name, "--kind", kind, "--pane", created, ...nativeAgentArgs(kind, input.model, input.effort)];
  const { result: started, busyRetries, elapsedMs } = startAgentWhenPaneReady(startArgv, { ...environment, run }, cwd);
  if (started.status !== 0) {
    // A nonzero startup may leave a live trust dialog (2026-09-07).
    // Only a positively observed shell-only foreground can be cleaned up;
    // an absent name alone does not prove the new pane is empty.
    const code = herdrErrorCode(started.stderr);
    const waited = code === "agent_pane_busy"
      ? `; the pane never became an available shell in ${Math.round(elapsedMs / 1000)} s (${busyRetries} retries)`
      : "";
    let cleanup = `pane ${created} was retained for inspection; startup state is uncertain, and no handoff was sent`;
    if (code === "agent_not_ready") {
      cleanup = `agent ${input.name} is not ready; pane ${created} was retained for inspection, and no handoff was sent`;
    } else if (started.status !== null) {
      const observed = run(["pane", "process-info", "--pane", created]);
      const parsed = parseJson(observed.stdout) as { result?: { process_info?: {
        pane_id?: string; shell_pid?: number; foreground_process_group_id?: number;
        foreground_processes?: { pid?: number }[];
      } } } | null;
      const info = parsed?.result?.process_info;
      if (observed.status === 0 && info?.pane_id === created
        && Number.isInteger(info.shell_pid) && info.shell_pid! > 0
        && info.foreground_process_group_id === info.shell_pid
        && Array.isArray(info.foreground_processes) && info.foreground_processes.length === 1
        && info.foreground_processes[0]?.pid === info.shell_pid) {
        const closed = run(made.created.closeArgv);
        cleanup = closed.status === 0 ? "the empty pane was closed" : `the empty pane ${created} could not be closed (${closed.status ?? "no status"}) and is still open`;
      }
    }
    return { ok: false, value: null, problem: `herdr agent start ${input.name} --kind ${kind} in ${created} failed (${started.status ?? "no status"}): ${(started.stderr || started.stdout).trim()}${waited}; ${cleanup}` };
  }

  // Declared before the handoff so the row is already a child when the
  // agent's first output lands. A failed declaration is reported, never
  // fatal: the agent is live and the supervisor needs it prompted.
  const declared = run(["pane", "report-metadata", created, "--source", METADATA_SOURCE, "--token", `${PARENT_PANE_TOKEN}=${dispatcher}`], cwd);
  const lineage: SpawnLineage = {
    parentPaneId: dispatcher,
    problem: declared.status === 0 ? null
      : `herdr pane report-metadata ${created} failed (${declared.status ?? "no status"}): ${(declared.stderr || declared.stdout).trim()}; the row will show as a root, not under ${dispatcher}`,
  };

  if (kind === "codex") {
    const beforeInitialization = prepareCodexFirstTurn(prepared, { ...environment, run });
    if (beforeInitialization.kind !== "found" || beforeInitialization.agent.paneId !== created
      || beforeInitialization.agent.name !== input.name || beforeInitialization.agent.kind !== "codex"
      || beforeInitialization.agent.terminalId === null || beforeInitialization.agent.interactiveReady !== true
      || (beforeInitialization.agent.status !== "idle" && beforeInitialization.agent.status !== "done")) {
      return { ok: false, value: null, problem: `implementor ${input.name} in ${created} is not confirmed ready for session initialization; no handoff was sent` };
    }
    if (beforeInitialization.agent.sessionId === null) {
      const finalReady = prepareCodexFirstTurn(prepared, { ...environment, run }, false);
      if (finalReady.kind !== "found" || finalReady.agent.terminalId !== beforeInitialization.agent.terminalId || finalReady.agent.sessionId !== null) return { ok: false, value: null, problem: `implementor ${input.name} in ${created} changed before session initialization; no handoff was sent` };
      const initialized = promptAgent({ target: created, text: CODEX_INITIALIZATION_PROMPT, expectedInputGuard: finalReady.agent.inputGuard }, { ...environment, run });
      if (initialized.outcome !== "accepted") return { ok: false, value: null, problem: `implementor ${input.name} in ${created} initialization submission was ${initialized.outcome} (${initialized.code}); no handoff was sent` };
    }
  }
  const observed = kind === "codex" ? initializedCodex(prepared, { ...environment, run }) : getAgent(created, { ...environment, run });
  if (observed.kind !== "found" || observed.agent.paneId !== created || observed.agent.name !== input.name
    || observed.agent.sessionId === null || observed.agent.terminalId === null) {
    const detail = observed.kind === "found"
      ? `expected ${input.name} in ${created}, found ${observed.agent.name ?? "unnamed"} in ${observed.agent.paneId} with session ${observed.agent.sessionId ?? "missing"} and terminal ${observed.agent.terminalId ?? "missing"}`
      : observed.detail;
    return { ok: false, value: null, problem: `implementor ${input.name} is running in ${created}, but its exact identity could not be recorded before handoff: ${detail}; no handoff was sent` };
  }
  const identity: SpawnResult = {
    paneId: created, workspaceId: made.created.workspaceId, tabId: made.created.tabId, name: input.name, kind, lineage,
    sessionId: observed.agent.sessionId, terminalId: observed.agent.terminalId,
    hostScope: (environment.env ?? process.env)["HERDR_SOCKET_PATH"]?.trim() || "default", recordedAt: new Date().toISOString(),
  };
  try { input.beforePrompt?.(identity); }
  catch (error) {
    return { ok: false, value: null, problem: `implementor ${input.name} is running in ${created}, but pre-handoff persistence failed: ${error instanceof Error ? error.message : String(error)}; no handoff was sent` };
  }

  // Persistence can include several filesystem and index writes. Re-read the
  // exact pane after that work, because herdr 0.9.1 has no receiver-side
  // input guard and a replacement during the durable callback must not inherit
  // the executable handoff (independent review incident, 2026-09-20).
  const current = getAgent(created, { ...environment, run });
  if (current.kind !== "found" || current.agent.paneId !== identity.paneId || current.agent.name !== identity.name
    || current.agent.sessionId !== identity.sessionId || current.agent.terminalId !== identity.terminalId) {
    const detail = current.kind === "found"
      ? `found ${current.agent.name ?? "unnamed"} in ${current.agent.paneId}, session ${current.agent.sessionId ?? "missing"}, terminal ${current.agent.terminalId ?? "missing"}`
      : current.detail;
    return { ok: false, value: null, problem: `implementor identity changed after pre-handoff persistence: expected ${identity.name} in ${identity.paneId}, session ${identity.sessionId}, terminal ${identity.terminalId}; ${detail}; no handoff was sent` };
  }
  try { input.beforeSubmit?.(identity); }
  catch (error) {
    return { ok: false, value: null, problem: `implementor ${input.name} is running in ${created}, but final handoff authority validation failed: ${error instanceof Error ? error.message : String(error)}; no handoff was sent` };
  }

  const prompted = promptAgent({ target: created, text: input.prompt, expectedInputGuard: current.agent.inputGuard }, { ...environment, run });
  if (prompted.outcome !== "accepted") {
    // The prompt carries the whole handoff in one argv entry, and a failing
    // wrapper may echo argv. Never retain output for this call: it would
    // write the operator's verbatim context into the supervisor's log. The
    // started pane stays open - the agent is alive and the supervisor can
    // hand it the packet itself.
    return { ok: false, value: null, problem: `herdr agent prompt ${created} <redacted prompt> was not confirmed (${prompted.outcome}, ${prompted.code}); the implementor is running in ${created} with no handoff` };
  }
  return { ok: true, value: identity, problem: null };
}

/**
 * Close a pane that a persisted partial dispatch proves it created, but only
 * after Herdr proves the pane has no agent and is still a shell-only process.
 * This is the recovery half of the pre-start record: no guessed pane and no
 * coordinate-based cleanup can enter this path.
 */
export function closePreparedSpawn(prepared: PreparedSpawn, environment: HerdrEnvironment = {}): HoleResult<boolean> {
  const observed = getAgent(prepared.paneId, environment);
  if (observed.kind === "found") return { ok: false, value: null, problem: `prepared pane ${prepared.paneId} now holds an agent; refusing to close it` };
  if (observed.kind === "unavailable") return { ok: false, value: null, problem: observed.detail };
  const run = environmentRun(environment);
  const inspected = run(["pane", "process-info", "--pane", prepared.paneId], prepared.cwd);
  if (inspected.status !== 0) {
    const code = herdrErrorCode(inspected.stderr) ?? herdrErrorCode(inspected.stdout);
    if (code === "pane_not_found" || code === "agent_not_found") return { ok: true, value: true, problem: null };
    return { ok: false, value: null, problem: `cannot inspect prepared pane ${prepared.paneId}: ${(inspected.stderr || inspected.stdout).trim()}` };
  }
  const parsed = parseJson(inspected.stdout) as { result?: { process_info?: {
    pane_id?: string; shell_pid?: number; foreground_process_group_id?: number; foreground_processes?: { pid?: number }[];
  } } } | null;
  const info = parsed?.result?.process_info;
  const shellOnly = info?.pane_id === prepared.paneId
    && Number.isInteger(info.shell_pid) && info.shell_pid! > 0
    && info.foreground_process_group_id === info.shell_pid
    && Array.isArray(info.foreground_processes) && info.foreground_processes.length === 1
    && info.foreground_processes[0]?.pid === info.shell_pid;
  if (!shellOnly) return { ok: false, value: null, problem: `prepared pane ${prepared.paneId} is not a proven empty shell; refusing to close it` };
  const closed = run(["pane", "close", prepared.paneId], prepared.cwd);
  return closed.status === 0
    ? { ok: true, value: true, problem: null }
    : { ok: false, value: null, problem: `failed to close empty prepared pane ${prepared.paneId}: ${(closed.stderr || closed.stdout).trim()}` };
}

/** Hole 2: read an agent's recent output, for diagnosis only. */
export function readPane(
  input: { name: string; lines?: number },
  environment: HerdrEnvironment = {},
): HoleResult<string> {
  const capabilities = environmentCapabilities(environment);
  if (!capabilities.holes.read) return { ok: false, value: null, problem: `read unavailable: ${capabilities.reason}` };
  const run = environmentRun(environment);
  const executed = run(["agent", "read", input.name, "--source", "recent-unwrapped", "--lines", String(input.lines ?? 120)]);
  if (executed.status !== 0) {
    return { ok: false, value: null, problem: `herdr agent read ${input.name} failed (${executed.status ?? "no status"}): ${(executed.stderr || executed.stdout).trim()}` };
  }
  return { ok: true, value: executed.stdout, problem: null };
}

/**
 * Hole 3: is this agent still running?
 *
 * Presence in the list is the whole answer. herdr 0.8.2's AgentStatus enum is
 * idle | working | blocked | done | unknown - none of which means dead, and a
 * dead agent simply leaves the list (measured 2026-09-07). This used to test
 * `status !== "exited"` against a field that is spelled `agent_status` and a
 * value that never occurs, so every listed agent and every typo answered
 * "alive".
 */
export function isAgentAlive(
  input: { name: string },
  environment: HerdrEnvironment = {},
): HoleResult<boolean> {
  const capabilities = environmentCapabilities(environment);
  if (!capabilities.holes.alive) return { ok: false, value: null, problem: `alive unavailable: ${capabilities.reason}` };
  const listed = listAgents(environmentRun(environment));
  if (listed.problem !== null) return { ok: false, value: null, problem: listed.problem };
  return { ok: true, value: listed.agents.some((entry) => entry.name === input.name), problem: null };
}



/**
 * One live agent as `agent get <pane|name>` answers it (herdr 0.9.1,
 * measured 2026-09-18). `agent_session.value` is the runtime's own session
 * UUID and the only identity that survives a herdr server restart;
 * `terminal_id` does not survive one.
 * `input_guard` is absent on 0.9.1 and present only on the guarded-prompt
 * fork (modakbul-gongbang/herdr#3).
 */
export interface AgentObservation {
  paneId: string;
  name: string | null;
  kind: string;
  sessionId: string | null;
  terminalId: string | null;
  status: "idle" | "working" | "blocked" | "done" | "unknown";
  stateChangeSeq: number | null;
  interactiveReady: boolean | null;
  inputGuard: string | null;
}

export type AgentLookup =
  | { kind: "found"; agent: AgentObservation }
  /** herdr answered and the target holds no agent: it exited, was closed, or never existed. */
  | { kind: "absent"; detail: string }
  /** herdr did not answer usefully; this proves nothing about the agent. */
  | { kind: "unavailable"; detail: string };

const AGENT_STATUSES = new Set(["idle", "working", "blocked", "done", "unknown"]);

/**
 * Hole 4: what agent is in this pane right now.
 *
 * Not gated on HERDR_ENV: the supervisor tick runs under launchd with no
 * herdr variables at all, and the socket answering is the only availability
 * that matters to it. A pane that herdr cannot find is `absent` (exit 1,
 * `agent_not_found`, measured 2026-09-18); any other failure is
 * `unavailable`, which the caller must not read as absence (D-06).
 */
export function getAgent(target: string, environment: HerdrEnvironment = {}, timeoutMs?: number): AgentLookup {
  const run = environmentRun(environment);
  const executed = run(["agent", "get", target], undefined, timeoutMs);
  if (executed.status !== 0) {
    const code = herdrErrorCode(executed.stderr) ?? herdrErrorCode(executed.stdout);
    if (executed.status === 1 && code === "agent_not_found") return { kind: "absent", detail: `herdr lists no agent at ${target}` };
    return { kind: "unavailable", detail: `herdr agent get ${target} failed (${executed.status ?? "no status"}${code === null ? "" : `, ${code}`}): ${(executed.stderr || executed.stdout).trim().slice(0, 200)}` };
  }
  const parsed = parseJson(executed.stdout) as { result?: { type?: string; agent?: Record<string, unknown> } } | null;
  const raw = parsed?.result?.agent;
  if (parsed?.result?.type !== "agent_info" || raw === undefined || raw === null || typeof raw !== "object") {
    return { kind: "unavailable", detail: `herdr agent get ${target} returned no agent_info record` };
  }
  const text = (value: unknown): string | null => typeof value === "string" && value !== "" ? value : null;
  const paneId = text(raw["pane_id"]);
  const kind = text(raw["agent"]);
  if (paneId === null || kind === null) return { kind: "unavailable", detail: `herdr agent get ${target} returned an agent without pane_id or agent kind` };
  const status = text(raw["agent_status"]) ?? "unknown";
  const session = raw["agent_session"];
  return {
    kind: "found",
    agent: {
      paneId,
      name: text(raw["name"]),
      kind,
      sessionId: session !== null && typeof session === "object" ? text((session as Record<string, unknown>)["value"]) : null,
      terminalId: text(raw["terminal_id"]),
      status: (AGENT_STATUSES.has(status) ? status : "unknown") as AgentObservation["status"],
      stateChangeSeq: typeof raw["state_change_seq"] === "number" ? raw["state_change_seq"] : null,
      interactiveReady: typeof raw["interactive_ready"] === "boolean" ? raw["interactive_ready"] : null,
      inputGuard: text(raw["input_guard"]),
    },
  };
}

/**
 * How a submission ended. `unknown` is the honest third value: a timeout or
 * an unparseable failure may already have delivered bytes, so the caller
 * must not resend on it (engineering 11).
 */
export interface PromptOutcome {
  outcome: "accepted" | "rejected" | "unknown";
  /** `session-match` when the plain prompt was used; `guarded` when herdr checked the input guard itself. */
  path: "session-match" | "guarded";
  code: string;
  detail: string;
}

/** Codes herdr returns before any input reaches the pane (measured 2026-09-18 on 0.9.1; guard codes from the fork). */
const REJECTED_BEFORE_INPUT = new Set([
  "agent_not_found", "agent_name_not_found", "agent_blocked", "agent_not_ready", "agent_pane_not_found",
  "agent_input_guard_mismatch", "guarded_prompt_unsupported", "herdr_spawn_failed",
]);

const PROCESS_DID_NOT_START = new Set(["ENOENT", "EACCES", "ENOEXEC"]);

/**
 * Hole 5: submit one wake.
 *
 * With a guard the call carries `--expected-input-guard` and herdr itself
 * refuses the input when the pane's agent has changed since the guard was
 * read; that closes the get-then-prompt window structurally. Without one
 * the caller has already matched the session UUID and terminal (D-06) and
 * the plain prompt is sent. A guarded request never falls back to the plain
 * path: herdr 0.9.1 rejects the flag with exit 2 and `unknown option`
 * (measured 2026-09-18), which is reported as `guarded_prompt_unsupported`
 * so the mismatch between "guard offered" and "guard refused" is visible
 * instead of silently downgraded (D-07).
 *
 * The text is one argv element passed to spawnSync without a shell, so
 * whatever state.json, git or herdr put into a wake line reaches the pane as
 * literal keystrokes and never as a command. A refactor to a shell string
 * would reintroduce that injection path.
 */
export function promptAgent(
  input: { target: string; text: string; expectedInputGuard: string | null },
  environment: HerdrEnvironment = {},
  timeoutMs?: number,
): PromptOutcome {
  const run = environmentRun(environment);
  const guarded = input.expectedInputGuard !== null;
  const argv = ["agent", "prompt", input.target, input.text, ...(guarded ? ["--expected-input-guard", input.expectedInputGuard!] : [])];
  const executed = run(argv, undefined, timeoutMs);
  const path: PromptOutcome["path"] = guarded ? "guarded" : "session-match";
  if (executed.status === 0) {
    if (guarded) {
      const payload = parseJson(executed.stdout) as { result?: { outcome?: string } } | null;
      if (payload?.result?.outcome !== "submitted") {
        return { outcome: "unknown", path, code: "guarded_prompt_invalid_response", detail: "herdr returned success without a submitted acknowledgement" };
      }
    }
    return { outcome: "accepted", path, code: "submitted", detail: guarded ? "herdr submitted the wake at the guarded input boundary" : "herdr submitted the wake" };
  }
  const structured = herdrErrorCode(executed.stderr) ?? herdrErrorCode(executed.stdout);
  const usage = /unknown option/i.test(executed.stderr) || /unknown option/i.test(executed.stdout);
  const code = structured ?? (executed.status === 2 && guarded && usage
    ? "guarded_prompt_unsupported"
    : executed.errorCode !== undefined
      ? PROCESS_DID_NOT_START.has(executed.errorCode) ? "herdr_spawn_failed" : executed.errorCode === "ETIMEDOUT" ? "herdr_prompt_timeout" : "herdr_prompt_failed"
      : executed.status === null ? "herdr_prompt_timeout" : "herdr_prompt_failed");
  const detail = (executed.stderr || executed.stdout).trim().slice(0, 300) || "herdr returned no diagnostic";
  return {
    outcome: REJECTED_BEFORE_INPUT.has(code) ? "rejected" : "unknown",
    path,
    code,
    detail: code === "guarded_prompt_unsupported"
      ? `${detail}; the installed herdr offered an input guard but refuses --expected-input-guard, so no wake was sent and none will be sent unguarded`
      : detail,
  };
}

/**
 * Whether the installed herdr accepts `--expected-input-guard`, read from
 * its own help text. Reported by `sasu supervisor status` and `doctor` so an
 * operator can tell "guard offered but refused" from "no guard on this
 * herdr" (B11, B17). Diagnostic only; the tick decides per wake from the
 * `input_guard` field herdr actually returns.
 */
export function guardedPromptSupport(environment: HerdrEnvironment = {}): { supported: boolean | null; detail: string | null } {
  const run = environmentRun(environment);
  const help = run(["agent", "prompt", "--help"]);
  if (help.status !== 0) return { supported: null, detail: `herdr agent prompt --help failed (${help.status ?? "no status"}): ${(help.stderr || help.stdout).trim().slice(0, 200)}` };
  return { supported: /--expected-input-guard/.test(`${help.stdout}${help.stderr}`), detail: null };
}
