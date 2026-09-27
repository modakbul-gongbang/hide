import path from "node:path";
import { DEFAULTS, event, HcoordError, id, MAX_AGENTS, MAX_LETTER_RECORDS, MAX_BODY_BYTES, MAX_BRIEF_BYTES, MAX_EVENTS, MAX_MESSAGE_BYTES, MAX_QUEUE, MAX_REQUESTS, MAX_SPAWN_INTENTS, MAX_WATCH_HISTORY, SPAWN_EVENT_SLOTS, own, put, sameExecution, validateSpawnSpec, type Delivery, type Ledger, type LetterRecord, type Participant, type Request, type SpawnIntent, type Watch } from "./model";

type Args = Record<string, unknown>;
const isHere = (machine: string): boolean => machine === "local" || machine === require("node:os").hostname();
export interface Outcome { value: unknown; changed: boolean }
const required = (args: Args, key: string): string => {
  const value = args[key];
  if (typeof value !== "string" || value.trim() === "") throw new HcoordError("invalid_argument", `${key} is required`);
  return value;
};
const optional = (args: Args, key: string): string | null => {
  const value = args[key];
  if (value === undefined || value === null) return null;
  if (typeof value !== "string") throw new HcoordError("invalid_argument", `${key} must be text`);
  return value;
};
const body = (args: Args, key = "body"): string => {
  const value = required(args, key);
  if (Buffer.byteLength(value) > MAX_BODY_BYTES) throw new HcoordError("capacity", `${key} exceeds ${MAX_BODY_BYTES} bytes`);
  return value;
};
const brief = (args: Args): string | null => {
  const value = optional(args, "brief");
  if (value === null || value.trim() === "") return null;
  if (Buffer.byteLength(value) > MAX_BRIEF_BYTES) throw new HcoordError("capacity", `brief exceeds ${MAX_BRIEF_BYTES} bytes`);
  return value;
};
const person = (state: Ledger, agentId: string): Participant => {
  const agent = own(state.participants, agentId);
  if (!agent) throw new HcoordError("not_found", `participant ${agentId} is not registered`);
  return agent;
};
const request = (state: Ledger, requestId: string): Request => {
  const found = own(state.requests, requestId);
  if (!found) throw new HcoordError("not_found", `request ${requestId} does not exist`);
  return found;
};
const authority = (actor: string, allowed: Array<string | null>): void => {
  if (actor !== "human" && !allowed.includes(actor)) throw new HcoordError("forbidden", "current observer, sender, or human authority is required");
};
const integerMs = (args: Args, key: string): number => {
  const value = args[key];
  const number = typeof value === "number" ? value : Number(value);
  if (!Number.isSafeInteger(number) || number < 1000 || number > 365 * 24 * 60 * 60_000) throw new HcoordError("invalid_argument", `${key} must be 1 second to 365 days`);
  return number;
};
const timed = (at: string, ms: number): string => new Date(Date.parse(at) + ms).toISOString();
const pendingRelay = (item: Request): boolean => item.status === "answered" && item.intermediary !== null && item.relayBody === null && !item.watchCheckedAt;
const legacyWatchIntent = (watch: Watch, item: Request): boolean => {
  const parts = item.intent.split(":");
  return item.from === watch.target && parts.length === 4 && parts[0] === "watch" && parts[1] === watch.target && /^\d+$/.test(parts[2] ?? "") && parts[3] === watch.cycle;
};
const watchRequest = (state: Ledger, watch: Watch): Request | undefined => {
  if (watch.cycle === null) return undefined;
  if (watch.requestId !== undefined && watch.requestId !== null) return own(state.requests, watch.requestId);
  // Ledgers written before request IDs were saved still have one generated request per cycle.
  return Object.values(state.requests).find((item) => legacyWatchIntent(watch, item));
};
const uncheckedWatchRequest = (state: Ledger, item: Request): boolean => {
  const watch = own(state.watches, item.from);
  if (watch?.cycle === null || watch === undefined) return false;
  if (watch.requestId !== undefined && watch.requestId !== null) return watch.requestId === item.id;
  return legacyWatchIntent(watch, item) && watchRequest(state, watch)?.id === item.id;
};
export const watchForRequest = (state: Ledger, item: Request): Watch | null => {
  const watch = own(state.watches, item.from);
  return watch && uncheckedWatchRequest(state, item) ? watch : null;
};
const terminalRequest = (state: Ledger, item: Request): boolean => !(uncheckedWatchRequest(state, item) && item.status !== "canceled") && (item.status === "canceled" || (item.status === "answered" && !pendingRelay(item) && item.deliveries.every((delivery) => {
  if (item.watchCheckedAt && delivery.actionClosedAt) return true;
  const phase = delivery.phase ?? "request";
  return phase === "request" ? ["accepted", "acknowledged", "failed", "superseded"].includes(delivery.status)
    : phase === "delivery_problem" || phase === "relay_problem" || phase === "watch_check" ? ["accepted", "acknowledged", "superseded"].includes(delivery.status)
    : delivery.status === "acknowledged" || delivery.status === "superseded";
}))
  || (!item.requiresReply && item.deliveries.length > 0 && item.deliveries.every((delivery) => delivery.status === "accepted" || delivery.status === "acknowledged")));
const queueDelivery = (item: Request, recipient: string, at: string, phase: Delivery["phase"] = "request"): Delivery => {
  const delivery: Delivery = { id: id("d"), requestId: item.id, recipient, phase, status: "pending", reason: null, reservedAt: at, attemptedAt: null, acceptedAt: null, acknowledgedAt: null, runtimeCode: null };
  item.deliveries.push(delivery);
  return delivery;
};
const retireUnsent = (item: Request, reason: string, phase?: Delivery["phase"]): void => {
  for (const delivery of item.deliveries) if ((delivery.status === "pending" || delivery.status === "deferred") && (phase === undefined || (delivery.phase ?? "request") === phase)) {
    delivery.status = "superseded";
    delivery.reason = reason;
  }
};
/**
 * Questions the target asked the old observer, and answers a person gave
 * through the old observer that it has not relayed yet, move to the new
 * observer with the watch, so none waits on a session that left (PRD B15).
 */
