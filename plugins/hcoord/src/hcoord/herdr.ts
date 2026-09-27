import path from "node:path";
import { CODEX_INITIALIZATION_PROMPT, getAgent, initializedCodex, prepareCodexFirstTurn, promptAgent, runHerdrCommand, startAgentWhenPaneReady } from "../implement/herdr";
import { HcoordError, sameExecution, validateSpawnSpec, type Delivery, type Participant, type Request, type SpawnIntent, type Watch } from "./model";
import { herdrRoute, isLocalMachine, requireRemoteHerdr } from "./remote";

/** Herdr routing for a record that names its machine and socket scope. */
const at = (record: { machine: string; hostScope: string }) => herdrRoute(record.machine, record.hostScope);

/**
 * Confirms the pane hosts the named execution before registration (D-18: the
 * same pane and session; the terminal only for a sessionless agent). The
 * terminal Herdr reports now is what registration records. A remote machine
 * must be a saved, enabled Herdr machine whose server answers (PRD B2, B16).
 */
export function validateBinding(machine: string, session: string, instance: string, pane: string | null, hostScope = "default"): { pane: string; runtime: Participant["runtime"]; instance: string } {
  if (!isLocalMachine(machine)) requireRemoteHerdr(machine);
  if (pane === null) throw new HcoordError("invalid_argument", "registration requires an exact --pane execution target");
  const found = getAgent(pane, herdrRoute(machine, hostScope), 2000);
  if (found.kind !== "found") throw new HcoordError("runtime_unavailable", `Herdr did not confirm the specified pane: ${found.detail}`);
  if (!sameExecution({ machine, hostScope, pane, session, instance }, { machine, hostScope, pane: found.agent.paneId, session: found.agent.sessionId, instance: found.agent.terminalId })) throw new HcoordError("identity_conflict", "the pane hosts a different execution; select its current session", { current: { pane: found.agent.paneId, session: found.agent.sessionId, instance: found.agent.terminalId } });
  return { pane, runtime: found.agent.status === "blocked" ? "unknown" : found.agent.status, instance: found.agent.terminalId ?? instance };
}

type LocalBinding = Pick<Participant, "machine" | "hostScope" | "session" | "instance" | "pane">;
type ParticipantInspection = { runtime: Participant["runtime"]; connection: Participant["connection"]; reason: string; interactiveReady: boolean | null };
export const OFFICIAL_PROMPT_BOUNDARY = {
  transport: "Herdr 0.9.1 agent.prompt",
  preflight: "same pane and session (the terminal only for a sessionless agent), idle or done lifecycle, and interactive readiness unless Herdr reports none",
  atomicInputProtection: false,
  limitation: "Herdr 0.9.1 does not atomically bind prompt submission to the preflight identity or protect human typing between inspection and submission",
} as const;

export function inspectParticipant(participant: LocalBinding): ParticipantInspection {
  if (participant.pane === null) return { runtime: "unknown", connection: "unverified", reason: "participant has no exact pane binding", interactiveReady: null };
  const found = getAgent(participant.pane, at(participant), 1000);
  if (found.kind !== "found") return { runtime: "unknown", connection: "unavailable", reason: found.detail, interactiveReady: null };
  const agent = found.agent;
  if (!sameExecution(participant, { machine: participant.machine, hostScope: participant.hostScope, pane: agent.paneId, session: agent.sessionId, instance: agent.terminalId })) return { runtime: "unknown", connection: "unavailable", reason: "execution identity changed", interactiveReady: null };
  return { runtime: agent.status === "blocked" ? "unknown" : agent.status, connection: "connected", reason: agent.status === "blocked" ? "recipient is blocked" : "the recorded execution observed in its pane", interactiveReady: agent.interactiveReady };
}

/**
 * Whether a connected execution may take typed input now. The recipient must
 * be idle or done; a readiness flag of false holds it. Herdr 0.9.1 reports
 * `interactive_ready` only for agents it started, so a hand-started Observer
 * has no flag at all, and requiring `true` held every delivery to such an
 * Observer for six hours of a live run (2026-09-26).
 * Without a flag, idle or done is the readiness evidence Herdr gives.
 */
