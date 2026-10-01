import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { API_VERSION, HcoordError, LETTER_OPERATIONS, MAX_AGENTS, MAX_CONNECTIONS, MAX_EVENTS, MAX_LEDGER_BYTES, MAX_MESSAGE_BYTES, MAX_OUTBOX_LETTERS, MAX_QUEUE, REMOTE_PROTOCOL, SPAWN_EVENT_SLOTS, own, put, sameExecution, type Ledger, type LetterRecord } from "./model";
import { event } from "./model";
import { execute, recordLetter, watchForRequest } from "./service";
import { outboxCount, parseLetter, readOutbox, removeLetters, type Found, type RawLetter } from "./outbox";
import { blockedSpawnError, confirmSpawnPane, createSpawnPane, createSpawnWorktree, observedPlacement, discoverAgents, officialDeliveryAvailable, OFFICIAL_PROMPT_BOUNDARY, inspectDelivery, inspectParticipant, inspectSpawnedAgent, nameSpawnedAgent, observeStartedAgent, parentPlacement, prepareSpawnInitialization, startSpawnedAgent, submitOfficial, submitSpawnInitialization, validateBinding, waitForSpawnInitialization } from "./herdr";
import type { SpawnIntent } from "./model";
import { dataDir, ledgerPath, loadLedger, saveLedger, socketPath, stopMarkerPath } from "./store";
import { notifyHuman, notifyText } from "./platform";
import { createFailureLog } from "./failure-log";
import { isLocalMachine, remoteCall, remoteCallAsync, requireRemoteHerdr, remoteOutcome, savedMachine, type Raw } from "./remote";
import { reconcileAlert, recordClean, recordReady, recordStart } from "./health";
import { carriesLineage, clearLineage, hostsAnotherSession, lineageCurrent, readRouteLineage, writeLineage, type LineageWrite } from "./lineage";
import { machineId } from "./identity";
import { HCOORD_VERSION } from "./version";

/** Whether this process reached the daemon on its last call; the CLI derives its health warning from it. */
export let lastDaemonContact: "answered" | "unreachable" | null = null;

export interface WireRequest { version: number; operation: string; args: Record<string, unknown> }
export interface WireResult { ok: boolean; value?: unknown; error?: { code: string; message: string; detail?: Record<string, unknown> }; observedAt: string; delivery?: "delivered" | "pending" }
/** A local letter younger than this waits for its writer's own collect call before the sweep takes it. */
const LOCAL_SWEEP_GRACE_MS = 2000;
const SWEEP_LETTERS_PER_TICK = 16;
// Remote letters reach the HQ only when it collects them (PRD risk: delivery
// lags by this interval). Chosen for a ~3 s SSH round trip measured to mini on
// 2026-09-24; after a refusal the machine waits the longer backoff.
const COLLECT_INTERVAL_MS = 5000;
const COLLECT_BACKOFF_MS = 30_000;
// One log line per machine and cause per this window while a collection keeps failing the same way.
const COLLECT_FAILURE_LOG_WINDOW_MS = 10 * 60_000;
const COLLECT_LETTERS = 64;
// A local `pane list` costs one short Herdr call, so a lost token comes back
// within seconds; a remote one is an SSH round trip inside the operation
// queue, so it runs as rarely as the unreachable backoff.
const LOCAL_LINEAGE_MS = 5000;
const REMOTE_LINEAGE_MS = 60_000;
// A write Herdr refused is not retried every pass.
const LINEAGE_WRITE_RETRY_MS = 5 * 60_000;
const mutation = (operation: string): boolean => !["status", "agent.list", "agent.show", "watch.list", "request.show", "inbox", "graph", "events"].includes(operation);

export async function callDaemon(operation: string, args: Record<string, unknown> = {}, home = os.homedir(), timeoutOverrideMs?: number): Promise<WireResult> {
  if (process.platform === "win32") throw new HcoordError("unsupported_platform", "Windows named-pipe ACL support is unverified; no local daemon connection was attempted");
  const request = `${JSON.stringify({ version: API_VERSION, operation, args })}\n`;
  if (Buffer.byteLength(request) > MAX_MESSAGE_BYTES) throw new HcoordError("capacity", `request exceeds ${MAX_MESSAGE_BYTES} bytes; shorten context or native arguments before retrying`);
  return await new Promise<WireResult>((resolve, reject) => {
    const socket = net.createConnection(socketPath(home));
    let text = "";
    const timeoutMs = timeoutOverrideMs ?? (operation === "agent.spawn" ? 75_000 : 10_000);
    const timer = setTimeout(() => { socket.destroy(); lastDaemonContact = "unreachable"; reject(new HcoordError("timeout", `coordinator did not answer within ${timeoutMs / 1000} seconds`)); }, timeoutMs);
    const finish = (error?: Error, value?: WireResult): void => { clearTimeout(timer); socket.destroy(); if (error) reject(error); else resolve(value!); };
    socket.on("connect", () => socket.write(request));
    socket.on("data", (chunk: Buffer) => {
      text += chunk.toString("utf8");
      if (Buffer.byteLength(text) > MAX_MESSAGE_BYTES) return finish(new HcoordError("capacity", "coordinator response exceeded message limit"));
      const newline = text.indexOf("\n");
      if (newline === -1) return;
      try { const parsed = JSON.parse(text.slice(0, newline)) as WireResult; lastDaemonContact = "answered"; finish(undefined, parsed); }
      catch { finish(new HcoordError("protocol", "coordinator returned invalid JSON")); }
    });
    socket.on("error", (error: NodeJS.ErrnoException) => {
      clearTimeout(timer);
      lastDaemonContact = "unreachable";
      if (error.code === "ENOENT" || error.code === "ECONNREFUSED") reject(new HcoordError("daemon_down", "coordinator daemon is not running; start it with hcoord daemon start"));
      else if (error.code === "EACCES" || error.code === "EPERM") reject(new HcoordError("permission_denied", "coordinator socket access was denied; allow this session to connect to the local user socket, then retry"));
      else reject(new HcoordError("transport", "coordinator transport failed; inspect the local socket and daemon log before retrying"));
    });
  });
}

export function staleRead(operation: string, args: Record<string, unknown> = {}, home = os.homedir()): WireResult {
  if (mutation(operation)) throw new HcoordError("daemon_down", "coordinator daemon is not running; start it with hcoord daemon start");
  const saved = fs.existsSync(ledgerPath(home));
  const state = loadLedger(home);
  const observedAt = new Date().toISOString();
  const outcome = execute(state, operation, args, observedAt);
  const data = operation === "agent.list" ? { items: (outcome.value as Array<Record<string, unknown>>).map((item) => ({ registered: true, ...item })), partialFailures: ["daemon stopped; Herdr discovery unavailable"], observedAt: state.updatedAt }
    : operation === "status" ? { ...(outcome.value as object), deliverySafety: OFFICIAL_PROMPT_BOUNDARY } : outcome.value;
  return { ok: true, value: { data, stale: true, lastObservedAt: saved ? state.updatedAt : null, warning: saved ? "daemon stopped: automatic watch, reminders, and delivery are inactive" : "daemon stopped and no saved observation exists" }, observedAt };
}

/**
 * Runs the daemon until a stop request or signal. Returns "manual_stop"
 * without starting when the user stopped it, so a KeepAlive supervisor that
 * restarts only failed exits leaves the manual stop in place (PRD B13).
 */
