import { ArrowLeftIcon, CheckIcon, ChevronRightIcon, FlagIcon, GitPullRequestIcon, CircleDotIcon } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { Actions } from "./actions";
import { AgentMark } from "./AgentMark";
import { agentProductName } from "./agentAdapters";
import { deviceConnected } from "./devices";
import { Button } from "./components/ui/button";
import { Hint } from "./components/ui/tooltip";
import { useInterfaceTranslation } from "./i18n/client";
import { cn } from "./lib/utils";
import { issueDraftFailureKey, orderedOutputs, recordTitle, recordTree, taskState, workKey } from "./sessionWork";
import type { AgentStatusCode, MissedWork, SessionRecord as RecordValue, WorkOutput, WorkSession, WorkTask } from "./snapshot";
import { statusText } from "./agentStatus";
import { useShellStore } from "./store";
import { summariesEnabled } from "./hideAi";
import { SESSION_INDENT_LEVELS } from "./sessionTree";

const ROW = "min-w-0 rounded-xs text-left outline-none focus-visible:ring-1 focus-visible:ring-ring";
const TONES = { done: "text-success", merged: "text-pr-merged", working: "text-agent-working", blocked: "text-warning", question: "text-warning", unfinished: "text-warning", waiting: "text-subtle-foreground", review: "text-success", ci_failed: "text-destructive", open: "text-success", closed: "text-muted-foreground" };

export function SessionRecord({ actions, target }: { actions: Actions; target: WorkSession }) {
  const { t } = useInterfaceTranslation();
  const section = useShellStore((state) => state.sessionRecord);
  const rest = useShellStore((state) => state.rest);
  const summaries = summariesEnabled(rest?.status?.background_ai);
  const connected = deviceConnected(rest, target.device_id);
  const own = section?.session && workKey(section.session) === workKey(target) ? section : null;
  const records = useMemo(() => own?.record ? recordTree(own.record) : [], [own?.record]);
  const sourceNames = useMemo(() => new Map(records.map((record) => [workKey(record.session), recordTitle(record, summaries)])), [records, summaries]);
  const wasConnected = useRef(connected);
  useEffect(() => () => { actions.closeSessionRecord(); }, [actions]);
  useEffect(() => {
    if (connected && !wasConnected.current) actions.openSessionRecord(target);
    wasConnected.current = connected;
  }, [actions, connected, target]);
  return <div className="flex min-h-0 min-w-0 flex-1 flex-col" data-session-record="true">
    <Button size="sm" variant="ghost" className="self-start px-md" onClick={() => actions.closeSessionRecord()}><ArrowLeftIcon />{t("sessionWork.back")}</Button>
    {!own || own.loading ? <div className="space-y-md px-md py-sm" aria-busy="true" data-record-loading="true">{[0, 1, 2].map((key) => <div key={key} className="h-(--size-control-lg) rounded-sm bg-muted" />)}</div> : own.failure ? <div role="status" className="px-md py-sm text-caption"><p>{t("sessionWork.failed")}</p><Button size="sm" variant="ghost" onClick={() => actions.openSessionRecord(target)}>{t("common.retry")}</Button></div> : own.record ? <div className={cn("min-h-0 flex-1 overflow-y-auto px-md pb-md text-caption [word-break:keep-all]", !connected && "opacity-(--opacity-disabled)")} data-record-content="true">
      <h2 className="flex min-w-0 items-center gap-xs py-sm text-subhead font-semibold"><AgentMark kind={target.agent} /><span className="min-w-0 truncate" title={recordTitle(own.record, summaries)}>{recordTitle(own.record, summaries)}</span></h2>
      <p className="mb-sm text-micro text-muted-foreground">{[own.record.state ? statusText(t, own.record.state as AgentStatusCode) : null, `${t("sessionWork.children")} ${records.length - 1}`, !summaries ? `${t("sessionWork.requests")} ${own.record.requests.length}` : null].filter(Boolean).join(" · ")}</p>
      {summaries && own.record.tasks.length > 0 ? <Verdict record={own.record} /> : null}
      <h3 className="mt-md mb-xs font-medium text-subtle-foreground">{t("sessionWork.outputs")} <span className="text-micro text-muted-foreground">{`PR ${own.record.outputs.filter((output) => output.kind === "pr").length}`} · {t("issue.label")} {own.record.outputs.filter((output) => output.kind === "issue").length}</span></h3>
      {orderedOutputs(own.record.outputs).map((output) => <OutputRow key={`${workKey(output.source)}:${output.kind}:${output.repository}:${output.number}`} output={output} sourceName={sourceNames.get(workKey(output.source)) ?? output.source_name} actions={actions} />)}
      {summaries ? <MissedList record={own.record} actions={actions} connected={connected} /> : null}
      <WorkList record={own.record} summaries={summaries} />
      {own.record.children.length ? <><h3 className="mt-md mb-xs font-medium text-subtle-foreground">{t("sessionWork.children")} {records.length - 1}</h3><Children record={own.record} summaries={summaries} /></> : null}
    </div> : null}
  </div>;
}