export function inputReadiness(runtime: Participant["runtime"], interactiveReady: boolean | null): { ready: boolean; reason: string } {
  if (runtime !== "idle" && runtime !== "done") return { ready: false, reason: `recipient is ${runtime}; submission deferred` };
  if (interactiveReady === false) return { ready: false, reason: "Herdr reports the recipient is not interactive-ready; submission deferred" };
  return { ready: true, reason: interactiveReady === true ? "exact idle or done execution is interactive-ready" : "exact idle or done execution; Herdr reports no readiness flag for it" };
}

export function inspectDelivery(recipient: Participant): { ready: boolean; reason: string; runtime: Participant["runtime"]; connection: Participant["connection"] } {
  const observed = inspectParticipant(recipient);
  const { runtime, connection } = observed;
  if (connection !== "connected") return { ready: false, ...observed };
  if (observed.reason === "recipient is blocked") return { ready: false, reason: observed.reason, runtime, connection };
  return { ...inputReadiness(runtime, observed.interactiveReady), runtime, connection };
}

/**
 * The dispatch preflight and registration check. Under `inputReadiness` no
 * connected execution is permanently unready: a working or unflagged one
 * receives input once it is idle, so the check is the exact identity and the
 * official prompt API, and a hand-started Observer is accepted (D-17, B22).
 */
export function officialDeliveryAvailable(participant: LocalBinding): { ready: boolean; reason: string } {
  const observed = inspectParticipant(participant);
  if (observed.connection !== "connected") return { ready: false, reason: observed.reason };
  const support = officialPromptSupport(participant);
  return support.ready ? { ready: true, reason: "Observer execution and official agent.prompt API confirmed; delivery remains non-atomic" } : support;
}

export function officialPromptSupport(route?: { machine: string; hostScope: string }): { ready: boolean; reason: string } {
  const help = route === undefined ? runHerdrCommand(["agent", "prompt", "--help"], 2000) : at(route).run!(["agent", "prompt", "--help"], undefined, 2000);
  if (help.status !== 0 || !/herdr agent prompt <TARGET> <TEXT>/.test(`${help.stdout}${help.stderr}`)) return { ready: false, reason: "official Herdr agent.prompt API could not be confirmed" };
  return { ready: true, reason: "official Herdr agent.prompt API confirmed; delivery remains non-atomic" };
}

/**
 * The notice a recipient acts on: an identifier line, the one command that
 * closes it, and the content it carries (D-21). How to handle each kind lives
 * in the recipient's own instructions, not in every message; a live run
 * found the repeated explanations long and read past them (2026-09-26). The
 * command names every ID it needs, because a remote agent cannot query the
 * HQ (PRD B8, B9). `peer` is the participant the notice is about, and a watch
 * brief is the watcher's note, carried verbatim.
 */
export function messageForDelivery(item: Request, delivery: Delivery, watch: Pick<Watch, "cycle" | "brief"> | null, peer?: Participant): string {
  const me = delivery.recipient;
  const from = peer ? `${peer.name} (${item.from})` : item.from;
  const lines = (...parts: Array<string | null | undefined>): string => parts.filter((part): part is string => typeof part === "string" && part !== "").join("\n");
  const cycle = watch?.cycle ?? null;
  if (cycle !== null && (delivery.phase === "request" || delivery.phase === "watch_check" || delivery.phase === undefined)) {
    return lines(`HCOORD_WATCH_CHECK ${from} cycle ${cycle}`, `close: hcoord watch check ${item.from} --cycle ${cycle} --actor ${me}`, watch?.brief);
  }
  if (delivery.phase === "relay_problem") return lines(`HCOORD_RELAY_PROBLEM ${item.id}`, `relay: hcoord request relay ${item.id} --actor ${me} --body <text>`, `answer from ${item.respondent ?? "unknown"}: ${item.answer ?? ""}`);
  if (delivery.phase === "delivery_problem") {
    const inspect = peer?.pane ? `herdr ${isLocalMachine(peer.machine) ? "" : `--machine ${peer.machine} `}agent read ${peer.pane} --source recent-unwrapped --lines 80` : `hcoord request show ${item.id}`;
    return lines(`HCOORD_DELIVERY_PROBLEM ${item.id} ${from} has not acknowledged the relay`, `inspect: ${inspect}`);
  }
  if (delivery.phase === "relay") return lines(`HCOORD_RELAY ${item.id}`, `ack: hcoord request ack ${item.id} --actor ${me} --delivery ${delivery.id}`, item.relayBody);
  if (delivery.phase === "answer") return lines(`HCOORD_ANSWER ${item.id}`, item.intermediary === me ? `relay: hcoord request relay ${item.id} --actor ${me} --body <text>` : `ack: hcoord request ack ${item.id} --actor ${me} --delivery ${delivery.id}`, `answer: ${item.answer ?? ""}`);
  if (!item.requiresReply) return lines(`HCOORD_NOTICE ${item.id} from ${from}`, "no reply needed", item.body, item.context === null ? null : `context: ${item.context}`);
  return lines(`HCOORD_REQUEST ${item.id} from ${from}`, `reply: hcoord request reply ${item.id} --as ${me} --body <answer> | escalate: hcoord request escalate ${item.id} --actor ${me}`, item.body, item.context === null ? null : `context: ${item.context}`);
}