export async function runDaemon(home = os.homedir()): Promise<"stopped" | "manual_stop"> {
  if (process.platform === "win32") throw new HcoordError("unsupported_platform", "Windows local IPC needs a verified user-restricted named pipe adapter");
  if (fs.existsSync(stopMarkerPath(home))) return "manual_stop";
  fs.mkdirSync(dataDir(home), { recursive: true, mode: 0o700 });
  fs.chmodSync(dataDir(home), 0o700);
  let ledger = loadLedger(home);
  // Read the process identity once before the server starts. A status reply
  // must describe the daemon that answered, not the CLI that asked it.
  const daemonIdentity = {
    hcoordVersion: HCOORD_VERSION,
    apiVersion: API_VERSION,
    machineId: machineId(),
    runtime: { executable: process.execPath, cli: path.resolve(__dirname, "cli.js") },
  };
  const socketFile = socketPath(home);
  const lockFile = `${socketFile}.lock`;
  let lockOwned = false, socketOwned = false;
  const acquireLock = (): void => {
    try { fs.writeFileSync(lockFile, `${process.pid}\n`, { flag: "wx", mode: 0o600 }); lockOwned = true; return; }
    catch (error) { if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error; }
    const recovery = `${lockFile}.recovery`;
    const owner = `${recovery}/owner`;
    const alive = (pid: number): boolean => {
      if (!Number.isSafeInteger(pid) || pid <= 0) return false;
      try { process.kill(pid, 0); return true; }
      catch (error) { if ((error as NodeJS.ErrnoException).code === "ESRCH") return false; throw error; }
    };
    let recoveryOwned = false;
    for (let attempt = 0; attempt < 3 && !recoveryOwned; attempt += 1) {
      try { fs.mkdirSync(recovery, { mode: 0o700 }); recoveryOwned = true; }
      catch (error) {
        if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;
        let ownerPid: number | null = null;
        try { ownerPid = Number(fs.readFileSync(owner, "utf8").trim()); }
        catch (readError) { if ((readError as NodeJS.ErrnoException).code !== "ENOENT") throw readError; }
        if (ownerPid !== null ? alive(ownerPid) : Date.now() - fs.statSync(recovery).mtimeMs < 10_000) throw new HcoordError("startup_in_progress", "another daemon is starting or recovering; retry after it finishes");
        try { if (ownerPid !== null) fs.unlinkSync(owner); fs.rmdirSync(recovery); }
        catch (cleanupError) { if ((cleanupError as NodeJS.ErrnoException).code !== "ENOENT") throw cleanupError; }
      }
    }
    if (!recoveryOwned) throw new HcoordError("startup_in_progress", "another daemon is recovering; retry after it finishes");
    try {
      fs.writeFileSync(owner, `${process.pid}\n`, { flag: "wx", mode: 0o600 });
      const pid = Number(fs.readFileSync(lockFile, "utf8").trim());
      if (alive(pid)) throw new HcoordError("already_running", "coordinator daemon is running or starting");
      fs.unlinkSync(lockFile);
      fs.writeFileSync(lockFile, `${process.pid}\n`, { flag: "wx", mode: 0o600 });
      lockOwned = true;
    } finally { if (fs.existsSync(owner)) fs.unlinkSync(owner); fs.rmdirSync(recovery); }
  };
  acquireLock();
  try {
  if (fs.existsSync(socketFile)) {
    try { await callDaemon("status", {}, home); throw new HcoordError("already_running", "coordinator daemon is already running"); }
    catch (error) { if (!(error instanceof HcoordError) || error.code !== "daemon_down") throw error; }
    fs.unlinkSync(socketFile);
  }
  let connections = 0;
  let queuedOperations = 0;
  let processing = Promise.resolve();
  let tickPending = false;
  let closing = false;
  let lastRetentionAt = 0;
  const commit = (operation: string, args: Record<string, unknown>, at: string, letter?: LetterRecord): unknown => {
    const next = structuredClone(ledger);
    const outcome = execute(next, operation, args, at);
    if (letter) recordLetter(next, letter);
    if (outcome.changed || letter) { saveLedger(next, home); ledger = next; }
    return outcome.value;
  };
  const lineageFor = (participant: import("./model").Participant): LineageWrite | null => {
    if (participant.parent === null) return null;
    const parent = own(ledger.participants, participant.parent);
    if (!parent) {
      return { status: "failed", parentPane: "unknown", parentMachine: null, message: `agent is running but parent ${participant.parent} is absent from the ledger`, nextAction: "restore or re-register the exact parent, then retry the same register or spawn intent" };
    }
    let result: LineageWrite;
    try { result = writeLineage(parent, participant); }
    catch (error) {
      const message = error instanceof Error ? error.message : "lineage token write failed";
      result = { status: "failed", parentPane: parent.pane ?? "unknown", parentMachine: null, message: `agent is running but its lineage tokens were not recorded: ${message}`, nextAction: "retry the same register or spawn intent; hcoord will not create another pane or agent" };
    }
    if (result.status === "failed") {
      try {
        const next = structuredClone(ledger);
        event(next, new Date().toISOString(), "lineage.token_failed", participant.id, null, { parent: parent.id, machine: participant.machine, reason: result.message });
        saveLedger(next, home); ledger = next;
      } catch (error) {
        process.stderr.write(`${JSON.stringify({ event: "hcoord.lineage_failure_log_failed", at: new Date().toISOString(), participant: participant.id, parent: parent.id, code: error instanceof HcoordError ? error.code : "internal" })}\n`);
      }
    }
    return result;
  };
  const spawnAgent = (args: Record<string, unknown>, at: string): unknown => {
    let intent = commit("agent.spawn.reserve", args, at) as SpawnIntent;
    const requireEventSlots = (slots: number, unfinishedStep: string): void => {
      if (MAX_EVENTS - ledger.events.length < slots) throw new HcoordError("capacity", "event history has insufficient room to finish the saved spawn; resolve retention before retrying", { intent: intent.key, pane: intent.pane, unfinishedStep });
    };
    const requireSpawnStorage = (steps: number, unfinishedStep: string): void => {
      // The daemon serializes operations. Reserve one maximum wire payload per
      // remaining progress write before any external effect can add an identity.
      const available = MAX_LEDGER_BYTES - fs.statSync(ledgerPath(home)).size;
      if (available < steps * MAX_MESSAGE_BYTES) throw new HcoordError("capacity", "ledger has insufficient byte headroom to finish the saved spawn; resolve retention before retrying", { intent: intent.key, pane: intent.pane, unfinishedStep, availableBytes: available });
    };
    const reconcilePane = args["reconcilePane"];
    if (reconcilePane !== null && reconcilePane !== undefined && (typeof reconcilePane !== "string" || reconcilePane.trim() === "")) throw new HcoordError("invalid_argument", "--reconcile-pane requires an exact pane ID");
    if (intent.pane !== null && reconcilePane !== null && reconcilePane !== undefined && reconcilePane !== intent.pane) throw new HcoordError("identity_conflict", "the spawn intent already owns a different pane", { intent: intent.key, pane: intent.pane });
    if (intent.status === "complete") return { intent, participant: ledger.participants[intent.participant!], watch: ledger.watches[intent.participant!] ?? null };
    if (Object.keys(ledger.participants).length >= MAX_AGENTS) throw new HcoordError("capacity", "participant limit has no room for this saved spawn; resolve retention before retrying", { intent: intent.key, pane: intent.pane });
    let createdNow = false;
    if (intent.pane === null) {
      const parent = ledger.participants[intent.parent]!;
      if (intent.status !== "reserved") {
        if (typeof reconcilePane !== "string" || reconcilePane.trim() === "") throw new HcoordError("spawn_uncertain", "tab creation outcome is unknown; inspect the original tab and retry this intent with --reconcile-pane <exact-pane-id>", { intent: intent.key, pane: null, unfinishedStep: "record_pane" });
        // Older persisted intents lack placement, so retain their parent placement check.
        // A worktree's placement was never recorded; the named pane's own placement is adopted.
        const placement = intent.worktree ? observedPlacement(intent, reconcilePane) : intent.placement ?? parentPlacement(parent);
        confirmSpawnPane({ ...intent, pane: reconcilePane }, placement);
        const found = inspectSpawnedAgent({ ...intent, pane: reconcilePane });
        if (found.state === "absent" && args["resumeStart"] !== true) throw new HcoordError("spawn_uncertain", "pane is confirmed but has no agent; retry with --reconcile-pane and --resume-start after inspecting it", { intent: intent.key, pane: reconcilePane, unfinishedStep: "agent_start" });
        intent = commit("agent.spawn.pane", { intent: intent.key, pane: reconcilePane, ...placement }, new Date().toISOString()) as SpawnIntent;
      } else if (intent.worktree) {
        if (reconcilePane !== null && reconcilePane !== undefined) throw new HcoordError("invalid_argument", "a new spawn intent cannot reconcile an existing pane");
        // The target's Herdr and hcoord must be usable before any remote effect (PRD B15, B16).
        if (!isLocalMachine(intent.machine)) { requireRemoteHerdr(intent.machine); remoteCall(intent.machine, ["hello", "--hq", os.hostname()]); }
        requireSpawnStorage(SPAWN_EVENT_SLOTS.reserve - 1, "create_worktree");
        intent = commit("agent.spawn.unknown", { intent: intent.key, reason: "worktree creation reserved; outcome pending" }, at) as SpawnIntent;
        let created: { pane: string; workspace: string; cwd: string };
        try { created = createSpawnWorktree(intent); }
        catch (error) {
          if (error instanceof HcoordError && (error.code === "repo_missing" || error.code === "worktree_failed")) commit("agent.spawn.release", { intent: intent.key, reason: error.message, code: error.code }, new Date().toISOString());
          throw error;
        }
        try { intent = commit("agent.spawn.pane", { intent: intent.key, ...created }, new Date().toISOString()) as SpawnIntent; }
        catch (error) { throw new HcoordError("spawn_uncertain", "worktree was created but pane recording failed; repair storage and retry this intent with --reconcile-pane", { intent: intent.key, pane: created.pane, unfinishedStep: "record_pane", code: error instanceof HcoordError ? error.code : "storage_failed" }); }
        createdNow = true;
      } else {
        if (reconcilePane !== null && reconcilePane !== undefined) throw new HcoordError("invalid_argument", "a new spawn intent cannot reconcile an existing pane");
        const placement = parentPlacement(parent);
        requireSpawnStorage(SPAWN_EVENT_SLOTS.reserve - 1, "create_pane");
        intent = commit("agent.spawn.unknown", { intent: intent.key, reason: "tab creation reserved; outcome pending", ...placement }, at) as SpawnIntent;
        let pane: string;
        try { pane = createSpawnPane(intent, placement); }
        catch (error) { throw new HcoordError("spawn_uncertain", "tab creation outcome is unknown; inspect the original intent before reconciling a pane", { intent: intent.key, pane: null, unfinishedStep: "record_pane", code: error instanceof HcoordError ? error.code : "runtime_failed" }); }
        try { intent = commit("agent.spawn.pane", { intent: intent.key, pane }, new Date().toISOString()) as SpawnIntent; }
        catch (error) { throw new HcoordError("spawn_uncertain", "tab was created but pane recording failed; inspect the saved pane and repair storage before reconciling this intent", { intent: intent.key, pane, unfinishedStep: "record_pane", code: error instanceof HcoordError ? error.code : "storage_failed" }); }
        createdNow = true;
      }
    }
    const restoreName = (justStarted: boolean): void => {
      if (nameSpawnedAgent(intent, justStarted)) process.stderr.write(`${JSON.stringify({ event: "hcoord.spawn_name_restored", at: new Date().toISOString(), intent: intent.key, pane: intent.pane, machine: intent.machine })}\n`);
    };
    // A retry repairs a name Herdr dropped from the execution this intent started.
    if (!createdNow) restoreName(false);
    let identity = inspectSpawnedAgent(intent);
    if (identity.state === "absent") {
      if (!createdNow && args["resumeStart"] !== true) throw new HcoordError("spawn_uncertain", "saved pane has no confirmed agent; inspect it and retry this intent with --resume-start", { intent: intent.key, pane: intent.pane, unfinishedStep: "agent_start" });
      if (!createdNow) confirmSpawnPane(intent, intent.placement ?? parentPlacement(ledger.participants[intent.parent]!));
      // A deliberate --resume-start replaces the start recorded for an agent that is gone.
      const resetStart = !createdNow && intent.observedInstance != null;
      const startSlots = (resetStart ? 1 : 0) + SPAWN_EVENT_SLOTS.started + (intent.kind === "codex" ? SPAWN_EVENT_SLOTS.beforeExternalStart : SPAWN_EVENT_SLOTS.beforeRegistration);
      requireEventSlots(startSlots, "agent_start");
      requireSpawnStorage(startSlots, "agent_start");
      if (resetStart) intent = commit("agent.spawn.start_reset", { intent: intent.key }, new Date().toISOString()) as SpawnIntent;
      // An unconfirmed start (a timeout, agent_not_ready, a lost reply) may
      // still have started the agent; it continues exactly as a retry of
      // this intent would. Only Herdr's busy refusal proves nothing started.
      let unconfirmed: HcoordError | null = null;
      try { startSpawnedAgent(intent); }
      catch (error) {
        if (!(error instanceof HcoordError) || error.code !== "spawn_uncertain" || error.detail?.["herdrCode"] === "agent_pane_busy") throw error;
        unconfirmed = error;
      }
      const started = observeStartedAgent(intent);
      if (started !== null) {
        try { intent = commit("agent.spawn.started", { intent: intent.key, instance: started.instance, session: started.session }, new Date().toISOString()) as SpawnIntent; }
        catch (error) {
          if (error instanceof HcoordError && error.code === "identity_conflict") throw error;
          throw new HcoordError("spawn_uncertain", "the agent was started but its execution could not be recorded; repair storage and retry this intent", { intent: intent.key, pane: intent.pane, unfinishedStep: "record_start", code: error instanceof HcoordError ? error.code : "storage_failed" });
        }
      }
      restoreName(true);
      identity = inspectSpawnedAgent(intent);
      if (identity.state === "absent") throw unconfirmed ?? new HcoordError("spawn_uncertain", "agent start returned but its named execution is unavailable", { intent: intent.key, pane: intent.pane });
    }
    if (identity.state === "initializing") {
      if (intent.observedInstance != null && identity.instance !== intent.observedInstance) throw new HcoordError("identity_conflict", "spawn terminal was replaced after its first observation", { intent: intent.key, pane: intent.pane });
      if (identity.blocked) throw blockedSpawnError(intent);
      if (intent.kind !== "codex") throw new HcoordError("spawn_uncertain", "the agent has not reported its session yet; inspect the saved pane and retry this intent", { intent: intent.key, pane: intent.pane, unfinishedStep: "inspect_agent" });
      if (intent.initialization !== "pending") throw new HcoordError("spawn_uncertain", "first-turn submission may already have occurred; inspect the saved pane without resubmitting it", { intent: intent.key, pane: intent.pane, unfinishedStep: "initialize_agent" });
      if (identity.interactiveReady !== true || (identity.runtime !== "idle" && identity.runtime !== "done")) throw new HcoordError("spawn_uncertain", "named agent is not interactive-ready for its first turn", { intent: intent.key, pane: intent.pane, unfinishedStep: "initialize_agent" });
      requireEventSlots(SPAWN_EVENT_SLOTS.beforeFirstTurn, "initialize_agent");
      requireSpawnStorage(SPAWN_EVENT_SLOTS.beforeFirstTurn, "initialize_agent");
      prepareSpawnInitialization(intent, identity.instance);
      intent = commit("agent.spawn.initialization", { intent: intent.key, phase: "reserved", instance: identity.instance }, new Date().toISOString()) as SpawnIntent;
      let submitted: ReturnType<typeof submitSpawnInitialization>;
      try { submitted = submitSpawnInitialization(intent, identity.instance); }
      catch (error) {
        // Inspection and argument validation precede the prompt effect.
        if (error instanceof HcoordError) commit("agent.spawn.initialization", { intent: intent.key, phase: "pending" }, new Date().toISOString());
        throw error;
      }
      if (submitted.outcome === "rejected") {
        commit("agent.spawn.initialization", { intent: intent.key, phase: "pending" }, new Date().toISOString());
        throw new HcoordError("spawn_uncertain", "official first-turn prompt was rejected before input; retry this saved intent after the pane is ready", { intent: intent.key, pane: intent.pane, unfinishedStep: "initialize_agent", code: submitted.code });
      }
      if (submitted.outcome === "unknown") throw new HcoordError("spawn_uncertain", "official first-turn submission is unknown; inspect this pane before retrying without another prompt", { intent: intent.key, pane: intent.pane, unfinishedStep: "initialize_agent", code: submitted.code });
      waitForSpawnInitialization(intent);
      identity = inspectSpawnedAgent(intent);
      if (identity.state !== "ready") throw new HcoordError("spawn_uncertain", "first turn was submitted but execution identity is not yet available; retry this intent without resubmitting it", { intent: intent.key, pane: intent.pane, unfinishedStep: "initialize_agent" });
    }
    if (identity.state !== "ready") throw new HcoordError("spawn_uncertain", "spawned execution identity remains unavailable", { intent: intent.key, pane: intent.pane, unfinishedStep: "inspect_agent" });
    if (intent.initialization === undefined && !createdNow) throw new HcoordError("spawn_uncertain", "legacy spawn intent has no first execution observation; do not bind a possible replacement automatically", { intent: intent.key, pane: intent.pane, unfinishedStep: "inspect_agent" });
    requireEventSlots(SPAWN_EVENT_SLOTS.beforeRegistration, "register_agent");
    requireSpawnStorage(SPAWN_EVENT_SLOTS.beforeRegistration, "register_agent");
    intent = commit("agent.spawn.identity", { intent: intent.key, runtimeSession: identity.session, instance: identity.instance }, new Date().toISOString()) as SpawnIntent;
    try { return commit("agent.spawn.complete", { intent: intent.key, runtimeSession: identity.session, instance: identity.instance, runtime: identity.runtime, project: ledger.participants[intent.parent]?.project }, new Date().toISOString()); }
    catch (error) { throw new HcoordError("spawn_uncertain", "agent exists but registration failed; inspect the saved pane and retry this intent after repairing storage", { intent: intent.key, pane: intent.pane, unfinishedStep: "register_agent", code: error instanceof HcoordError ? error.code : "storage_failed" }); }
  };
  /**
   * `agent register --check`: what registration would decide, saved nowhere
   * (D-19). The execution must be the one in its pane (D-18), registering it
   * must not conflict with a recorded participant, and it must take official
   * delivery, so a caller can refuse before it creates anything (PRD B3).
   */
  const checkRegistration = (args: Record<string, unknown>, at: string): unknown => {
    const machine = String(args["machine"] ?? "");
    const hostScope = isLocalMachine(machine) ? String(args["hostScope"] ?? "default") : "default";
    const session = String(args["session"] ?? ""), pane = typeof args["pane"] === "string" ? args["pane"] : null;
    const binding = validateBinding(machine, session, String(args["instance"] ?? ""), pane, hostScope);
    const decided = execute(structuredClone(ledger), "agent.register", { ...args, hostScope, runtime: binding.runtime, instance: binding.instance }, at).value as { id: string };
    const delivery = officialDeliveryAvailable({ machine, hostScope, session, instance: binding.instance, pane: binding.pane });
    if (!delivery.ready) throw new HcoordError("unsupported_runtime", `official delivery to this execution is unavailable: ${delivery.reason}`);
    return { ready: true, saved: false, participant: own(ledger.participants, decided.id) ? decided.id : null, pane: binding.pane, session, instance: binding.instance, runtime: binding.runtime, delivery: delivery.reason };
  };
  const recordOnly = (letter: LetterRecord): void => {
    const next = structuredClone(ledger);
    recordLetter(next, letter);
    saveLedger(next, home); ledger = next;
  };
  /** One write, whether it arrived as a letter or as a direct API call. */
  const performWrite = (operation: string, args: Record<string, unknown>, at: string, letter?: LetterRecord): unknown => {
    if (operation === "agent.spawn") {
      const value = spawnAgent(args, at) as { participant: import("./model").Participant };
      const lineage = lineageFor(value.participant);
      if (letter) recordOnly(letter);
      return lineage === null ? value : { ...value, lineage };
    }
    if (operation === "agent.register") {
      const machine = String(args["machine"] ?? "");
      // A remote pane is addressed through its saved machine's own session, never this host's socket.
      if (!isLocalMachine(machine)) args = { ...args, hostScope: "default" };
      const binding = validateBinding(machine, String(args["session"] ?? ""), String(args["instance"] ?? ""), typeof args["pane"] === "string" ? args["pane"] : null, String(args["hostScope"] ?? "default"));
      if (!isLocalMachine(machine)) remoteCall(machine, ["hello", "--hq", os.hostname()]);
      args = { ...args, runtime: binding.runtime, instance: binding.instance };
      const participant = commit(operation, args, at, letter) as import("./model").Participant;
      const lineage = lineageFor(participant);
      return lineage === null ? participant : { ...participant, lineage };
    }
    return commit(operation, args, at, letter);
  };
  // Herdr drops every pane token when its server restarts, and after a reboot
  // that restart can land minutes after this daemon starts (2026-09-29: all
  // 39 children failed a startup-only pass and stayed roots). So every route
  // holding a child is read on a timer: one `pane list` per route, and a write
  // only where a live child lost its tokens.
  const routeReadAt = new Map<string, number>();
  const unreadableRoutes = new Set<string>();
  const writeRetryAt = new Map<string, number>();
  const reconcileLineage = (now: number): void => {
    for (const id of writeRetryAt.keys()) if (!own(ledger.participants, id)) writeRetryAt.delete(id);
    const routes = new Map<string, import("./model").Participant[]>();
    for (const child of Object.values(ledger.participants)) {
      if (child.parent === null || child.pane === null) continue;
      const key = `${child.machine}\u0000${child.hostScope}`;
      routes.set(key, [...(routes.get(key) ?? []), child]);
    }
    let scanned = 0, filled = 0, failed = 0, ended = 0;
    const endedChildren: { participant: string; parent: string; pane: string; session: string; observed: string }[] = [];
    for (const [key, children] of routes) {
      const { machine, hostScope } = children[0]!;
      const local = isLocalMachine(machine);
      if (now - (routeReadAt.get(key) ?? -Infinity) < (local ? LOCAL_LINEAGE_MS : REMOTE_LINEAGE_MS)) continue;
      routeReadAt.set(key, now);
      // A remote route keeps its own SSH deadline; this one bounds the local call.
      const panes = readRouteLineage(machine, hostScope, 2000);
      if (panes === null) {
        // One line when the route stops answering, not one per pass while a server is down.
        if (!unreadableRoutes.has(key)) process.stderr.write(`${JSON.stringify({ event: "hcoord.lineage_route_unavailable", at: new Date(now).toISOString(), machine })}\n`);
        unreadableRoutes.add(key);
        continue;
      }
      unreadableRoutes.delete(key);
      for (const child of children) {
        const parent = own(ledger.participants, child.parent!);
        const pane = panes.get(child.pane!);
        if (!parent || parent.pane === null || !pane) continue;
        if (!sameExecution(child, { machine, hostScope, pane: child.pane, session: pane.session, instance: pane.instance })) {
          // The pane now hosts another agent: the relationship ended with the session it was
          // written for, and its tokens would hand the new agent a parent it never had.
          if (!hostsAnotherSession(child, pane) || !carriesLineage(pane) || (writeRetryAt.get(child.id) ?? 0) > now) continue;
          let cleared: ReturnType<typeof clearLineage>;
          try { cleared = clearLineage(child); }
          catch (error) { cleared = { status: "failed", message: error instanceof Error ? error.message : "lineage token clear failed" }; }
          if (cleared.status === "cleared") {
            ended += 1; writeRetryAt.delete(child.id);
            endedChildren.push({ participant: child.id, parent: parent.id, pane: child.pane!, session: child.session, observed: pane.session! });
            continue;
          }
          failed += 1;
          writeRetryAt.set(child.id, now + LINEAGE_WRITE_RETRY_MS);
          process.stderr.write(`${JSON.stringify({ event: "hcoord.lineage_clear_failed", at: new Date(now).toISOString(), participant: child.id, parent: parent.id, machine: child.machine, reason: cleared.message.slice(0, 300) })}\n`);
          continue;
        }
        scanned += 1;
        if (lineageCurrent(parent, child, pane) || (writeRetryAt.get(child.id) ?? 0) > now) continue;
        let result: LineageWrite;
        try { result = writeLineage(parent, child); }
        catch (error) { result = { status: "failed", parentPane: parent.pane, parentMachine: null, message: error instanceof Error ? error.message : "lineage token write failed", nextAction: null }; }
        if (result.status === "written") { filled += 1; writeRetryAt.delete(child.id); continue; }
        failed += 1;
        writeRetryAt.set(child.id, now + LINEAGE_WRITE_RETRY_MS);
        process.stderr.write(`${JSON.stringify({ event: "hcoord.lineage_write_failed", at: new Date(now).toISOString(), participant: child.id, parent: parent.id, machine: child.machine, reason: result.message.slice(0, 300) })}\n`);
      }
    }
    if (filled > 0 || ended > 0) {
      const next = structuredClone(ledger);
      const at = new Date(now).toISOString();
      for (const entry of endedChildren) event(next, at, "lineage.ended", entry.participant, null, { parent: entry.parent, pane: entry.pane, recordedSession: entry.session, observedSession: entry.observed });
      if (filled > 0) event(next, at, "lineage.reconciled", "daemon", null, { scanned, filled, failed });
      saveLedger(next, home); ledger = next;
    }
  };
  // Results of letters the sweep applied before their writer asked; bounded,
  // and only a convenience: the ledger record remains the dedupe authority.
  const letterResults = new Map<string, WireResult>();
  const remember = (letterId: string, result: WireResult): void => {
    letterResults.set(letterId, result);
    if (letterResults.size > 256) letterResults.delete(letterResults.keys().next().value!);
  };
  const refusal = (error: unknown, at: string): WireResult => {
    const reason = error instanceof HcoordError ? error : new HcoordError("internal", "coordinator operation failed; inspect daemon stderr");
    if (!(error instanceof HcoordError)) process.stderr.write(`${JSON.stringify({ event: "hcoord.letter_failed", at, code: "internal" })}\n`);
    return { ok: false, error: { code: reason.code, message: reason.message, ...(reason.detail ? { detail: reason.detail } : {}) }, observedAt: at };
  };
  /**
   * Applies one letter at most once. The effect and the letter record share
   * one ledger save; the caller deletes the letter only after that save.
   * Returns null when the letter must stay in its outbox (unsupported).
   */
  const applyLetter = (found: Found, origin: string, reported: boolean): { result: WireResult; remove: boolean } => {
    const at = new Date().toISOString();
    const known = own(ledger.letters, found.id);
    if (known) {
      const cached = letterResults.get(found.id);
      const result = cached ?? (known.outcome === "applied" ? { ok: true, value: { letter: found.id, outcome: "applied", operation: known.operation }, observedAt: at } : { ok: false, error: { code: known.code ?? "rejected", message: known.message ?? "letter was not applied" }, observedAt: at });
      return { result, remove: known.outcome !== "unsupported" };
    }
    const record = (outcome: LetterRecord["outcome"], code: string | null, message: string | null): LetterRecord => ({ id: found.id, origin, operation: found.letter?.operation ?? "unknown", at, outcome, code, message, reported });
    const unsupported = (code: string, message: string): { result: WireResult; remove: boolean } => {
      recordOnly(record("unsupported", code, message));
      return { result: { ok: false, error: { code, message }, observedAt: at }, remove: false };
    };
    if (found.letter === null) return unsupported("unsupported_letter", found.reason);
    const letter = found.letter;
    // A remote writer names its own host as local; the HQ knows it by its saved machine label.
    if (origin !== "local" && letter.operation === "agent.register") {
      const named = String(letter.args["machine"] ?? "");
      letter.args = { ...letter.args, machine: named === "local" || named === letter.writer.host ? origin : named, hostScope: "default" };
    }
    if (letter.writer.protocol !== REMOTE_PROTOCOL) return unsupported("version_mismatch", `letter from hcoord protocol ${letter.writer.protocol}; this coordinator speaks ${REMOTE_PROTOCOL}`);
    let result: WireResult;
    if (!LETTER_OPERATIONS.has(letter.operation)) {
      result = { ok: false, error: { code: "forbidden", message: `operation ${letter.operation} cannot travel as a letter` }, observedAt: at };
      recordOnly(record("rejected", "forbidden", result.error!.message));
      return { result, remove: true };
    }
    try {
      result = { ok: true, value: performWrite(letter.operation, letter.args, at, record("applied", null, null)), observedAt: at };
    } catch (error) {
      result = refusal(error, at);
      if (!own(ledger.letters, found.id)) recordOnly(record("rejected", result.error!.code, result.error!.message));
    }
    remember(found.id, result);
    return { result, remove: true };
  };
  /** Applies local letters oldest first; stops after `until` when given. */
  const collectLocal = (until: string | null, minimumAgeMs: number): WireResult | null => {
    let requested: WireResult | null = null, applied = 0;
    for (const found of readOutbox(MAX_OUTBOX_LETTERS, home)) {
      if (until === null && (Date.now() - found.createdMs < minimumAgeMs || applied >= SWEEP_LETTERS_PER_TICK)) break;
      if (!own(ledger.letters, found.id)) applied += 1;
      const outcome = applyLetter(found, "local", found.id === until);
      if (outcome.remove) removeLetters([found.id], home);
      if (found.id === until) { requested = outcome.result; break; }
    }
    return requested;
  };
  /** A machine collection refusal a person must fix is kept in the ledger until a later success. */
  const noteMachine = (machine: string, problem: { code: string; message: string } | null): void => {
    const current = ledger.machines[machine]?.problem ?? null;
    if (current === null && problem === null) return;
    if (current !== null && problem !== null && current.code === problem.code) return;
    const at = new Date().toISOString();
    const next = structuredClone(ledger);
    put(next.machines, machine, { problem: problem === null ? null : { ...problem, at } });
    event(next, at, problem === null ? "machine.recovered" : "machine.problem", machine, null, { code: problem?.code ?? null });
    saveLedger(next, home); ledger = next;
  };
  /** An unreachable machine makes its participants unobservable, not failed (PRD B11). */
  const markUnreachable = (machine: string, reason: string): void => {
    const at = new Date().toISOString();
    const next = structuredClone(ledger);
    let changed = false;
    for (const participant of Object.values(next.participants)) {
      if (participant.machine !== machine || participant.connection === "unavailable") continue;
      execute(next, "agent.observe", { id: participant.id, runtime: "unknown", connection: "unavailable", reason }, at);
      changed = true;
    }
    if (changed) { saveLedger(next, home); ledger = next; }
  };
  /** Applies one collected batch oldest first; returns the letters the remote may delete. */
  const applyCollected = (machine: string, raw: Raw): string[] => {
    let value: Record<string, unknown>;
    try { value = remoteOutcome(machine, raw); }
    catch (error) {
      const reason = error instanceof HcoordError ? error : new HcoordError("internal", "collection failed");
      if (reason.code === "machine_unreachable") markUnreachable(machine, reason.message);
      else noteMachine(machine, { code: reason.code, message: reason.message });
      throw reason;
    }
    noteMachine(machine, null);
    const letters = Array.isArray(value["letters"]) ? value["letters"] as RawLetter[] : [];
    const removable: string[] = [];
    for (const letter of letters) {
      if (typeof letter?.id !== "string" || typeof letter.text !== "string" || typeof letter.createdMs !== "number") continue;
      if (applyLetter(parseLetter(letter.id, letter.createdMs, letter.text), machine, false).remove) removable.push(letter.id);
    }
    return removable;
  };
  /** Runs one step on the serialized operation queue; a failed step never blocks the next. */
  const queue = <T>(step: () => T): Promise<T> => {
    const run = processing.then(step);
    processing = run.then(() => undefined, () => undefined);
    return run;
  };
  const collections = new Map<string, { inFlight: boolean; nextAt: number }>();
  const collectFailures = createFailureLog({ failed: "hcoord.collect_failed", recovered: "hcoord.collect_recovered" }, (line) => process.stderr.write(line), COLLECT_FAILURE_LOG_WINDOW_MS);
  const inFlight = new Set<Promise<void>>();
  const aborter = new AbortController();
  /** Starts at most one collection per remote machine; SSH runs outside the operation queue. */
  const pollRemotes = (): void => {
    const machines = new Set([...Object.values(ledger.participants).map((p) => p.machine), ...Object.values(ledger.spawnIntents).filter((i) => i.status !== "complete").map((i) => i.machine)].filter((m) => !isLocalMachine(m)));
    for (const machine of machines) {
      const state = collections.get(machine) ?? { inFlight: false, nextAt: 0 };
      collections.set(machine, state);
      if (state.inFlight || closing || Date.now() < state.nextAt) continue;
      state.inFlight = true;
      const job = (async () => {
        let retryAfter = COLLECT_BACKOFF_MS;
        try {
          let target: string;
          try { target = savedMachine(machine).target; }
          catch (error) {
            const reason = error instanceof HcoordError ? error : new HcoordError("internal", "saved machine lookup failed");
            await queue(() => noteMachine(machine, { code: reason.code, message: reason.message }));
            throw reason;
          }
          const raw = await remoteCallAsync(target, ["take", "--limit", String(COLLECT_LETTERS)], aborter.signal);
          const removable = await queue(() => closing ? [] : applyCollected(machine, raw));
          retryAfter = COLLECT_INTERVAL_MS;
          // A failed deletion is harmless: the next take returns recorded letters, which are only deleted again.
          if (removable.length) await remoteCallAsync(target, ["drop", ...removable], aborter.signal);
          collectFailures.recovered(machine, { machine });
        } catch (error) {
          // The SSH or remote diagnostic belongs in the log: one live run saw a single transient auth refusal that the code alone could not explain.
          // The same cause on the same machine logs once per window, with the count it folded.
          const code = error instanceof HcoordError ? error.code : "internal";
          collectFailures.failed(machine, code, { machine, code, detail: error instanceof Error ? error.message.slice(0, 300) : null });
        } finally { state.inFlight = false; state.nextAt = Date.now() + retryAfter; }
      })();
      inFlight.add(job);
      void job.finally(() => inFlight.delete(job));
    }
  };
  const processOutbox = (): void => {
    let examined = 0;
    for (const item of Object.values(ledger.requests)) {
      if (item.status === "canceled") continue;
      for (const delivery of item.deliveries) {
        if (examined >= 1) return;
        if (delivery.status !== "pending" && delivery.status !== "deferred") continue;
        const watch = watchForRequest(ledger, item);
        // Stop preserves the open cycle for explicit restart or reassignment.
        // Do not submit its retained wake to the former observer while paused.
        if (watch?.status === "stopped" && delivery.recipient !== "human") continue;
        if (item.status === "answered" && (delivery.phase ?? "request") === "request") {
          const next = structuredClone(ledger);
          const current = next.requests[item.id]!.deliveries.find((entry) => entry.id === delivery.id)!;
          current.status = "superseded";
          current.reason = "original request was answered before submission";
          saveLedger(next, home); ledger = next;
          continue;
        }
        if (delivery.attemptedAt !== null && Date.now() - Date.parse(delivery.attemptedAt) < 5000) continue;
        examined += 1;
        if (delivery.recipient === "human") {
          const next = structuredClone(ledger);
          const current = next.requests[item.id]!.deliveries.find((entry) => entry.id === delivery.id)!;
          current.attemptedAt = new Date().toISOString();
          current.status = "unknown";
          current.reason = "notification reserved; result not confirmed";
          saveLedger(next, home); ledger = next;
          const result = notifyHuman(item.id);
          const finished = structuredClone(ledger);
          const recorded = finished.requests[item.id]!.deliveries.find((entry) => entry.id === delivery.id)!;
          recorded.status = result.ok ? "accepted" : "failed";
          recorded.runtimeCode = result.code;
          recorded.reason = result.ok ? "Herdr notification accepted; human reading is unconfirmed" : "notification failed; open hcoord inbox to act";
          if (result.ok) recorded.acceptedAt = new Date().toISOString();
          event(finished, new Date().toISOString(), result.ok ? "notification.accepted" : "notification.failed", item.id, item.intent, { deliveryId: delivery.id, code: result.code });
          saveLedger(finished, home); ledger = finished;
          continue;
        }
        const recipient = own(ledger.participants, delivery.recipient);
        if (!recipient) {
          const next = structuredClone(ledger);
          const current = next.requests[item.id]!.deliveries.find((entry) => entry.id === delivery.id)!;
          current.status = "failed"; current.reason = "recipient is no longer registered; inspect the request in the CLI inbox"; current.attemptedAt = new Date().toISOString();
          event(next, current.attemptedAt, "delivery.failed", item.id, item.intent, { deliveryId: delivery.id, code: "recipient_missing" });
          saveLedger(next, home); ledger = next;
          continue;
        }
        const at = new Date().toISOString();
        const next = structuredClone(ledger);
        const current = next.requests[item.id]!.deliveries.find((entry) => entry.id === delivery.id)!;
        const target = own(next.participants, delivery.recipient)!;
        const inspection = inspectDelivery(recipient);
        current.attemptedAt = at;
        target.runtime = inspection.runtime;
        target.connection = inspection.connection;
        target.observedAt = at;
        if (!inspection.ready) {
          current.status = "deferred";
          current.reason = inspection.reason;
          saveLedger(next, home); ledger = next;
          continue;
        }
        // Persist uncertainty before the external effect. A crash or timeout never causes a blind resend.
        current.status = "unknown";
        current.reason = "official prompt submission reserved; outcome not yet confirmed";
        saveLedger(next, home); ledger = next;
        // The disk reservation is required for retry safety. Reobserve directly
        // afterward so known identity, lifecycle and readiness changes defer.
        // Official 0.9.1 still cannot make this check atomic with agent.prompt.
        const finalInspection = inspectDelivery(recipient);
        if (!finalInspection.ready) {
          const deferred = structuredClone(ledger);
          const record = deferred.requests[item.id]!.deliveries.find((entry) => entry.id === delivery.id)!;
          record.status = "deferred";
          record.reason = finalInspection.reason;
          saveLedger(deferred, home); ledger = deferred;
          continue;
        }
        const outcome = submitOfficial(item, delivery, recipient, watch, own(ledger.participants, item.from));
        const finished = structuredClone(ledger);
        const recorded = finished.requests[item.id]!.deliveries.find((entry) => entry.id === delivery.id)!;
        recorded.status = outcome.status;
        recorded.reason = outcome.reason;
        recorded.runtimeCode = outcome.code;
        if (outcome.status === "accepted") recorded.acceptedAt = new Date().toISOString();
        event(finished, new Date().toISOString(), `delivery.${outcome.status}`, item.id, item.intent, { deliveryId: delivery.id, code: outcome.code });
        saveLedger(finished, home); ledger = finished;
      }
    }
  };
  const server = net.createServer((socket) => {
    connections += 1;
    if (connections > MAX_CONNECTIONS) { socket.end(`${JSON.stringify({ ok: false, error: { code: "capacity", message: `connection limit ${MAX_CONNECTIONS} reached` }, observedAt: new Date().toISOString() })}\n`); connections -= 1; return; }
    socket.setTimeout(80_000, () => socket.destroy());
    let input = "";
    let received = false;
    socket.on("close", () => { connections -= 1; });
    socket.on("data", (chunk: Buffer) => {
      if (received) return;
      input += chunk.toString("utf8");
      if (Buffer.byteLength(input) > MAX_MESSAGE_BYTES) { received = true; socket.end(`${JSON.stringify({ ok: false, error: { code: "capacity", message: `request exceeds ${MAX_MESSAGE_BYTES} bytes; shorten context or native arguments before retrying` }, observedAt: new Date().toISOString() })}\n`); return; }
      const newline = input.indexOf("\n");
      if (newline < 0) return;
      if (input.slice(newline + 1).trim() !== "") { socket.end(`${JSON.stringify({ ok: false, error: { code: "protocol", message: "one request per connection is allowed" }, observedAt: new Date().toISOString() })}\n`); return; }
      received = true;
      socket.pause();
      if (queuedOperations >= MAX_QUEUE) { socket.end(`${JSON.stringify({ ok: false, error: { code: "capacity", message: `queued operation limit ${MAX_QUEUE} reached` }, observedAt: new Date().toISOString() })}\n`); return; }
      queuedOperations += 1;
      const line = input.slice(0, newline);
      input = "";
      processing = processing.then(async () => {
        const at = new Date().toISOString();
        let result: WireResult;
        try {
          if (closing) throw new HcoordError("manual_stop", "daemon is stopping; restart it before submitting more work");
          const decoded = JSON.parse(line) as WireRequest;
          if (decoded.version !== API_VERSION) throw new HcoordError("version_mismatch", `API version ${decoded.version} is unsupported; expected ${API_VERSION}`);
          if (typeof decoded.operation !== "string" || !decoded.args || typeof decoded.args !== "object" || Array.isArray(decoded.args)) throw new HcoordError("protocol", "operation and object args are required");
          if (decoded.operation === "daemon.stop") {
            fs.writeFileSync(stopMarkerPath(home), `${at}\n`, { mode: 0o600 });
            result = { ok: true, value: { stopped: true }, observedAt: at };
            closing = true;
          } else {
            if (decoded.operation.startsWith("agent.spawn.") || decoded.operation === "tick" || decoded.operation === "agent.observe") throw new HcoordError("forbidden", "operation is daemon-internal");
            if (decoded.operation === "outbox.collect") {
              const letterId = decoded.args["letter"];
              if (typeof letterId !== "string" || letterId === "") throw new HcoordError("invalid_argument", "letter is required");
              const collected = collectLocal(letterId, 0);
              const known = own(ledger.letters, letterId);
              const resolved = collected ?? letterResults.get(letterId) ?? (known ? applyLetter({ id: letterId, createdMs: 0, letter: null, reason: "" }, "local", true).result : null);
              if (resolved === null) throw new HcoordError("not_found", "letter is neither in the outbox nor recorded; inspect hcoord inbox before resending");
              // The sweep may have applied it before this call arrived; its writer sees the outcome now, so it leaves the inbox.
              const record = own(ledger.letters, letterId);
              if (record && !record.reported) {
                const next = structuredClone(ledger);
                next.letters[letterId]!.reported = true;
                saveLedger(next, home); ledger = next;
              }
              socket.end(`${JSON.stringify({ ...resolved, delivery: "delivered" })}\n`);
              return;
            }
            let value = decoded.operation === "agent.check" ? checkRegistration(decoded.args, at) : LETTER_OPERATIONS.has(decoded.operation) ? performWrite(decoded.operation, decoded.args, at) : commit(decoded.operation, decoded.args, at);
            if (decoded.operation === "status") {
              value = { ...(value as object), ...daemonIdentity, deliverySafety: OFFICIAL_PROMPT_BOUNDARY, usage: { uncollectedLocalLetters: outboxCount(home), ledgerBytes: fs.existsSync(ledgerPath(home)) ? fs.statSync(ledgerPath(home)).size : 0,
                connections, queuedOperations, queuedDeliveries: Object.values(ledger.requests).reduce((sum, item) => sum + item.deliveries.filter((delivery) => delivery.status === "pending" || delivery.status === "deferred").length, 0),
                uncertainSpawns: Object.values(ledger.spawnIntents).filter((intent) => intent.status === "unknown").length } };
            }
            if (decoded.operation === "agent.list") {
              const registered = value as Array<Record<string, unknown>>;
              const scopes = [...new Set(["local\u0000default", ...Object.values(ledger.participants).map((entry) => `${isLocalMachine(entry.machine) ? "local" : entry.machine}\u0000${isLocalMachine(entry.machine) ? entry.hostScope : "default"}`)])];
              const discovered = scopes.slice(0, 4).map((scope) => { const [machine, hostScope] = scope.split("\u0000"); return discoverAgents(Object.values(ledger.participants), typeof decoded.args["project"] === "string" ? decoded.args["project"] : null, machine, hostScope); });
              value = { items: [...registered.map((entry) => ({ registered: true, ...entry })), ...discovered.flatMap((entry) => entry.items)], partialFailures: [...discovered.flatMap((entry) => entry.partialFailures), ...(scopes.length > 4 ? [`discovery skipped ${scopes.length - 4} socket scopes; registered participants remain visible`] : [])], observedAt: at };
            }
            result = { ok: true, value, observedAt: at };
          }
        } catch (error) {
          const reason = error instanceof HcoordError ? error : error instanceof SyntaxError ? new HcoordError("protocol", "request JSON is invalid") : new HcoordError("internal", "coordinator operation failed; inspect daemon stderr");
          if (!(error instanceof HcoordError)) process.stderr.write(`${JSON.stringify({ event: "hcoord.operation_failed", at, code: error instanceof SyntaxError ? "invalid_json" : "internal" })}\n`);
          result = { ok: false, error: { code: reason.code, message: reason.message, ...(reason.detail ? { detail: reason.detail } : {}) }, observedAt: at };
        }
        socket.end(`${JSON.stringify(result)}\n`);
        if (closing) server.close();
      }).catch(() => {
        process.stderr.write(`${JSON.stringify({ event: "hcoord.operation_failed", at: new Date().toISOString(), code: "internal" })}\n`);
        if (!socket.destroyed) socket.end(`${JSON.stringify({ ok: false, error: { code: "internal", message: "coordinator operation failed; inspect daemon log" }, observedAt: new Date().toISOString() })}\n`);
      }).finally(() => { queuedOperations -= 1; });
    });
  });
  // Handlers exist before the start record: a stop that landed between that
  // record and the socket being ready used the default action, left no clean
  // mark, and a clean stop read as a crash (hcoord-health e2e flake, 2026-09-25).
  const closed = new Promise<void>((resolve) => server.once("close", () => resolve()));
  const onSignal = (): void => { if (!closing) { closing = true; if (server.listening) server.close(); } };
  process.once("SIGTERM", onSignal);
  process.once("SIGINT", onSignal);
  recordStart(process.pid, new Date().toISOString(), home);
  try {
    await new Promise<void>((resolve, reject) => { server.once("error", reject); server.listen(socketFile, () => { server.off("error", reject); resolve(); }); });
    socketOwned = true;
    fs.chmodSync(socketFile, 0o600);
    recordReady(process.pid, new Date().toISOString(), home);
    reconcileAlert(home, Date.now(), true, notifyText);
    let lineagePending = false;
    const queueLineage = (): void => {
      if (lineagePending || closing) return;
      lineagePending = true;
      processing = processing.then(() => { if (!closing) reconcileLineage(Date.now()); }).catch((error) => {
        process.stderr.write(`${JSON.stringify({ event: "hcoord.lineage_reconcile_failed", at: new Date().toISOString(), code: error instanceof HcoordError ? error.code : "internal" })}\n`);
      }).finally(() => { lineagePending = false; });
    };
    queueLineage();
    if (closing) server.close();
    const timer = setInterval(() => {
      queueLineage();
      if (tickPending || closing) return;
      tickPending = true;
      processing = processing.then(() => {
        if (closing) return;
        const at = new Date().toISOString();
        const next: Ledger = structuredClone(ledger);
        const due = Object.values(next.watches).filter((watch) => watch.status === "active" && Date.parse(watch.dueAt) <= Date.parse(at)).sort((a, b) => Date.parse(a.dueAt) - Date.parse(b.dueAt)).slice(0, 4);
        const observedTargets: string[] = [], watchReadings: Record<string, "gone" | "present"> = {};
        for (const watch of due) {
          const participant = own(next.participants, watch.target);
          if (!participant) throw new HcoordError("corrupt_ledger", "active watch target is missing");
          const observed = inspectParticipant(participant);
          execute(next, "agent.observe", { id: participant.id, runtime: observed.runtime, connection: observed.connection, reason: observed.reason }, at);
          observedTargets.push(participant.id);
          // Only a target Herdr says is gone, with an observer that is gone too, counts toward ending the watch;
          // a Herdr or remote that could not be asked says nothing about either side. A reachable side clears the count.
          if (observed.connection === "connected") { watchReadings[participant.id] = "present"; continue; }
          if (!observed.gone) continue;
          const observer = watch.observer === null ? undefined : own(next.participants, watch.observer);
          const observerReading = observer === undefined ? null : inspectParticipant(observer);
          if (observerReading === null || observerReading.gone) watchReadings[participant.id] = "gone";
          else if (observerReading.connection === "connected") watchReadings[participant.id] = "present";
        }
        const oldestUnwatched = Object.values(next.participants).filter((person) => person.runtime !== "done" && next.watches[person.id]?.status !== "active" && Date.parse(at) - Date.parse(person.observedAt) >= 300_000).sort((a, b) => Date.parse(a.observedAt) - Date.parse(b.observedAt))[0];
        if (oldestUnwatched) {
          const observed = inspectParticipant(oldestUnwatched);
          execute(next, "agent.observe", { id: oldestUnwatched.id, runtime: observed.runtime, connection: observed.connection, reason: observed.reason }, at);
        }
        const runRetention = Date.parse(at) - lastRetentionAt >= 3_600_000;
        const outcome = execute(next, "tick", { observedTargets, watchReadings, runRetention }, at);
        if (outcome.changed || observedTargets.length > 0 || oldestUnwatched) { saveLedger(next, home); ledger = next; }
        if (runRetention) lastRetentionAt = Date.parse(at);
        collectLocal(null, LOCAL_SWEEP_GRACE_MS);
        processOutbox();
      }).catch((error) => { process.stderr.write(`${JSON.stringify({ event: "hcoord.tick_failed", at: new Date().toISOString(), code: error instanceof HcoordError ? error.code : "internal" })}\n`); }).finally(() => { tickPending = false; });
      pollRemotes();
    }, 1000);
    await closed;
    clearInterval(timer);
    aborter.abort();
    await Promise.allSettled([...inFlight]);
    await processing;
    recordClean(process.pid, new Date().toISOString(), home);
  } finally {
    process.off("SIGTERM", onSignal);
    process.off("SIGINT", onSignal);
    try { if (socketOwned && fs.existsSync(socketFile)) fs.unlinkSync(socketFile); } catch { /* report only through original error */ } }
  } finally { try { if (lockOwned && fs.existsSync(lockFile)) fs.unlinkSync(lockFile); } catch { /* report only through original error */ } }
  return "stopped";
}