function Verdict({ record }: { record: RecordValue }) {
  const { t } = useInterfaceTranslation();
  const done = record.tasks.filter((task) => task.done).length;
  const working = record.tasks.filter((task) => ["working", "review"].includes(taskState(task, record.outputs))).length;
  const left = record.tasks.length - done - working;
  return <div className="space-y-xs rounded-sm bg-card p-sm" data-record-verdict="true">
    <div className="flex flex-wrap items-baseline gap-xs"><strong className="text-title font-semibold tabular-nums text-success">{done}<span className="text-caption text-muted-foreground">/{record.tasks.length}</span></strong><span className="text-success">{t("sessionWork.done")}</span><span className="ml-auto text-micro text-muted-foreground">{t("sessionWork.working")} {working} · {t("sessionWork.unfinished")} {left}</span></div>
    <div className="flex h-(--spacing-xs) gap-xxs" role="img" aria-label={`${done}/${record.tasks.length} ${t("sessionWork.done")}`}>
      {done > 0 ? <span className="rounded-xs bg-success" style={{ flex: done }} /> : null}{working > 0 ? <span className="rounded-xs bg-agent-working" style={{ flex: working }} /> : null}{left > 0 ? <span className="rounded-xs bg-muted-foreground" style={{ flex: left }} /> : null}
    </div>
  </div>;
}