function moveLetters(state: Ledger, target: string, from: string, to: string, at: string): number {
  let moved = 0;
  for (const item of Object.values(state.requests)) {
    if (item.from !== target || uncheckedWatchRequest(state, item)) continue;
    if (item.status === "open" && item.to === from) {
      retireUnsent(item, "watch assigned to another observer before submission");
      item.to = to;
      queueDelivery(item, to, at);
      moved += 1;
    } else if (pendingRelay(item) && item.intermediary === from) {
      retireUnsent(item, "watch assigned to another observer before the answer was relayed", "answer");
      retireUnsent(item, "watch assigned to another observer before the answer was relayed", "relay_problem");
      item.intermediary = to;
      queueDelivery(item, to, at, "answer");
      moved += 1;
    }
  }
  return moved;
}

/**
 * Records a letter in the same ledger save as its effect, so a crash before the
 * outbox deletion re-reads a letter the ledger already knows (PRD B10).
 */
export function recordLetter(state: Ledger, record: LetterRecord): void {
  if (own(state.letters, record.id)) throw new HcoordError("conflict", "letter is already recorded");
  if (Object.keys(state.letters).length >= MAX_LETTER_RECORDS) throw new HcoordError("capacity", `letter history reached ${MAX_LETTER_RECORDS}; the letter stays in its outbox until retention frees room`);
  put(state.letters, record.id, record);
  if (record.outcome !== "applied") event(state, record.at, "letter.rejected", record.id, null, { origin: record.origin, operation: record.operation, code: record.code });
  else state.updatedAt = record.at;
}

/** Work that still needs this HQ: moving the HQ away is refused while any remains (PRD D-15, B17). */
export function openWork(state: Ledger): { requests: Array<{ id: string; from: string; to: string; status: string }>; watches: Array<{ target: string; observer: string | null }> } {
  return {
    requests: Object.values(state.requests).filter((item) => !terminalRequest(state, item)).map((item) => ({ id: item.id, from: item.from, to: item.to, status: item.status })),
    watches: Object.values(state.watches).filter((watch) => watch.status === "active").map((watch) => ({ target: watch.target, observer: watch.observer })),
  };
}

/** Whether a spawn's execution is still the one first observed in its saved pane (D-18). */
function spawnObservationHolds(record: SpawnIntent, session: string, instance: string): boolean {
  const recorded = { machine: record.machine, hostScope: record.hostScope, pane: record.pane, session: record.observedSession ?? null, instance: record.observedInstance ?? null };
  return (recorded.session === null && recorded.instance === null) || sameExecution(recorded, { ...recorded, session, instance });
}