export function submitOfficial(item: Request, delivery: Delivery, recipient: Participant, watch: Pick<Watch, "cycle" | "brief"> | null, peer?: Participant): { status: Delivery["status"]; code: string; reason: string } {
  if (recipient.pane === null) throw new HcoordError("invalid_state", "recipient pane missing at submission");
  const result = promptAgent({ target: recipient.pane, text: messageForDelivery(item, delivery, watch, peer), expectedInputGuard: null }, at(recipient), 2000);
  if (result.path !== "session-match") throw new HcoordError("runtime_unavailable", "Herdr adapter returned an unexpected prompt path; inspect the delivery outcome");
  return { status: result.outcome === "accepted" ? "accepted" : result.outcome === "rejected" ? "deferred" : "unknown", code: result.code, reason: result.detail };
}

const herdrJson = (text: string): Record<string, unknown> => {
  try { const parsed = JSON.parse(text) as Record<string, unknown>; if (parsed && typeof parsed === "object") return parsed; }
  catch { /* structural failure below */ }
  throw new HcoordError("runtime_unavailable", "Herdr returned invalid JSON; spawn state remains uncertain");
};

export function parentPlacement(parent: Participant): { workspace: string; cwd: string } {
  if (parent.pane === null) throw new HcoordError("identity_conflict", "parent has no exact pane binding");
  validateBinding(parent.machine, parent.session, parent.instance, parent.pane, parent.hostScope);
  const result = at(parent).run!(["pane", "get", parent.pane], undefined, 2000);
  if (result.status !== 0) throw new HcoordError("runtime_unavailable", "Herdr could not inspect the parent pane");
  const data = herdrJson(result.stdout);
  const pane = (data["result"] as Record<string, unknown> | undefined)?.["pane"] as Record<string, unknown> | undefined;
  if (pane?.["pane_id"] !== parent.pane || typeof pane?.["workspace_id"] !== "string" || typeof pane?.["cwd"] !== "string") throw new HcoordError("runtime_unavailable", "Herdr parent pane identity, workspace, or cwd is unconfirmed");
  return { workspace: pane["workspace_id"], cwd: pane["cwd"] };
}

export function createSpawnPane(record: SpawnIntent, placement: { workspace: string; cwd: string }): string {
  const result = at(record).run!(["tab", "create", "--workspace", placement.workspace, "--cwd", placement.cwd, "--label", record.name, "--no-focus"], undefined, 5000);
  if (result.status !== 0) throw new HcoordError("spawn_uncertain", "Herdr did not confirm tab creation; do not repeat this intent without inspecting the original result");
  const data = herdrJson(result.stdout);
  const created = data["result"] as Record<string, unknown> | undefined;
  const pane = created?.["root_pane"] as Record<string, unknown> | undefined;
  if (typeof pane?.["pane_id"] !== "string") throw new HcoordError("spawn_uncertain", "Herdr returned no pane ID after tab creation; do not create another tab");
  return pane["pane_id"];
}

/**
 * Creates the child's worktree and workspace with Herdr on the target machine
 * (PRD D-09). Herdr's own refusal means nothing was created; any other
 * failure leaves the outcome unknown and is never retried blindly (PRD B5).
 */