function OutputRow({ output, sourceName, actions }: { output: WorkOutput; sourceName: string; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const state = output.state in TONES ? output.state as keyof typeof TONES : "open";
  const Icon = output.kind === "pr" ? GitPullRequestIcon : CircleDotIcon;
  const stale = output.stale ? output.last_read_at_unix_ms ? new Date(output.last_read_at_unix_ms).toLocaleString() : t("common.unavailable") : null;
  return <button type="button" className={cn(ROW, "flex w-full items-center gap-xs py-xxs hover:bg-accent")} data-record-output={output.kind} onClick={() => actions.openRecordedOutput(output)}>
    <Hint label={stale ?? t(`sessionWork.${state}`)}><span className={cn("inline-flex shrink-0 items-center gap-xxs text-micro", TONES[state], output.stale && "opacity-(--opacity-disabled)")}><Icon className="size-(--size-icon-sm)" />#{output.number} {t(`sessionWork.${state}`)}</span></Hint><span className="min-w-0 flex-1 truncate" title={output.title}>{output.title}</span><Hint label={sourceName}><span data-output-source="true" className="max-w-1/4 truncate text-micro text-muted-foreground">{sourceName}</span></Hint>
  </button>;
}

export function MissedRow({ missed, record, actions }: { missed: MissedWork; record: Pick<RecordValue, "session" | "title">; actions: Actions; connected: boolean }) {
  const { t } = useInterfaceTranslation();
  const rest = useShellStore((state) => state.rest);
  const connected = deviceConnected(rest, record.session.device_id);
  const draft = rest?.issue_work?.draft;
  const ownDraft = draft && workKey(draft.source.session) === workKey(record.session) && draft.source.id === missed.id ? draft : null;
  const drafting = ownDraft?.phase === "working";
  const busy = missed.creating || draft?.phase === "working";
  return <li className="min-w-0 py-xs" data-missed-work={missed.id} data-missed-status={missed.status}>
    <p className="flex items-start gap-xs"><FlagIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0 text-warning" /><span className="min-w-0 whitespace-normal [overflow-wrap:anywhere]" title={missed.text}>{missed.text}</span></p>
    <button type="button" className={cn(ROW, "mt-xxs block w-full truncate text-micro text-muted-foreground")} title={record.title} onClick={() => actions.openSessionRecord(record.session)}>{record.title} · {t(`sessionWork.kind.${missed.kind}`)}</button>
    {missed.status === "open" ? <div className="mt-xs flex gap-xs"><Hint label={!connected ? t("devices.rail.notConnected") : t("sessionWork.issue")}><span><Button size="sm" variant="outline" disabled={!connected || busy} data-missed-issue={missed.id} onClick={() => actions.draftMissedIssue(record.session, missed.id)}>{t(drafting ? "sessionWork.drafting" : "sessionWork.issue")}</Button></span></Hint><Button size="sm" variant="ghost" disabled={busy} onClick={() => actions.setMissedStatus(record.session, missed.id, "dismissed")}>{t("sessionWork.dismiss")}</Button>{drafting ? <Button size="sm" variant="ghost" onClick={() => actions.cancelMissedIssueDraft()}>{t("common.cancel")}</Button> : null}</div> : missed.status === "issue" && missed.issue_number && missed.issue_repository ? <Button size="sm" variant="link" onClick={() => actions.openRecordedIssue(record.session, missed.issue_repository!, missed.issue_number!)}>#{missed.issue_number}</Button> : <div className="mt-xs flex items-center gap-xs text-micro text-muted-foreground"><span>{t(missed.status === "resolved" ? "sessionWork.resolved" : "sessionWork.dismissed")}</span><Button size="sm" variant="ghost" onClick={() => actions.setMissedStatus(record.session, missed.id, "open")}>{t(missed.status === "resolved" ? "sessionWork.reopen" : "sessionWork.undo")}</Button></div>}
    {ownDraft?.phase === "failed" ? <p role="status" className="mt-xxs text-micro text-destructive">{t(issueDraftFailureKey(ownDraft.message))}</p> : null}
  </li>;
}

function MissedList({ record, actions, connected }: { record: RecordValue; actions: Actions; connected: boolean }) {
  const { t } = useInterfaceTranslation();
  const all = recordTree(record).flatMap((owner) => owner.missed.map((missed) => ({ owner, missed }))).sort((a, b) => b.missed.at_unix_ms - a.missed.at_unix_ms);
  const open = all.filter(({ missed }) => missed.status === "open"), closed = all.filter(({ missed }) => missed.status !== "open");
  if (!all.length) return null;
  return <section className="mt-md rounded-sm bg-warning/10 p-sm" data-record-missed="true"><h3 className="font-medium text-warning">{t("sessionWork.missed")} {open.length}</h3><ul>{open.map(({ owner, missed }) => <MissedRow key={`${workKey(owner.session)}:${missed.id}`} missed={missed} record={owner} actions={actions} connected={connected} />)}</ul>{closed.length ? <details><summary className={cn(ROW, "cursor-pointer py-xs text-micro text-muted-foreground")}>{t("sessionWork.closedCount", { count: closed.length })}</summary><ul>{closed.map(({ owner, missed }) => <MissedRow key={`${workKey(owner.session)}:${missed.id}`} missed={missed} record={owner} actions={actions} connected={connected} />)}</ul></details> : null}</section>;
}

function RequestRow({ text, at }: { text: string; at: number }) {
  return <div className="flex min-w-0 gap-xs py-xxs text-micro text-subtle-foreground" data-record-request="true"><time className="shrink-0 text-muted-foreground" dateTime={new Date(at).toISOString()}>{new Date(at).toLocaleString([], { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" })}</time><span className="min-w-0 whitespace-normal [overflow-wrap:anywhere]" title={text}>{text.split(/\r?\n/)[0]}</span></div>;
}

function WorkList({ record, summaries }: { record: RecordValue; summaries: boolean }) {
  const { t } = useInterfaceTranslation();
  const tasks = summaries ? record.tasks : [];
  const requests = summaries ? record.requests.filter((request) => !request.judged || request.task_ids.length === 0) : record.requests;
  const chapters = summaries ? [{ id: null, goal: null, at_unix_ms: 0 }, ...record.chapters].reverse() : [{ id: null, goal: null, at_unix_ms: 0 }];
  const chapterOf = (at: number) => summaries ? record.chapters.filter((chapter) => chapter.at_unix_ms <= at).at(-1)?.id ?? null : null;
  return <section className="mt-md" data-record-work="true"><h3 className="mb-xs font-medium text-subtle-foreground">{t(tasks.length ? "sessionWork.tasks" : "sessionWork.requests")}</h3>
    {!tasks.length && !requests.length ? <p className="text-micro text-muted-foreground">{t("sessionWork.empty")}</p> : null}
    {chapters.map((chapter) => {
      const rows = [...tasks.filter((task) => task.chapter_id === chapter.id).map((task) => ({ at: task.at_unix_ms, task, request: null })), ...requests.filter((request) => !summaries || (request.chapter_id ?? chapterOf(request.at_unix_ms)) === chapter.id).map((request) => ({ at: request.at_unix_ms, task: null, request }))].sort((a, b) => summaries ? a.at - b.at : b.at - a.at);
      if (!rows.length) return null;
      return <div key={chapter.id ?? "raw"}>
      {chapter.goal ? <div className="mt-sm mb-xs flex flex-wrap items-baseline gap-xs"><h4 className="min-w-0 text-caption font-medium">{chapter.goal}</h4><time className="text-micro text-muted-foreground">{new Date(chapter.at_unix_ms).toLocaleString()}</time></div> : null}
      {rows.map((row) => row.task ? <TaskRow key={row.task.id} task={row.task} record={record} /> : row.request ? <RequestRow key={`request:${row.request.offset}`} text={row.request.text} at={row.request.at_unix_ms} /> : null)}
    </div>; })}
  </section>;
}

function TaskRow({ task, record }: { task: WorkTask; record: RecordValue }) {
  const { t } = useInterfaceTranslation();
  const state = taskState(task, record.outputs);
  const numbers = record.outputs.filter((output) => output.task_id === task.id).map((output) => `${output.kind === "pr" ? "PR" : t("issue.label")} #${output.number}`).join(" · ");
  return <details data-record-task={task.id} className="py-xxs"><summary className={cn(ROW, "flex cursor-pointer items-baseline gap-xs", state === "done" && "text-subtle-foreground")}>
    <span aria-label={t(`sessionWork.${state}`)} className={TONES[state]}>{state === "done" ? <CheckIcon className="size-(--size-icon-sm)" /> : state === "blocked" ? "⛔" : "◐"}</span>
    <span className="min-w-0 flex-1">{task.name}{numbers ? <span className="ml-xs text-micro text-muted-foreground">{numbers}</span> : null}</span><span className={cn("shrink-0 text-micro", TONES[state])}>{t(`sessionWork.${state}`)}</span>
  </summary><div className="ml-sm border-l border-lineage-rail pl-sm">{record.requests.filter((request) => request.task_ids.includes(task.id)).map((request) => <RequestRow key={request.offset} text={request.text} at={request.at_unix_ms} />)}</div></details>;
}

function Children({ record, summaries, depth = 1 }: { record: RecordValue; summaries: boolean; depth?: number }) {
  const { t } = useInterfaceTranslation();
  const [visible, setVisible] = useState(5);
  return <div className={cn("min-w-0", depth <= SESSION_INDENT_LEVELS && "border-l border-lineage-rail pl-sm")}>{record.children.slice(0, visible).map((child) => <details key={workKey(child.session)} data-record-child={child.session.session_id}><summary className={cn(ROW, "flex cursor-pointer items-center gap-xs py-xs")}><ChevronRightIcon aria-hidden="true" className="size-(--size-icon-sm)" /><span className="min-w-0 flex-1 truncate" title={`${recordTitle(child, summaries)}\n${agentProductName(child.session.agent)}`}>{recordTitle(child, summaries)}<span className="sr-only">, {agentProductName(child.session.agent)}</span></span><span className="shrink-0 text-micro text-muted-foreground">{child.state ? statusText(t, child.state as AgentStatusCode) : null}</span><span className="shrink-0 text-micro text-pr-merged">{child.outputs.filter((output) => output.kind === "pr").map((output) => `#${output.number}`).join(" ")}</span></summary><WorkList record={child} summaries={summaries} />{child.children.length ? <Children record={child} summaries={summaries} depth={depth + 1} /> : null}</details>)}{record.children.length > visible ? <Button size="sm" variant="ghost" onClick={() => setVisible((count) => count + 5)}>{t("sessionWork.moreChildren", { count: record.children.length - visible })}</Button> : null}</div>;
}
