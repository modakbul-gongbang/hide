import { localDeviceId, type AgentRow, type SessionRecord, type SessionWorkSummary, type SnapshotRest, type WorkOutput, type WorkSession, type WorkTask } from "./snapshot";

export const workKey = (session: WorkSession) => `${session.device_id}\0${session.agent}\0${session.session_id}`;
export function sessionOfAgent(agent: AgentRow, rest: SnapshotRest | null): WorkSession | null {
  return agent.session_id ? { device_id: agent.device_id ?? localDeviceId(rest), agent: agent.agent_kind, session_id: agent.session_id } : null;
}
export function recordTitle(record: Pick<SessionRecord, "title" | "manual_title" | "native_title" | "fallback_title">, summaries: boolean): string {
  return record.manual_title ?? (summaries ? record.title : record.native_title ?? record.fallback_title);
}
export function recordTree(record: SessionRecord): SessionRecord[] {
  return [record, ...record.children.flatMap(recordTree)];
}
export function taskState(task: WorkTask, outputs: readonly WorkOutput[]) {
  const prs = outputs.filter((output) => output.kind === "pr" && output.task_id === task.id);
  if (prs.some((output) => output.state === "ci_failed")) return "ci_failed";
  if (prs.some((output) => ["open", "review"].includes(output.state))) return "review";
  if (task.done) return "done";
  return task.end === "done" ? "unfinished" : task.end === "waiting" ? "working" : task.end;
}
const OUTPUT_ORDER: Record<string, number> = { ci_failed: 0, review: 1, open: 2, merged: 3, closed: 4 };
export function issueDraftFailureKey(message: string | null | undefined) {
  return message === "issue_draft_repository_unknown" ? "sessionWork.issue_draft_repository_unknown" : message === "issue_draft_repository_changed" ? "sessionWork.issue_draft_repository_changed" : "sessionWork.issue_draft_unavailable";
}
export function orderedOutputs(outputs: readonly WorkOutput[]): WorkOutput[] {
  return [...outputs].sort((a, b) => (OUTPUT_ORDER[a.state] ?? 5) - (OUTPUT_ORDER[b.state] ?? 5) || b.created_at_unix_ms - a.created_at_unix_ms);
}
/** Scope comes from the existing Sessions model; ended means resolved or absent. */
export function endedWork(work: readonly SessionWorkSummary[], agents: readonly AgentRow[], rest: SnapshotRest | null, checkout: string | null): SessionWorkSummary[] {
  const live = new Set(agents.filter((agent) => !agent.resolved).flatMap((agent) => { const session = sessionOfAgent(agent, rest); return session ? [workKey(session)] : []; }));
  return work.filter((row) => row.open_missed > 0 && !live.has(workKey(row.session)) && (!checkout || row.cwd === checkout || row.cwd?.startsWith(`${checkout}/`)));
}