export function createSpawnWorktree(record: SpawnIntent): { pane: string; workspace: string; cwd: string } {
  const tree = record.worktree;
  if (!tree) throw new HcoordError("invalid_state", "spawn intent has no worktree request");
  const result = at(record).run!(["worktree", "create", "--cwd", tree.repo, "--branch", tree.branch, ...(tree.path ? ["--path", tree.path] : []), "--label", record.name, "--no-focus"], undefined, 20_000);
  if (result.status !== 0) {
    const refusal = (() => { try { return (JSON.parse(result.stderr || result.stdout) as { error?: { code?: string; message?: string } }).error; } catch { return undefined; } })();
    if (refusal?.code === "not_git_worktree") throw new HcoordError("repo_missing", `no Git repository at ${tree.repo} on ${record.machine}; clone the source repository there first`, { code: refusal.code });
    if (typeof refusal?.code === "string") throw new HcoordError("worktree_failed", `Herdr refused the worktree on ${record.machine}: ${refusal.message ?? refusal.code}`, { code: refusal.code });
    throw new HcoordError("spawn_uncertain", `worktree creation on ${record.machine} has an unknown outcome; inspect herdr --machine ${record.machine} worktree list and retry this intent with --reconcile-pane <root pane>`, { intent: record.key, pane: null, unfinishedStep: "create_worktree" });
  }
  const created = (herdrJson(result.stdout)["result"] ?? {}) as Record<string, unknown>;
  const root = created["root_pane"] as Record<string, unknown> | undefined;
  if (typeof root?.["pane_id"] !== "string" || typeof root["workspace_id"] !== "string" || typeof root["cwd"] !== "string") throw new HcoordError("spawn_uncertain", "Herdr created a worktree without a root pane record; inspect it and reconcile this intent", { intent: record.key, pane: null, unfinishedStep: "create_worktree" });
  return { pane: root["pane_id"], workspace: root["workspace_id"], cwd: root["cwd"] };
}

/** The placement of a pane a person named after an uncertain worktree creation. */
export function observedPlacement(record: SpawnIntent, pane: string): { workspace: string; cwd: string } {
  const result = at(record).run!(["pane", "get", pane], undefined, 2000);
  if (result.status !== 0) throw new HcoordError("spawn_uncertain", "the named pane is unavailable; inspect it before reconciling", { pane });
  const data = (herdrJson(result.stdout)["result"] as Record<string, unknown> | undefined)?.["pane"] as Record<string, unknown> | undefined;
  if (data?.["pane_id"] !== pane || typeof data["workspace_id"] !== "string" || typeof data["cwd"] !== "string") throw new HcoordError("spawn_uncertain", "Herdr did not confirm the named pane's workspace and cwd", { pane });
  return { workspace: data["workspace_id"], cwd: data["cwd"] };
}

export function confirmSpawnPane(record: SpawnIntent, placement: { workspace: string; cwd: string }): void {
  if (record.pane === null) throw new HcoordError("spawn_uncertain", "the spawn intent has no pane ID to inspect");
  const result = at(record).run!(["pane", "get", record.pane], undefined, 2000);
  if (result.status !== 0) throw new HcoordError("spawn_uncertain", "the recorded pane is unavailable; inspect it before resuming", { pane: record.pane });
  const data = herdrJson(result.stdout);
  const pane = (data["result"] as Record<string, unknown> | undefined)?.["pane"] as Record<string, unknown> | undefined;
  if (pane?.["pane_id"] !== record.pane || pane?.["workspace_id"] !== placement.workspace || pane?.["cwd"] !== placement.cwd) {
    throw new HcoordError("identity_conflict", "the pane does not match the spawn intent's saved placement", { pane: record.pane });
  }
}

export type SpawnInspection = { state: "absent" } | { state: "initializing"; instance: string; interactiveReady: boolean | null; runtime: Participant["runtime"]; blocked: boolean } | { state: "ready"; session: string; instance: string; runtime: Participant["runtime"] };

