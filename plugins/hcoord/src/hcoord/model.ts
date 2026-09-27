export const SCHEMA = "hcoord.ledger.v1" as const;
export const API_VERSION = 1;
export const MAX_AGENTS = 2048;
export const MAX_REQUESTS = 10000;
export const MAX_EVENTS = 20000;
// A spawn can write six progress events; one extra slot records a rejected
// first-turn submission without stranding the saved intent at the event cap.
export const SPAWN_EVENT_SLOTS = { reserve: 7, beforeExternalStart: 4, beforeFirstTurn: 4, beforeRegistration: 2 } as const;
export const MAX_BODY_BYTES = 16 * 1024;
/** A watch brief rides on every watch check, so it stays a few lines (D-21). */
export const MAX_BRIEF_BYTES = 2 * 1024;
export const MAX_LEDGER_BYTES = 64 * 1024 * 1024;
export const MAX_CONNECTIONS = 64;
export const MAX_SPAWN_INTENTS = 4096;
export const MAX_WATCH_HISTORY = 20000;
export const MAX_QUEUE = 256;
export const MAX_MESSAGE_BYTES = 1024 * 1024;
// Every agent write travels as a letter so a stopped daemon or an unreachable
// HQ delays the write instead of losing it (PRD D-10, D-11, D-12).
export const LETTER_SCHEMA = "hcoord.letter.v1" as const;
// The HQ and each remote hcoord must speak the same remote protocol before the
// HQ registers, spawns on, or collects from that machine (PRD D-12, D-17).
export const REMOTE_PROTOCOL = 1;
export const MAX_OUTBOX_LETTERS = 1024;
export const MAX_LETTER_RECORDS = 20000;
export const LETTER_OPERATIONS = new Set(["config.set", "agent.register", "agent.spawn", "watch.start", "watch.assign", "watch.stop", "watch.check", "request.send", "request.reply", "request.relay", "request.ack", "request.cancel", "request.escalate", "agent.end"]);
export const DEFAULTS = { watchMs: 5 * 60_000, remindMs: 15 * 60_000, escalateMs: 30 * 60_000, retentionMs: 30 * 24 * 60 * 60_000 };

export type RequestStatus = "open" | "answered" | "canceled";
export type DeliveryStatus = "pending" | "deferred" | "accepted" | "acknowledged" | "unknown" | "failed" | "superseded";
export interface Participant {
  id: string; machine: string; hostScope: string; session: string; instance: string; name: string; project: string | null;
  parent: string | null; pane: string | null; runtime: "working" | "idle" | "done" | "unknown";
  connection: "connected" | "unavailable" | "unverified"; observedAt: string;
  /** Where a worktree spawn placed this agent (PRD B3). */
  worktree?: { repo: string; branch: string; path: string } | null;
}
/** Where an execution runs and what Herdr calls it; `null` where Herdr reports nothing. */
export interface ExecutionBinding { machine: string; hostScope: string; pane: string | null; session: string | null; instance: string | null }

/**
 * Whether two bindings name the same execution (measured 2026-09-26): the
 * same machine, host scope, pane and session. A Herdr restart gives every
 * pane a new terminal id and clears agent names while the pane and the
 * agent's session stay, and matching the terminal made 7 of 12 live
 * participants undeliverable (2026-09-26), so terminal and name are recorded,
 * never required to match. Only an execution Herdr reports without a session
 * is told apart by its terminal. Every identity check in hcoord uses this.
 */
export function sameExecution(recorded: ExecutionBinding, observed: ExecutionBinding): boolean {
  if (recorded.machine !== observed.machine || recorded.hostScope !== observed.hostScope) return false;
  if (recorded.pane === null || observed.pane !== recorded.pane) return false;
  if (recorded.session !== null && observed.session !== null) return observed.session === recorded.session;
  return recorded.instance !== null && observed.instance === recorded.instance;
}