export function execute(state: Ledger, operation: string, args: Args, at: string): Outcome {
  if (operation === "status") return { changed: false, value: {
    schema: state.schema, at, lastUpdatedAt: state.updatedAt, eventCursor: state.seq, counts: {
      agents: Object.keys(state.participants).length, watches: Object.values(state.watches).filter((w) => w.status === "active").length,
      requests: Object.values(state.requests).filter((r) => !terminalRequest(state, r)).length, events: state.events.length,
    }, caps: { agents: MAX_AGENTS, requests: MAX_REQUESTS, spawnIntents: MAX_SPAWN_INTENTS, watchHistory: MAX_WATCH_HISTORY, events: 20000, ledgerBytes: 64 * 1024 * 1024, messageBytes: MAX_MESSAGE_BYTES, connections: 64, queuedOperations: MAX_QUEUE }, config: state.config,
  } };
  if (operation === "config.set") {
    const key = required(args, "key") as keyof typeof DEFAULTS;
    if (!Object.hasOwn(DEFAULTS, key)) throw new HcoordError("invalid_argument", `unknown policy ${key}`);
    const value = integerMs(args, "value");
    state.config[key] = value;
    event(state, at, "config.changed", key, null, { value });
    return { changed: true, value: { ...state.config } };
  }
  if (operation === "agent.register") {
    const machine = required(args, "machine"), hostScope = required(args, "hostScope"), session = required(args, "session"), instance = required(args, "instance"), name = required(args, "name");
    const binding = { machine, hostScope, pane: optional(args, "pane"), session, instance };
    // Participants registered under the older terminal rule can share one
    // execution; the one already carrying the requested name is kept, else the latest.
    const matches = Object.values(state.participants).filter((p) => sameExecution(p, binding));
    const existing = matches.find((p) => p.name === name) ?? matches.at(-1);
    const namesakes = Object.values(state.participants).filter((p) => p.machine === machine && p.hostScope === hostScope && p.session === session && p.name === name && !sameExecution(p, binding));
    if (namesakes.length) throw new HcoordError("identity_conflict", "name belongs to the same session in another pane", { candidates: namesakes });
    if (existing) {
      const project = optional(args, "project");
      if (existing.parent !== optional(args, "parent") || (project !== null && existing.project !== null && existing.project !== path.resolve(project))) throw new HcoordError("identity_conflict", "this execution is registered with a different parent or project; inspect the exact binding before changing it", { candidates: matches });
      // Terminal and name are recorded, never matched (D-18): a registration keeps them current.
      const refreshed = existing.instance !== instance || existing.name !== name;
      if (refreshed) { event(state, at, "agent.binding_refreshed", existing.id, null, { instance, name, previousInstance: existing.instance, previousName: existing.name }); existing.instance = instance; existing.name = name; }
      if (project !== null && existing.project === null) { existing.project = path.resolve(project); event(state, at, "agent.project_attached", existing.id); return { changed: true, value: existing }; }
      return { changed: refreshed, value: existing };
    }
    if (Object.keys(state.participants).length >= MAX_AGENTS) throw new HcoordError("capacity", `participant limit ${MAX_AGENTS} reached`);
    const parent = optional(args, "parent");
    if (parent !== null) person(state, parent);
    const participant: Participant = { id: id("a"), machine, hostScope, session, instance, name, project: optional(args, "project") === null ? null : path.resolve(optional(args, "project")!), parent,
      pane: optional(args, "pane"), runtime: (args["runtime"] as Participant["runtime"]) ?? "unknown", connection: "connected", observedAt: at };
    put(state.participants, participant.id, participant);
    event(state, at, "agent.registered", participant.id, null, { machine, session, instance, parent });
    return { changed: true, value: participant };
  }
  if (operation === "agent.spawn.reserve") {
    const key = required(args, "intent"), parent = person(state, required(args, "parent"));
    // Without --machine the child is placed beside its parent, as before (PRD B4).
    const machine = optional(args, "machine") ?? parent.machine, session = required(args, "session"), name = required(args, "name"), kind = required(args, "kind");
    validateSpawnSpec(name, kind);
    const nativeArgs = args["nativeArgs"];
    if (!Array.isArray(nativeArgs) || nativeArgs.some((value) => typeof value !== "string")) throw new HcoordError("invalid_argument", "nativeArgs must be a string array");
    if (parent.session !== session) throw new HcoordError("identity_conflict", "parent is not bound to the selected session");
    const repo = optional(args, "repo"), branch = optional(args, "branch"), worktreePath = optional(args, "path");
    if ((repo === null) !== (branch === null)) throw new HcoordError("invalid_argument", "--repo and --branch are given together");
    if (worktreePath !== null && repo === null) throw new HcoordError("invalid_argument", "--path applies only to a worktree spawn with --repo and --branch");
    const sameMachine = machine === parent.machine || (isHere(machine) && isHere(parent.machine));
    if (!sameMachine && repo === null) throw new HcoordError("invalid_argument", `a child on another machine needs --repo <source repository on ${machine}> and --branch <new branch>`);
    for (const [flag, value] of [["repo", repo], ["path", worktreePath]] as const) if (value !== null && !(value.startsWith("/") || value === "~" || value.startsWith("~/"))) throw new HcoordError("invalid_argument", `--${flag} must be absolute or start with ~/ on ${machine}`);
    const worktree = repo === null ? null : { repo, branch: branch!, path: worktreePath };
    const prior = own(state.spawnIntents, key);
    if (prior) {
      if (prior.parent !== parent.id || prior.machine !== machine || prior.session !== session || prior.name !== name || prior.kind !== kind || prior.noWatch !== (args["noWatch"] === true) || JSON.stringify(prior.nativeArgs) !== JSON.stringify(nativeArgs) || JSON.stringify(prior.worktree ?? null) !== JSON.stringify(worktree)) throw new HcoordError("intent_conflict", "spawn intent key already belongs to another operation", { participant: prior.participant, pane: prior.pane });
      // Older completed records retained a tab-creation warning. A same-key
      // read repairs that obsolete progress without repeating any external act.
      if (prior.status === "complete" && (prior.reason !== null || prior.initialization !== "complete")) {
        prior.reason = null;
        prior.initialization = "complete";
        return { changed: true, value: prior };
      }
      return { changed: false, value: prior };
    }
    if (Object.keys(state.participants).length >= MAX_AGENTS) throw new HcoordError("capacity", `participant limit ${MAX_AGENTS} reached`);
    if (Object.keys(state.spawnIntents).length >= MAX_SPAWN_INTENTS) throw new HcoordError("capacity", `spawn intent limit ${MAX_SPAWN_INTENTS} reached; uncertain outcomes remain inspectable`);
    if (MAX_EVENTS - state.events.length < SPAWN_EVENT_SLOTS.reserve) throw new HcoordError("capacity", "event history has insufficient room for a complete spawn; resolve retention before creating a pane");
    const record = { key, parent: parent.id, machine, hostScope: sameMachine ? parent.hostScope : "default", session, name, kind, nativeArgs, noWatch: args["noWatch"] === true, status: "reserved" as const, pane: null, participant: null, reason: null, at, initialization: "pending" as const, observedInstance: null, observedSession: null, worktree };
    put(state.spawnIntents, key, record);
    event(state, at, "agent.spawn_reserved", parent.id, key);
    return { changed: true, value: record };
  }
  if (operation === "agent.spawn.pane") {
    const record = own(state.spawnIntents, required(args, "intent"));
    if (!record) throw new HcoordError("not_found", "spawn intent does not exist");
    const pane = required(args, "pane");
    if (record.pane !== null && record.pane !== pane) throw new HcoordError("conflict", "spawn intent already owns another pane");
    record.pane = pane;
    record.status = "unknown";
    const workspace = optional(args, "workspace"), cwd = optional(args, "cwd");
    if (workspace !== null && cwd !== null) record.placement = { workspace, cwd };
    event(state, at, "agent.spawn_pane", pane, record.key);
    return { changed: true, value: record };
  }
  if (operation === "agent.spawn.unknown") {
    const record = own(state.spawnIntents, required(args, "intent"));
    if (!record || record.status === "complete") throw new HcoordError("conflict", "spawn intent is not reservable");
    record.status = "unknown"; record.reason = required(args, "reason");
    if (!record.worktree) record.placement = { workspace: required(args, "workspace"), cwd: required(args, "cwd") };
    event(state, at, "agent.spawn_uncertain", record.parent, record.key, { pane: record.pane });
    return { changed: true, value: record };
  }
  if (operation === "agent.spawn.release") {
    // Herdr definitively refused the worktree, so nothing exists; the same intent may try again later.
    const record = own(state.spawnIntents, required(args, "intent"));
    if (!record || record.status !== "unknown" || record.pane !== null) throw new HcoordError("invalid_state", "only a spawn without a pane can be released");
    record.status = "reserved"; record.reason = required(args, "reason");
    event(state, at, "agent.spawn_refused", record.parent, record.key, { code: required(args, "code") });
    return { changed: true, value: record };
  }
  if (operation === "agent.spawn.initialization") {
    const record = own(state.spawnIntents, required(args, "intent"));
    if (!record || record.pane === null || record.status === "complete") throw new HcoordError("invalid_state", "spawn initialization has no active saved pane");
    const phase = required(args, "phase");
    if (phase === "reserved" && record.initialization === "pending") {
      const instance = required(args, "instance");
      if (record.observedInstance !== null && record.observedInstance !== instance) throw new HcoordError("identity_conflict", "spawn terminal differs from its first observation");
      record.observedInstance = instance;
      record.initialization = "reserved";
    }
    else if (phase === "pending" && record.initialization === "reserved") record.initialization = "pending";
    else throw new HcoordError("invalid_state", "spawn initialization phase cannot make that transition");
    event(state, at, `agent.spawn_initialization_${phase}`, record.parent, record.key, { pane: record.pane });
    return { changed: true, value: record };
  }
  if (operation === "agent.spawn.identity") {
    const record = own(state.spawnIntents, required(args, "intent"));
    if (!record || record.pane === null || record.status === "complete") throw new HcoordError("invalid_state", "spawn has no active saved pane");
    const instance = required(args, "instance"), session = required(args, "runtimeSession");
    if (!spawnObservationHolds(record, session, instance)) throw new HcoordError("identity_conflict", "spawn execution differs from its first observation");
    record.observedInstance = instance; record.observedSession = session;
    event(state, at, "agent.spawn_identity", record.parent, record.key, { pane: record.pane });
    return { changed: true, value: record };
  }
  if (operation === "agent.spawn.complete") {
    const record = own(state.spawnIntents, required(args, "intent"));
    if (!record || record.pane === null) throw new HcoordError("not_found", "spawn pane is not recorded");
    if (record.status === "complete") return { changed: false, value: { intent: record, participant: state.participants[record.participant!] } };
    const runtimeSession = required(args, "runtimeSession"), instance = required(args, "instance");
    if (!spawnObservationHolds(record, runtimeSession, instance)) throw new HcoordError("identity_conflict", "spawn execution changed before registration");
    const matches = Object.values(state.participants).filter((p) => sameExecution(p, { machine: record.machine, hostScope: record.hostScope, pane: record.pane, session: runtimeSession, instance }));
    if (matches.length) throw new HcoordError("identity_conflict", "spawned execution is already registered elsewhere", { candidates: matches });
    if (Object.keys(state.participants).length >= MAX_AGENTS) throw new HcoordError("capacity", `participant limit ${MAX_AGENTS} reached; retain the saved spawn and resolve retention before registration`);
    const runtime = required(args, "runtime") as Participant["runtime"];
    if (!["working", "idle", "done", "unknown"].includes(runtime)) throw new HcoordError("invalid_argument", "spawn runtime observation is invalid");
    const worktree = record.worktree && record.placement ? { repo: record.worktree.repo, branch: record.worktree.branch, path: record.placement.cwd } : null;
    const participant: Participant = { id: id("a"), machine: record.machine, hostScope: record.hostScope, session: runtimeSession, instance, name: record.name, project: worktree ? worktree.path : optional(args, "project"), parent: record.parent, pane: record.pane, runtime, connection: "connected", observedAt: at, worktree };
    put(state.participants, participant.id, participant);
    if (!record.noWatch) {
      const watch: Watch = { target: participant.id, observer: record.parent, generation: 1, status: "active", intervalMs: state.config.watchMs, dueAt: timed(at, state.config.watchMs), cycle: null, requestId: null, checkedAt: null, startedAt: at, stoppedAt: null, observation: null };
      state.watches[participant.id] = watch;
    }
    record.status = "complete"; record.participant = participant.id; record.reason = null; record.initialization = "complete";
    event(state, at, "agent.spawn_complete", participant.id, record.key, { parent: record.parent, watch: !record.noWatch });
    return { changed: true, value: { intent: record, participant, watch: state.watches[participant.id] ?? null } };
  }
  if (operation === "agent.observe") {
    const participant = person(state, required(args, "id"));
    const runtime = required(args, "runtime");
    const connection = required(args, "connection");
    if (!["working", "idle", "done", "unknown"].includes(runtime) || !["connected", "unavailable", "unverified"].includes(connection)) throw new HcoordError("invalid_argument", "invalid runtime observation");
    participant.runtime = runtime as Participant["runtime"];
    participant.connection = connection as Participant["connection"];
    participant.observedAt = at;
    state.updatedAt = at;
    const watch = own(state.watches, participant.id);
    if (watch?.status === "active") watch.observation = `${runtime}/${connection} at ${at}${optional(args, "reason") ? `: ${optional(args, "reason")}` : ""}`;
    return { changed: true, value: participant };
  }
  if (operation === "agent.list") {
    const project = optional(args, "project");
    return { changed: false, value: Object.values(state.participants).filter((p) => project === null || p.project === path.resolve(project)).map((p) => ({ ...p, watch: state.watches[p.id] ?? null, openRequests: Object.values(state.requests).filter((r) => r.status === "open" && (r.from === p.id || r.to === p.id)).length })) };
  }
  if (operation === "agent.show") {
    const participant = person(state, required(args, "id"));
    return { changed: false, value: { ...participant, watch: state.watches[participant.id] ?? null, requests: Object.values(state.requests).filter((r) => r.from === participant.id || r.to === participant.id).map((r) => ({ id: r.id, status: r.status })) } };
  }
  if (operation === "agent.end") {
    // A participant leaves (D-20): its watch stops, an unchecked cycle is
    // dropped, and every question it still has open is canceled, so no wake,
    // reminder or inbox item outlives it. Ending it again changes nothing.
    const participant = person(state, required(args, "id")), actor = required(args, "actor");
    const watch = own(state.watches, participant.id);
    authority(actor, [participant.id, participant.parent, watch?.observer ?? null]);
    let watchStopped = false, cycleDropped = false;
    const canceled: string[] = [];
    if (watch?.status === "active") { watch.status = "stopped"; watch.stoppedAt = at; watchStopped = true; event(state, at, "watch.stopped", watch.target, null, { generation: watch.generation }); }
    // A watch stopped earlier keeps its unchecked cycle for a restart; its request is canceled below.
    if (watch && watch.cycle !== null) { watch.cycle = null; watch.requestId = null; cycleDropped = true; }
    for (const item of Object.values(state.requests)) {
      if (item.from !== participant.id || item.status !== "open") continue;
      item.status = "canceled"; item.canceledAt = at;
      for (const delivery of item.deliveries) if (delivery.status === "pending" || delivery.status === "deferred") { delivery.status = "failed"; delivery.reason = "participant ended before submission"; }
      event(state, at, "request.canceled", item.id, item.intent);
      canceled.push(item.id);
    }
    const value = { participant: participant.id, watchStopped, canceled };
    if (!watchStopped && !cycleDropped && canceled.length === 0) return { changed: false, value };
    event(state, at, "agent.ended", participant.id, null, { by: actor, watchStopped, canceled: canceled.length });
    return { changed: true, value };
  }
  if (operation === "watch.start" || operation === "watch.assign") {
    const target = person(state, required(args, "target"));
    const observer = person(state, required(args, "observer"));
    if (target.id === observer.id) throw new HcoordError("invalid_argument", "a participant cannot watch itself");
    const previous = own(state.watches, target.id);
    const actor = required(args, "actor");
    if (previous?.status === "active") authority(actor, [previous.observer]);
    else if (actor !== "human" && !(operation === "watch.start" && previous === undefined && target.parent === actor && observer.id === actor)) throw new HcoordError("forbidden", "a missing observer can be assigned only by a human, except a parent starting its own child's first watch");
    if (operation === "watch.start" && previous?.status === "active") throw new HcoordError("conflict", "watch is already active; use watch assign with its current generation");
    if (operation === "watch.assign" && previous?.status === "active" && previous.observer === observer.id) throw new HcoordError("conflict", "the observer is unchanged; inspect the active watch instead");
    const expected = optional(args, "expectedGeneration");
    if (operation === "watch.assign" && expected === null) throw new HcoordError("invalid_argument", "watch assignment requires the generation currently shown by watch list");
    if (expected !== null && Number(expected) !== (previous?.generation ?? 0)) throw new HcoordError("conflict", "watch generation changed; inspect current observer before retry", { current: previous });
    const intervalMs = args["intervalMs"] === undefined ? previous?.intervalMs ?? state.config.watchMs : integerMs(args, "intervalMs");
    const carriedRequest = previous ? watchRequest(state, previous) : undefined;
    if (previous?.status === "active" && previous.cycle !== null && carriedRequest === undefined) throw new HcoordError("corrupt_ledger", "the active watch has an unchecked cycle without its request; no new cycle was created");
    if (previous) {
      if (state.watchHistory.length >= MAX_WATCH_HISTORY) throw new HcoordError("capacity", `watch history reached ${MAX_WATCH_HISTORY}; retain the existing assignment until old history expires`);
      state.watchHistory.push({ ...previous, status: "stopped", stoppedAt: previous.stoppedAt ?? at });
    }
    const carryCycle = previous !== undefined && previous.cycle !== null && carriedRequest !== undefined && carriedRequest.status !== "canceled";
    // A new observer starts outside quiet, so it hears once about a target that is already resting.
    const watch: Watch = { target: target.id, observer: observer.id, generation: (previous?.generation ?? 0) + 1, status: "active", intervalMs,
      dueAt: carryCycle ? previous!.dueAt : timed(at, intervalMs),
      cycle: carryCycle ? previous!.cycle : null, requestId: carryCycle ? carriedRequest!.id : null,
      checkedAt: previous?.checkedAt ?? null, startedAt: at, stoppedAt: null, observation: previous?.observation ?? null,
      brief: brief(args) ?? previous?.brief ?? null, quietSince: null };
    if (carryCycle && carriedRequest) {
      const sameObserver = previous!.observer === observer.id;
      if (!sameObserver) retireUnsent(carriedRequest, "watch assigned to another observer");
      carriedRequest.to = observer.id;
      // Reusing the same observer preserves a pending or uncertain submission and its receipt.
      const phase = carriedRequest.status === "answered" ? "watch_check" : "request";
      const existingWake = sameObserver && carriedRequest.deliveries.some((delivery) => delivery.recipient === observer.id && (delivery.phase ?? "request") === phase && ["pending", "deferred", "unknown", "accepted", "acknowledged"].includes(delivery.status));
      if (!existingWake) queueDelivery(carriedRequest, observer.id, at, phase);
    }
    const moved = operation === "watch.assign" && previous?.observer != null && previous.observer !== observer.id ? moveLetters(state, target.id, previous.observer, observer.id, at) : 0;
    put(state.watches, target.id, watch);
    event(state, at, previous ? "watch.assigned" : "watch.started", target.id, null, { observer: observer.id, generation: watch.generation, movedRequests: moved });
    return { changed: true, value: watch };
  }
  if (operation === "watch.stop") {
    const target = required(args, "target"), actor = required(args, "actor");
    const watch = own(state.watches, target);
    if (!watch || watch.status === "stopped") throw new HcoordError("not_found", "active watch does not exist");
    authority(actor, [watch.observer]);
    watch.status = "stopped"; watch.stoppedAt = at;
    event(state, at, "watch.stopped", target, null, { generation: watch.generation });
    return { changed: true, value: watch };
  }
  if (operation === "watch.check") {
    const target = required(args, "target"), actor = required(args, "actor"), cycle = required(args, "cycle");
    const watch = own(state.watches, target);
    if (!watch || watch.status !== "active") throw new HcoordError("not_found", "active watch does not exist");
    if (actor !== watch.observer) throw new HcoordError("forbidden", "only the assigned observer can confirm this watch cycle");
    if (watch.cycle !== cycle) throw new HcoordError("conflict", "cycle is not the current unchecked cycle", { currentCycle: watch.cycle });
    const checkRequest = watchRequest(state, watch);
    if (!checkRequest) throw new HcoordError("corrupt_ledger", "the active watch has no request for its current cycle");
    watch.checkedAt = at; watch.cycle = null; watch.requestId = null; watch.dueAt = timed(at, watch.intervalMs);
    if (checkRequest.status === "open") { checkRequest.status = "answered"; checkRequest.answeredAt = at; checkRequest.answer = "cycle checked"; checkRequest.respondent = actor; checkRequest.recordedBy = actor; }
    checkRequest.watchCheckedAt = at;
    retireUnsent(checkRequest, "watch cycle checked");
    for (const delivery of checkRequest.deliveries) {
      const phase = delivery.phase ?? "request";
      if (phase === "watch_check" && delivery.recipient === actor && delivery.status === "accepted") { delivery.status = "acknowledged"; delivery.acknowledgedAt = at; }
      else if (delivery.status === "unknown" || delivery.status === "failed") { const prior = delivery.reason ?? `prior delivery ${delivery.status}`; delivery.reason = `${prior}; watch cycle checked, no further submission needed`; }
      delivery.actionClosedAt = at;
    }
    event(state, at, "watch.checked", target, cycle, { observer: watch.observer });
    return { changed: true, value: watch };
  }
  if (operation === "watch.list") return { changed: false, value: Object.values(state.watches) };
  if (operation === "request.send") {
    const from = required(args, "from"), to = required(args, "to"), intent = required(args, "intent");
    person(state, from);
    if (to !== "human") person(state, to);
    const content = body(args);
    const prior = Object.values(state.requests).find((r) => r.from === from && r.intent === intent);
    if (prior) {
      if (prior.to !== to || prior.body !== content || (prior.initialIntermediary !== undefined ? prior.initialIntermediary : prior.intermediary) !== optional(args, "intermediary") || prior.context !== optional(args, "context") || prior.waiting !== (args["waiting"] === true) || prior.requiresReply !== (args["notifyOnly"] !== true)) throw new HcoordError("intent_conflict", "intent key already belongs to a different request", { requestId: prior.id });
      return { changed: false, value: prior };
    }
    if (Object.keys(state.requests).length >= MAX_REQUESTS) throw new HcoordError("capacity", `request limit ${MAX_REQUESTS} reached; existing requests remain intact`);
    const item: Request = { id: id("r"), intent, from, to, intermediary: optional(args, "intermediary"), initialIntermediary: optional(args, "intermediary"), body: content, context: optional(args, "context"), status: "open",
      requiresReply: args["notifyOnly"] !== true, waiting: args["waiting"] === true, createdAt: at, answeredAt: null, answer: null, respondent: null, recordedBy: null,
      canceledAt: null, lateAnswers: [], relayBody: null, relayAt: null, escalatedAt: null, remindedAt: null, relayRemindedAt: null, relayEscalatedAt: null, deliveryRemindedAt: null, deliveryEscalatedAt: null, deliveries: [] };
    if (item.intermediary !== null) person(state, item.intermediary);
    queueDelivery(item, to, at);
    put(state.requests, item.id, item);
    event(state, at, "request.created", item.id, intent, { from, to, waiting: item.waiting, requiresReply: item.requiresReply });
    return { changed: true, value: item };
  }
  if (operation === "request.show") {
    const item = request(state, required(args, "id"));
    const uncertain = item.deliveries.filter((delivery) => delivery.status === "unknown" && !delivery.actionClosedAt);
    const deferred = item.deliveries.filter((delivery) => !delivery.actionClosedAt && (delivery.status === "deferred" || (delivery.status === "failed" && !(item.status === "answered" && (delivery.phase ?? "request") === "request"))));
    const watch = own(state.watches, item.from);
    const nextAction = item.status === "canceled" ? "inspect late answers; cancellation does not undo accepted delivery"
      : uncheckedWatchRequest(state, item) && watch?.status === "active" ? "assigned observer must use watch check; a human may stop or assign the watch"
      : uncheckedWatchRequest(state, item) && item.status === "answered" ? "watch is stopped; a human may restart or assign it for an explicit check"
      : uncheckedWatchRequest(state, item) ? "watch is stopped; a human may restart or assign it, or sender/human may cancel the old request"
      : item.watchCheckedAt ? "watch cycle checked; previous transport outcomes remain in delivery history, with no further wake submission needed"
      : pendingRelay(item) ? "intermediary must inspect the original answer and relay it; inspect blocked delivery first"
      : uncertain.length ? "inspect unknown submission before attempting another external effect"
      : deferred.length ? "inspect delivery reason and recipient identity"
      : item.status === "open" && item.requiresReply ? "assigned recipient may reply or sender may cancel/escalate"
      : "inspect delivery and acknowledgement separately from work result";
    return { changed: false, value: { ...item, nextAction } };
  }
  if (operation === "request.reply") {
    const item = request(state, required(args, "id")), answer = body(args), respondent = required(args, "respondent"), recordedBy = required(args, "recordedBy");
    if (uncheckedWatchRequest(state, item)) throw new HcoordError("conflict", "watch requests require the assigned observer's watch check; reply cannot close a cycle");
    if (respondent !== "human") person(state, respondent);
    if (recordedBy !== "human") person(state, recordedBy);
    if (respondent !== item.to && !(respondent === "human" && item.escalatedAt !== null)) throw new HcoordError("forbidden", "reply must name the assigned recipient or an escalated human");
    if (item.status === "answered") throw new HcoordError("conflict", "request already answered; original answer remains unchanged", { requestId: item.id });
    if (item.status === "canceled") {
      item.lateAnswers.push({ at, body: answer, respondent, recordedBy });
      event(state, at, "request.late_reply", item.id, item.intent, { respondent, recordedBy });
      return { changed: true, value: item };
    }
    item.status = "answered"; item.answeredAt = at; item.answer = answer; item.respondent = respondent; item.recordedBy = recordedBy;
    if (respondent === "human" && item.to !== "human" && item.intermediary === null) item.intermediary = item.to;
    retireUnsent(item, "request answered before original submission", "request");
    queueDelivery(item, item.intermediary ?? item.from, at, "answer");
    event(state, at, "request.answered", item.id, item.intent, { respondent, recordedBy });
    return { changed: true, value: item };
  }
  if (operation === "request.relay") {
    const item = request(state, required(args, "id")), actor = required(args, "actor"), relay = body(args);
    authority(actor, [item.intermediary]);
    if (item.status !== "answered") throw new HcoordError("conflict", "only an answered request may be relayed");
    if (item.watchCheckedAt) throw new HcoordError("conflict", "watch cycle was checked; its old answer no longer needs relay");
    if (item.intermediary === null) throw new HcoordError("conflict", "request has no intermediary to relay its answer");
    if (item.relayBody !== null) {
      if (item.relayBody !== relay) throw new HcoordError("conflict", "a different relay is already recorded");
      return { changed: false, value: item };
    }
    item.relayBody = relay; item.relayAt = at;
    retireUnsent(item, "answer relayed before prior answer notice", "answer");
    retireUnsent(item, "answer relayed before escalation notice", "request");
    retireUnsent(item, "answer relayed before relay problem notice", "relay_problem");
    for (const delivery of item.deliveries) if (delivery.phase === "answer" && delivery.recipient === actor && delivery.status === "accepted") { delivery.status = "acknowledged"; delivery.acknowledgedAt = at; }
    queueDelivery(item, item.from, at, "relay");
    event(state, at, "request.relayed", item.id, item.intent, { by: actor });
    return { changed: true, value: item };
  }
  if (operation === "request.cancel") {
    const item = request(state, required(args, "id"));
    authority(required(args, "actor"), [item.from]);
    const watch = own(state.watches, item.from);
    const stoppedCycle = uncheckedWatchRequest(state, item) && watch?.status === "stopped";
    if (uncheckedWatchRequest(state, item) && watch?.status === "active") throw new HcoordError("conflict", "an active watch cycle cannot be canceled as a question; use watch check, stop, or assign");
    if (item.status === "canceled") {
      if (!stoppedCycle) return { changed: false, value: item };
      watch!.cycle = null; watch!.requestId = null;
      event(state, at, "watch.cycle_canceled", item.from, item.intent);
      return { changed: true, value: item };
    }
    if (item.status === "answered") throw new HcoordError("conflict", "answered request cannot be canceled");
    item.status = "canceled"; item.canceledAt = at;
    if (stoppedCycle) { watch!.cycle = null; watch!.requestId = null; }
    for (const delivery of item.deliveries) if (delivery.status === "pending" || delivery.status === "deferred") { delivery.status = "failed"; delivery.reason = "request canceled before submission"; }
    event(state, at, "request.canceled", item.id, item.intent);
    return { changed: true, value: item };
  }
  if (operation === "request.escalate") {
    const item = request(state, required(args, "id"));
    authority(required(args, "actor"), [item.from, item.to, item.intermediary]);
    if (item.status !== "open") throw new HcoordError("conflict", "only an open request may be escalated");
    if (item.escalatedAt !== null) return { changed: false, value: item };
    item.escalatedAt = at;
    queueDelivery(item, "human", at);
    event(state, at, "request.escalated", item.id, item.intent);
    return { changed: true, value: item };
  }
  if (operation === "request.ack") {
    const item = request(state, required(args, "id"));
    const actor = required(args, "actor");
    const deliveryId = optional(args, "delivery");
    const delivery = deliveryId === null ? [...item.deliveries].reverse().find((d) => d.recipient === actor)
      : item.deliveries.find((d) => d.id === deliveryId && d.recipient === actor);
    if (delivery?.status === "acknowledged") return { changed: false, value: item };
    if (!delivery || delivery.status !== "accepted") throw new HcoordError("conflict", "there is no accepted delivery to acknowledge");
    delivery.status = "acknowledged"; delivery.acknowledgedAt = at;
    if (delivery.phase === "relay") retireUnsent(item, "child acknowledged the relay", "delivery_problem");
    event(state, at, "delivery.acknowledged", item.id, item.intent, { deliveryId: delivery.id });
    return { changed: true, value: item };
  }
  if (operation === "inbox") {
    const items: Array<Record<string, unknown>> = [];
    for (const item of Object.values(state.requests)) {
      if ((item.status === "open" && item.requiresReply && (item.to === "human" || item.escalatedAt !== null)) || pendingRelay(item)) items.push({ kind: pendingRelay(item) ? "relay_problem" : "question", requestId: item.id, createdAt: item.createdAt, from: item.from, to: item.to, status: item.status, nextAction: pendingRelay(item) ? "inspect parent delivery and relay the recorded answer" : uncheckedWatchRequest(state, item) ? "assign or restart the watch for an explicit check, or stop and cancel the old request" : "reply or cancel this request" });
      if (item.status !== "canceled" && !pendingRelay(item) && item.deliveries.some((delivery) => !delivery.actionClosedAt && (delivery.status === "unknown" || delivery.status === "deferred" || (delivery.status === "failed" && !(item.status === "answered" && (delivery.phase ?? "request") === "request"))))) items.push({ kind: "delivery_problem", requestId: item.id, createdAt: item.createdAt, nextAction: "inspect delivery history and recipient identity" });
    }
    for (const [machine, entry] of Object.entries(state.machines)) if (entry.problem) items.push({ kind: "machine_problem", machine, code: entry.problem.code, reason: entry.problem.message, at: entry.problem.at, nextAction: entry.problem.code === "version_mismatch" ? `install the HQ's hcoord version on ${machine}` : entry.problem.code === "remote_not_installed" ? `install hcoord on ${machine}` : `fix SSH access to ${machine}; hcoord stores no credentials` });
    for (const record of Object.values(state.letters)) if (record.outcome !== "applied" && !record.reported) items.push({ kind: "letter_rejected", letter: record.id, origin: record.origin, operation: record.operation, at: record.at, code: record.code, reason: record.message, nextAction: record.outcome === "unsupported" ? "upgrade the writing machine's hcoord to this coordinator's version; the letter stays in its outbox" : "the letter was not applied; resend the command if it is still needed" });
    for (const watch of Object.values(state.watches)) if (watch.status === "active" && (watch.observer === null || own(state.participants, watch.observer)?.connection === "unavailable" || !own(state.participants, watch.observer))) items.push({ kind: "watch_unassigned", target: watch.target, observer: watch.observer, nextAction: "inspect current observer and assign explicitly" });
    return { changed: false, value: items };
  }
  if (operation === "graph") return { changed: false, value: { participants: Object.values(state.participants), creation: Object.values(state.participants).filter((p) => p.parent).map((p) => ({ parent: p.parent, child: p.id })), watch: [...state.watchHistory, ...Object.values(state.watches)], prunedBefore: state.prunedBefore } };
  if (operation === "events") {
    const cursor = args["cursor"] === undefined ? 0 : Number(args["cursor"]);
    if (!Number.isSafeInteger(cursor) || cursor < 0) throw new HcoordError("invalid_argument", "cursor must be a nonnegative integer");
    if ((state.events.length && cursor < state.events[0]!.seq - 1) || (!state.events.length && state.prunedBefore !== null && cursor < state.seq)) throw new HcoordError("cursor_expired", "event cursor expired; reread status and graph");
    const entries = state.events.filter((e) => e.seq > cursor).slice(0, 256);
    const nextCursor = entries.at(-1)?.seq ?? state.seq;
    return { changed: false, value: { events: entries, cursor: nextCursor, hasMore: nextCursor < state.seq, observedAt: at } };
  }
  if (operation === "tick") {
    let cycles = 0, reminders = 0, escalations = 0;
    let changed = false;
    if (args["runRetention"] !== false) {
      const before = Date.parse(at) - state.config.retentionMs;
      for (const [key, item] of Object.entries(state.requests)) {
        if (terminalRequest(state, item) && Date.parse(item.watchCheckedAt ?? item.answeredAt ?? item.canceledAt ?? item.createdAt) < before) { delete state.requests[key]; state.prunedBefore = at; changed = true; }
      }
      const requestRefs = new Set<string>(), unresolvedRefs = new Set<string>();
      for (const item of Object.values(state.requests)) for (const agent of [item.from, item.to, item.intermediary]) if (agent !== null && agent !== "human") {
        requestRefs.add(agent);
        if (!terminalRequest(state, item)) unresolvedRefs.add(agent);
      }
      for (const [key, watch] of Object.entries(state.watches)) if (watch.status === "stopped" && watch.stoppedAt !== null && Date.parse(watch.stoppedAt) < before && !unresolvedRefs.has(key) && (watch.observer === null || !unresolvedRefs.has(watch.observer))) { delete state.watches[key]; state.prunedBefore = at; changed = true; }
      const history = state.watchHistory.filter((watch) => watch.stoppedAt === null || Date.parse(watch.stoppedAt) >= before || unresolvedRefs.has(watch.target) || (watch.observer !== null && unresolvedRefs.has(watch.observer)));
      if (history.length !== state.watchHistory.length) { state.watchHistory = history; state.prunedBefore = at; changed = true; }
      const watchRefs = new Set<string>();
      for (const watch of [...Object.values(state.watches), ...state.watchHistory]) { watchRefs.add(watch.target); if (watch.observer !== null) watchRefs.add(watch.observer); }
      const parentRefs = new Set(Object.values(state.participants).map((person) => person.parent).filter((parent): parent is string => parent !== null));
      const uncertainParentRefs = new Set(Object.values(state.spawnIntents).filter((intent) => intent.status === "unknown").map((intent) => intent.parent));
      const completedIntents = new Map<string, string[]>();
      for (const [key, intent] of Object.entries(state.spawnIntents)) if (intent.status === "complete" && intent.participant !== null) completedIntents.set(intent.participant, [...(completedIntents.get(intent.participant) ?? []), key]);
      for (const [key, participant] of Object.entries(state.participants)) {
        if (participant.runtime !== "done" || Date.parse(participant.observedAt) >= before || requestRefs.has(key) || watchRefs.has(key) || parentRefs.has(key) || uncertainParentRefs.has(key)) continue;
        delete state.participants[key];
        for (const intentKey of completedIntents.get(key) ?? []) delete state.spawnIntents[intentKey];
        state.prunedBefore = at; changed = true;
      }
      for (const [key, intent] of Object.entries(state.spawnIntents)) if (intent.status === "reserved" && Date.parse(intent.at) < before) { delete state.spawnIntents[key]; state.prunedBefore = at; changed = true; }
      for (const [key, record] of Object.entries(state.letters)) if (Date.parse(record.at) < before) { delete state.letters[key]; state.prunedBefore = at; changed = true; }
      const retained = state.events.filter((entry) => Date.parse(entry.at) >= before);
      if (retained.length !== state.events.length) { state.events = retained; state.prunedBefore = at; changed = true; }
    }
    const observed = args["observedTargets"] === undefined ? null : new Set(Array.isArray(args["observedTargets"]) ? args["observedTargets"] : []);
    for (const watch of Object.values(state.watches)) {
      if (watch.status !== "active" || Date.parse(watch.dueAt) > Date.parse(at)) continue;
      if (observed !== null && !observed.has(watch.target)) continue;
      changed = true;
      const target = own(state.participants, watch.target);
      const resting = target !== undefined && target.runtime !== "working";
      if (resting && watch.quietSince) { watch.dueAt = timed(at, watch.intervalMs); continue; }
      if (resting) { watch.quietSince = at; event(state, at, "watch.quiet", watch.target, null, { runtime: target!.runtime }); }
      else if (watch.quietSince) { watch.quietSince = null; event(state, at, "watch.resumed", watch.target); }
      if (watch.cycle === null) {
        watch.cycle = id("c"); cycles += 1;
        if (watch.observer !== null) {
          const item: Request = { id: id("r"), intent: `watch:${watch.target}:${watch.generation}:${watch.cycle}`, from: watch.target, to: watch.observer, intermediary: null, body: `Check participant ${watch.target} for cycle ${watch.cycle}`, context: watch.observation, status: "open", requiresReply: true, waiting: false, createdAt: at, answeredAt: null, answer: null, respondent: null, recordedBy: null, canceledAt: null, lateAnswers: [], relayBody: null, relayAt: null, escalatedAt: null, remindedAt: null, deliveries: [] };
          if (Object.keys(state.requests).length >= MAX_REQUESTS) throw new HcoordError("capacity", `request limit ${MAX_REQUESTS} reached; watch cycle was not saved`);
          queueDelivery(item, watch.observer, at);
          state.requests[item.id] = item;
          watch.requestId = item.id;
          event(state, at, "watch.cycle", watch.target, watch.cycle, { requestId: item.id });
        }
      } else {
        const checkRequest = watchRequest(state, watch);
        if (checkRequest?.status === "open") checkRequest.context = watch.observation;
      }
      watch.dueAt = timed(at, watch.intervalMs);
    }
    for (const item of Object.values(state.requests)) {
      if (pendingRelay(item) && item.answeredAt !== null) {
        const age = Date.parse(at) - Date.parse(item.answeredAt);
        if (age >= state.config.escalateMs && !item.relayEscalatedAt) {
          if (!item.relayRemindedAt) { item.relayRemindedAt = at; event(state, at, "request.relay_reminder_coalesced", item.id, item.intent); }
          item.relayEscalatedAt = at; escalations += 1; changed = true;
          queueDelivery(item, "human", at, "relay_problem");
          event(state, at, "request.relay_escalated", item.id, item.intent);
        } else if (age >= state.config.remindMs && !item.relayRemindedAt) {
          item.relayRemindedAt = at; reminders += 1; changed = true;
          if (item.intermediary !== null && !item.deliveries.some((delivery) => delivery.phase === "relay_problem" && delivery.recipient === item.intermediary)) queueDelivery(item, item.intermediary, at, "relay_problem");
          event(state, at, "request.relay_reminded", item.id, item.intent);
        }
      }
      if (item.status === "answered" && !item.watchCheckedAt && item.relayBody !== null && item.relayAt !== null && item.deliveries.some((delivery) => delivery.phase === "relay" && delivery.status !== "acknowledged")) {
        const age = Date.parse(at) - Date.parse(item.relayAt);
        if (age >= state.config.remindMs && !item.deliveryRemindedAt) {
          item.deliveryRemindedAt = at; reminders += 1; changed = true;
          if (item.intermediary !== null && !item.deliveries.some((delivery) => delivery.phase === "delivery_problem" && delivery.recipient === item.intermediary)) queueDelivery(item, item.intermediary, at, "delivery_problem");
          event(state, at, "request.delivery_reminded", item.id, item.intent);
        }
        if (age >= state.config.escalateMs && !item.deliveryEscalatedAt) {
          item.deliveryEscalatedAt = at; escalations += 1; changed = true;
          queueDelivery(item, "human", at, "delivery_problem");
          event(state, at, "request.delivery_escalated", item.id, item.intent);
        }
      }
      if (item.status !== "open" || !item.requiresReply) continue;
      const quiet = own(state.watches, item.from);
      if (quiet?.status === "active" && quiet.quietSince && uncheckedWatchRequest(state, item)) continue;
      const age = Date.parse(at) - Date.parse(item.createdAt);
      if (age >= state.config.escalateMs && item.escalatedAt === null) {
        if (item.remindedAt === null) { item.remindedAt = at; event(state, at, "request.reminder_coalesced", item.id, item.intent); }
        item.escalatedAt = at; escalations += 1; changed = true; queueDelivery(item, "human", at); event(state, at, "request.escalated", item.id, item.intent);
      } else if (age >= state.config.remindMs && item.remindedAt === null) { item.remindedAt = at; reminders += 1; changed = true; if (!item.deliveries.some((d) => d.status === "pending" || d.status === "deferred" || d.status === "unknown")) queueDelivery(item, item.to, at); event(state, at, "request.reminded", item.id, item.intent); }
    }
    return { changed, value: { cycles, reminders, escalations, at } };
  }
  throw new HcoordError("unknown_operation", `unsupported operation ${operation}`);
}