export function inspectSpawnedAgent(record: SpawnIntent): SpawnInspection {
  if (record.pane === null) return { state: "absent" };
  const found = getAgent(record.pane, at(record), 2000);
  if (found.kind === "absent") return { state: "absent" };
  if (found.kind !== "found") throw new HcoordError("spawn_uncertain", "Herdr cannot inspect the spawned pane; retain its ID and retry after reconnection", { intent: record.key, pane: record.pane, unfinishedStep: "inspect_agent" });
  if (found.agent.paneId !== record.pane || found.agent.name !== record.name || found.agent.kind !== record.kind) throw new HcoordError("identity_conflict", "spawn pane hosts a different execution; no binding was changed", { intent: record.key, pane: record.pane });
  if (found.agent.terminalId === null) throw new HcoordError("spawn_uncertain", "spawned agent has no terminal identity yet", { intent: record.key, pane: record.pane, unfinishedStep: "inspect_agent" });
  const runtime = found.agent.status === "blocked" ? "unknown" : found.agent.status;
  if (found.agent.sessionId === null) return { state: "initializing", instance: found.agent.terminalId, interactiveReady: found.agent.interactiveReady, runtime, blocked: found.agent.status === "blocked" };
  return { state: "ready", session: found.agent.sessionId, instance: found.agent.terminalId, runtime };
}

/** The first Codex turn follows agent start, including an explicit task. */
function codexLaunchArgs(nativeArgs: string[]): { startArgs: string[]; prompt: string } {
  const valueFlags = new Set(["-c", "--config", "-i", "--image", "-m", "--model", "-p", "--profile", "-s", "--sandbox", "-C", "--cd", "--add-dir", "-a", "--ask-for-approval", "--remote", "--remote-auth-token-env", "--local-provider", "--enable", "--disable"]);
  for (let index = 0; index < nativeArgs.length; index += 1) {
    const arg = nativeArgs[index]!;
    if (arg === "--") {
      if (nativeArgs.length - index !== 2) throw new HcoordError("invalid_argument", "Codex accepts one task argument after --");
      return { startArgs: nativeArgs.slice(0, index), prompt: nativeArgs[index + 1]! };
    }
    if (valueFlags.has(arg)) { index += 1; continue; }
    if (arg.startsWith("-")) continue;
    if (index !== nativeArgs.length - 1) throw new HcoordError("invalid_argument", "Codex task must be the final native argument");
    return { startArgs: nativeArgs.slice(0, index), prompt: arg };
  }
  return { startArgs: nativeArgs, prompt: CODEX_INITIALIZATION_PROMPT };
}

/**
 * A started agent that shows its own question before reporting a session,
 * such as Claude's folder-trust prompt in a new worktree (observed on mini,
 * 2026-09-24). That consent belongs to a person; hcoord never answers it.
 */
export function blockedSpawnError(record: SpawnIntent): HcoordError {
  const where = isLocalMachine(record.machine) ? "" : `--machine ${record.machine} `;
  return new HcoordError("spawn_blocked", `the agent in pane ${record.pane} on ${record.machine} is waiting on its own prompt (for example a folder-trust or permission question); answer it there (herdr ${where}agent read ${record.pane}), then retry this intent`, { intent: record.key, pane: record.pane, unfinishedStep: "agent_prompt" });
}

export function startSpawnedAgent(record: SpawnIntent): void {
  if (record.pane === null) throw new HcoordError("invalid_state", "spawn intent has no pane");
  validateSpawnSpec(record.name, record.kind);
  const nativeArgs = record.kind === "codex" ? codexLaunchArgs(record.nativeArgs).startArgs : record.nativeArgs;
  const { result } = startAgentWhenPaneReady(["agent", "start", record.name, "--kind", record.kind, "--pane", record.pane, ...(nativeArgs.length ? ["--", ...nativeArgs] : [])], at(record));
  if (result.status !== 0) throw new HcoordError("spawn_uncertain", `Herdr did not confirm agent start (${(result.stderr || result.stdout).trim().slice(0, 200) || "no diagnostic"}); inspect the saved pane before retry`, { intent: record.key, pane: record.pane, unfinishedStep: "agent_start" });
}