export interface Watch {
  target: string; observer: string | null; generation: number; status: "active" | "stopped";
  intervalMs: number; dueAt: string; cycle: string | null; requestId?: string | null; checkedAt: string | null;
  startedAt: string; stoppedAt: string | null; observation: string | null;
  /** The watcher's own note, carried verbatim on every watch check; hcoord never reads it (D-21). */
  brief?: string | null;
  /**
   * When the target was first seen not working. The one check for that
   * change was opened then; later due cycles are skipped, and the open one is
   * neither reminded nor escalated, until the target works again (D-20).
   */
  quietSince?: string | null;
}
export interface Delivery {
  id: string; requestId: string; recipient: string; status: DeliveryStatus; reason: string | null;
  phase?: "request" | "answer" | "relay" | "relay_problem" | "delivery_problem" | "watch_check";
  reservedAt: string; attemptedAt: string | null; acceptedAt: string | null; acknowledgedAt: string | null;
  runtimeCode: string | null; actionClosedAt?: string | null;
}
export interface Request {
  id: string; intent: string; from: string; to: string; intermediary: string | null; initialIntermediary?: string | null;
  body: string; context: string | null; status: RequestStatus; requiresReply: boolean; waiting: boolean;
  createdAt: string; answeredAt: string | null; answer: string | null; respondent: string | null;
  recordedBy: string | null; canceledAt: string | null; lateAnswers: Array<{ at: string; body: string; respondent: string; recordedBy: string }>;
  relayBody: string | null; relayAt: string | null; escalatedAt: string | null; remindedAt: string | null;
  watchCheckedAt?: string | null;
  relayRemindedAt?: string | null; relayEscalatedAt?: string | null;
  deliveryRemindedAt?: string | null; deliveryEscalatedAt?: string | null;
  deliveries: Delivery[];
}
export interface Letter { schema: string; id: string; operation: string; args: Record<string, unknown>; createdAt: string; writer: { host: string; protocol: number } }
/** One processed or refused letter. `reported` means the writer already saw the outcome directly. */
export interface LetterRecord { id: string; origin: string; operation: string; at: string; outcome: "applied" | "rejected" | "unsupported"; code: string | null; message: string | null; reported: boolean }
export interface Event { seq: number; at: string; type: string; subjectId: string; correlationId: string | null; detail: Record<string, string | number | boolean | null> }
export interface SpawnIntent { key: string; parent: string; machine: string; hostScope: string; session: string; name: string; kind: string; nativeArgs: string[]; noWatch: boolean; status: "reserved" | "unknown" | "complete"; pane: string | null; participant: string | null; reason: string | null; at: string; placement?: { workspace: string; cwd: string }; worktree?: { repo: string; branch: string; path: string | null } | null; initialization?: "pending" | "reserved" | "complete"; observedInstance?: string | null; observedSession?: string | null }
export interface Ledger {
  schema: typeof SCHEMA; seq: number; updatedAt: string; config: typeof DEFAULTS;
  participants: Record<string, Participant>; watches: Record<string, Watch>; watchHistory: Watch[]; requests: Record<string, Request>;
  spawnIntents: Record<string, SpawnIntent>; events: Event[]; prunedBefore: string | null;
  letters: Record<string, LetterRecord>;
  /** A remote machine's last collection refusal that needs a person (auth, install, version); cleared by the next success. */
  machines: Record<string, { problem: { code: string; message: string; at: string } | null }>;
}
export function emptyLedger(now: string): Ledger {
  return { schema: SCHEMA, seq: 0, updatedAt: now, config: { ...DEFAULTS }, participants: {}, watches: {}, watchHistory: [], requests: {}, spawnIntents: {}, events: [], prunedBefore: null, letters: {}, machines: {} };
}
// Persisted dictionaries are plain JSON objects, including after structuredClone.
// Own-key access prevents caller IDs such as __proto__ from becoming records.
export function own<T>(records: Record<string, T>, key: string): T | undefined {
  return Object.hasOwn(records, key) ? records[key] : undefined;
}
export function put<T>(records: Record<string, T>, key: string, value: T): void {
  Object.defineProperty(records, key, { value, enumerable: true, configurable: true, writable: true });
}
export class HcoordError extends Error {
  constructor(readonly code: string, message: string, readonly detail?: Record<string, unknown>) { super(message); this.name = "HcoordError"; }
}
export function validateSpawnSpec(name: string, kind: string): void {
  if (!/^[a-z][a-z0-9_-]{0,31}$/.test(name)) throw new HcoordError("invalid_argument", "Herdr agent name must use lowercase letters, digits, _ or -, up to 32 characters");
  if (!["codex", "claude", "opencode", "gemini"].includes(kind)) throw new HcoordError("unsupported_runtime", "this Herdr agent kind has not been verified for coordinator spawn");
}
export function id(prefix: string): string { return `${prefix}_${require("node:crypto").randomUUID()}`; }
export function event(state: Ledger, at: string, type: string, subjectId: string, correlationId: string | null = null, detail: Event["detail"] = {}): void {
  if (state.events.length >= MAX_EVENTS) throw new HcoordError("capacity", `event history reached ${MAX_EVENTS}; resolve retention or export before adding work`);
  state.seq += 1;
  state.events.push({ seq: state.seq, at, type, subjectId, correlationId, detail });
  state.updatedAt = at;
}