export function prepareSpawnInitialization(record: SpawnIntent, instance: string, advanceUpdate = true): void {
  if (record.pane === null || record.kind !== "codex") throw new HcoordError("invalid_state", "Codex initialization requires its saved pane");
  const observed = prepareCodexFirstTurn({ paneId: record.pane, name: record.name }, at(record), advanceUpdate);
  if (observed.kind !== "found") throw new HcoordError("spawn_uncertain", `Codex first-turn preflight could not confirm its composer: ${observed.detail}`, { intent: record.key, pane: record.pane, unfinishedStep: "initialize_agent" });
  if (observed.agent.terminalId !== instance || observed.agent.sessionId !== null) throw new HcoordError("identity_conflict", "spawn execution changed before initialization submission", { intent: record.key, pane: record.pane });
}

export function submitSpawnInitialization(record: SpawnIntent, instance: string): { outcome: "accepted" | "rejected" | "unknown"; code: string } {
  if (record.pane === null || record.kind !== "codex") throw new HcoordError("invalid_state", "Codex initialization requires its saved pane");
  const observed = inspectSpawnedAgent(record);
  if (observed.state !== "initializing" || observed.instance !== instance) throw new HcoordError("identity_conflict", "spawn execution changed before initialization submission", { intent: record.key, pane: record.pane });
  if (observed.interactiveReady !== true || (observed.runtime !== "idle" && observed.runtime !== "done")) throw new HcoordError("spawn_uncertain", "spawned agent is not interactive-ready; inspect the saved pane before retry", { intent: record.key, pane: record.pane, unfinishedStep: "initialize_agent" });
  prepareSpawnInitialization(record, instance, false);
  const result = promptAgent({ target: record.pane, text: codexLaunchArgs(record.nativeArgs).prompt, expectedInputGuard: null }, at(record));
  return { outcome: result.outcome, code: result.code };
}

export function waitForSpawnInitialization(record: SpawnIntent): void {
  if (record.pane === null) throw new HcoordError("invalid_state", "spawn intent has no pane");
  const isInitialTurn = codexLaunchArgs(record.nativeArgs).prompt === CODEX_INITIALIZATION_PROMPT;
  const observed = initializedCodex({ paneId: record.pane, name: record.name }, at(record), isInitialTurn);
  if (observed.kind !== "found" || observed.agent.sessionId === null || observed.agent.terminalId === null) {
    inspectSpawnedAgent(record);
    throw new HcoordError("spawn_uncertain", "Codex first turn did not confirm an execution identity; inspect the saved pane and retry this intent without resubmitting it", { intent: record.key, pane: record.pane, unfinishedStep: "initialize_agent" });
  }
}

export function discoverAgents(registered: Participant[], project: string | null, machine = "local", hostScope = "default"): { items: Array<Record<string, unknown>>; partialFailures: string[] } {
  const result = herdrRoute(machine, hostScope).run!(["agent", "list"], undefined, 500);
  if (result.status !== 0) return { items: [], partialFailures: [`${machine} Herdr agent discovery is unavailable`] };
  try {
    const parsed = herdrJson(result.stdout);
    const raw = (parsed["result"] as Record<string, unknown> | undefined)?.["agents"];
    if (!Array.isArray(raw)) throw new Error("missing agents");
    const items: Array<Record<string, unknown>> = [];
    for (const candidate of raw.slice(0, 2048)) {
      if (!candidate || typeof candidate !== "object") continue;
      const agent = candidate as Record<string, unknown>;
      const pane = agent["pane_id"], cwd = agent["cwd"];
      const session = (agent["agent_session"] as Record<string, unknown> | undefined)?.["value"];
      const instance = agent["terminal_id"];
      if (typeof pane !== "string" || typeof session !== "string" || typeof instance !== "string") continue;
      if (project !== null && (typeof cwd !== "string" || path.resolve(cwd) !== path.resolve(project))) continue;
      if (registered.some((p) => sameExecution(p, { machine, hostScope, pane, session, instance }))) continue;
      items.push({ id: `discovered:${machine}:${hostScope}:${session}:${instance}`, registered: false, watch: null, machine, hostScope, session, instance, pane, name: agent["name"] ?? null, project: cwd ?? null, runtime: agent["agent_status"] ?? "unknown", connection: "connected", observedAt: new Date().toISOString() });
    }
    return { items, partialFailures: [] };
  } catch { return { items: [], partialFailures: [`${machine} Herdr agent list returned an invalid response`] }; }
}
