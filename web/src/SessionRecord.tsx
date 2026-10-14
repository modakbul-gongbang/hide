import { ArrowLeftIcon, CheckIcon, ChevronRightIcon, CircleDotIcon, FlagIcon, LoaderCircleIcon, MessageCircleIcon, UserIcon } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Actions } from "./actions";
import { AgentMark } from "./AgentMark";
import { agentProductName } from "./agentAdapters";
import { deviceConnected } from "./devices";
import { Button } from "./components/ui/button";
import { PrMark } from "./components/pr-mark";
import { Hint } from "./components/ui/tooltip";
import { formatDateTime } from "./i18n/format";
import { requireInterfaceLanguage } from "./i18n/locale";
import { useInterfaceTranslation } from "./i18n/client";
import { cn } from "./lib/utils";
import { checklist, orderedOutputs, outputPrState, recordTitle, recordTree, workKey, type Checklist, type ChecklistTask, type TaskState } from "./sessionWork";
import type { AgentStatusCode, MissedWork, SessionRecord as RecordValue, WorkOutput, WorkRequest, WorkSession } from "./snapshot";
import { statusText } from "./agentStatus";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { summariesEnabled } from "./hideAi";
import { SESSION_INDENT_LEVELS } from "./sessionTree";
import { PR_LOOK } from "./prMark";

const ROW = "min-w-0 rounded-xs text-left outline-none focus-visible:ring-1 focus-visible:ring-ring";
/** Under a task's line: the icon column's width and its gap, so what hangs from a task lines up with its name. */
const LANE = "pl-[calc(var(--size-icon-sm)+var(--spacing-xs))]";
/** A task's mark: its colour is its state, and the state's name is in its hint and accessible name. */
const GLYPH: Record<TaskState, string> = { done: "text-success", working: "text-agent-working", review: "text-success", ci_failed: "text-success", unfinished: "text-warning", blocked: "text-warning", question: "text-warning" };
const FIRST_OPEN_REQUESTS = 5;

export function SessionRecord({ actions, target }: { actions: Actions; target: WorkSession }) {
  const { t } = useInterfaceTranslation();
  const section = useShellStore((state) => state.sessionRecord);
  const rest = useShellStore((state) => state.rest);
  const words = useUiStore((state) => state.sessionRecordWords);
  const summaries = summariesEnabled(rest?.status?.background_ai);
  const connected = deviceConnected(rest, target.device_id);
  const own = section?.session && workKey(section.session) === workKey(target) ? section : null;
  const records = useMemo(() => own?.record ? recordTree(own.record) : [], [own?.record]);
  const sourceNames = useMemo(() => new Map(records.map((record) => [workKey(record.session), recordTitle(record, summaries)])), [records, summaries]);
  const descendants = useMemo(() => records.slice(1), [records]);
  const wasConnected = useRef(connected);
  useEffect(() => () => { actions.closeSessionRecord(); }, [actions]);
  useEffect(() => {
    if (connected && !wasConnected.current) actions.openSessionRecord(target);
    wasConnected.current = connected;
  }, [actions, connected, target]);
  const record = own?.record;
  return <div className="flex min-h-0 min-w-0 flex-1 flex-col" data-session-record="true">
    <Button size="sm" variant="ghost" className="self-start px-md" onClick={() => actions.closeSessionRecord()}><ArrowLeftIcon />{t("sessionWork.back")}</Button>
    {!own || own.loading ? <div className="space-y-md px-md py-sm" aria-busy="true" data-record-loading="true">{[0, 1, 2].map((key) => <div key={key} className="h-(--size-control-lg) rounded-sm bg-muted" />)}</div> : own.failure ? <div role="status" className="px-md py-sm text-caption"><p>{t("sessionWork.failed")}</p><Button size="sm" variant="ghost" onClick={() => actions.openSessionRecord(target)}>{t("common.retry")}</Button></div> : record ? <div className={cn("min-h-0 flex-1 overflow-y-auto px-md pb-md text-caption [word-break:keep-all]", !connected && "opacity-(--opacity-disabled)")} data-record-content="true">
      <h2 className="flex min-w-0 items-center gap-xs py-sm text-subhead font-semibold"><AgentMark kind={target.agent} /><span className="min-w-0 truncate" title={recordTitle(record, summaries)}>{recordTitle(record, summaries)}</span></h2>
      <p className="mb-sm text-micro text-muted-foreground">{[record.state ? statusText(t, record.state as AgentStatusCode) : null, `${t("sessionWork.children")} ${records.length - 1}`, !summaries ? `${t("sessionWork.requests")} ${record.requests.length}` : null].filter(Boolean).join(" · ")}</p>
      <Outputs outputs={record.outputs} target={target} sourceNames={sourceNames} actions={actions} />
      {summaries
        ? <Work record={record} descendants={descendants} words={words} actions={actions} head />
        : <Requests record={record} />}
      {record.children.length ? <><SectionHead className="mt-md">{t("sessionWork.children")} <Count>{records.length - 1}</Count></SectionHead><Children record={record} summaries={summaries} words={words} actions={actions} /></> : null}
    </div> : null}
  </div>;
}

function SectionHead({ children, className, aside }: { children: React.ReactNode; className?: string; aside?: React.ReactNode }) {
  return <h3 className={cn("mb-xs flex min-h-(--size-control-sm) items-center gap-xs font-medium text-subtle-foreground", className)}><span className="flex min-w-0 items-baseline gap-xs">{children}</span>{aside ? <span className="ml-auto flex items-center">{aside}</span> : null}</h3>;
}

function Count({ children }: { children: React.ReactNode }) {
  return <span className="text-micro tabular-nums text-muted-foreground">{children}</span>;
}

/** This session's and every descendant's pull requests, worst first, then its issues; nothing at all without them. */
function Outputs({ outputs, target, sourceNames, actions }: { outputs: readonly WorkOutput[]; target: WorkSession; sourceNames: ReadonlyMap<string, string>; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const prs = orderedOutputs(outputs.filter((output) => output.kind === "pr"));
  const issues = orderedOutputs(outputs.filter((output) => output.kind === "issue"));
  if (!prs.length && !issues.length) return null;
  return <section data-record-outputs="true">
    {prs.length ? <SectionHead>{t("sessionWork.prs")} <Count>{prs.length}</Count></SectionHead> : null}
    {[...prs, ...issues].map((output) => <OutputRow key={`${workKey(output.source)}:${output.kind}:${output.repository}:${output.number}`} output={output} sourceName={workKey(output.source) === workKey(target) ? null : sourceNames.get(workKey(output.source)) ?? output.source_name} actions={actions} />)}
  </section>;
}

function staleHint(output: WorkOutput, unavailable: string): string | null {
  return output.stale ? output.last_read_at_unix_ms ? new Date(output.last_read_at_unix_ms).toLocaleString() : unavailable : null;
}

function OutputRow({ output, sourceName, actions }: { output: WorkOutput; sourceName: string | null; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const state = outputPrState(output.state);
  const word = output.kind === "pr" ? t(PR_LOOK[state].word) : t(output.state === "closed" ? "sessionWork.closed" : "sessionWork.open");
  const stale = staleHint(output, t("common.unavailable"));
  return <button type="button" className={cn(ROW, "flex min-h-(--size-control-sm) w-full items-center gap-xs py-xxs hover:bg-accent")} data-record-output={output.kind} aria-label={[`#${output.number} ${word}`, output.title, sourceName].filter(Boolean).join(" · ")} onClick={() => actions.openRecordedOutput(output)}>
    <Hint label={[word, stale].filter(Boolean).join(" · ")}>
      {output.kind === "pr" ? <PrMark state={state} number={output.number} stale={output.stale} /> : <span className={cn("inline-flex shrink-0 items-center gap-xxs font-mono text-caption", output.state === "closed" ? "text-muted-foreground" : "text-success", output.stale && "opacity-(--opacity-dimmed)")}><CircleDotIcon aria-hidden="true" className="size-(--size-pr-icon)" /><span className="text-subtle-foreground">#{output.number}</span></span>}
    </Hint>
    <span className="min-w-0 flex-1 truncate" title={output.title}>{output.title}</span>
    {sourceName ? <Hint label={sourceName}><span data-output-source="true" className="max-w-1/3 truncate text-micro text-muted-foreground">{sourceName}</span></Hint> : null}
  </button>;
}

/** The assigned work: a checklist with the words, the missed work and the newest unjudged request in their places. */
function Work({ record, descendants = [], words, actions, head = false }: { record: RecordValue; descendants?: readonly RecordValue[]; words: boolean; actions: Actions; head?: boolean }) {
  const { t } = useInterfaceTranslation();
  // The record at the top lists every descendant's missed work, named, at its end; a child's own checklist leaves it there instead of drawing it twice.
  const list = useMemo(() => checklist(head ? record : { ...record, missed: [] }, descendants), [head, record, descendants]);
  const received = record.requests.some((request) => request.author === "parent");
  const label = t(received ? "sessionWork.received" : "sessionWork.words");
  const nothing = !list.groups.length && !list.otherWords.length && !list.otherMissed.length && !list.now;
  return <section className={head ? "mt-md" : "mt-xs"} data-record-work="true">
    {head ? <SectionHead aside={<WordsButton label={label} />}>{t("sessionWork.tasks")} {list.total ? <Count>{list.done}/{list.total}</Count> : null}</SectionHead> : null}
    {nothing ? <p className="text-micro text-muted-foreground">{t("sessionWork.empty")}</p> : null}
    {list.groups.map((group) => <div key={group.id ?? "outside"}>
      {group.goal ? <GoalLine goal={group.goal} at={group.at} /> : null}
      {group.tasks.map((item) => <TaskRow key={item.task.id} item={item} session={record.session} words={words} actions={actions} />)}
    </div>)}
    {list.otherMissed.length || (words && list.otherWords.length) || list.now ? <OtherWork list={list} words={words} actions={actions} /> : null}
  </section>;
}

function WordsButton({ label }: { label: string }) {
  const on = useUiStore((state) => state.sessionRecordWords);
  const set = useUiStore((state) => state.setSessionRecordWords);
  return <Hint label={label}><button type="button" aria-pressed={on} aria-label={label} data-record-words={on ? "on" : "off"} className={cn(ROW, "inline-flex size-(--size-control-sm) items-center justify-center", on ? "bg-secondary text-foreground" : "text-muted-foreground hover:bg-accent")} onClick={() => set(!on)}><MessageCircleIcon aria-hidden="true" className="size-(--size-icon-sm)" /></button></Hint>;
}

function GoalLine({ goal, at }: { goal: string; at: number }) {
  const { i18n } = useInterfaceTranslation();
  const time = formatDateTime(requireInterfaceLanguage(i18n.language), at, { hour: "2-digit", minute: "2-digit", hour12: false });
  return <h4 className="mt-sm mb-xs min-w-0 whitespace-normal text-micro font-medium text-subtle-foreground [overflow-wrap:anywhere]" title={goal}>{goal} · <time dateTime={new Date(at).toISOString()}>{time}</time></h4>;
}

function TaskMark({ state }: { state: TaskState }) {
  const { t } = useInterfaceTranslation();
  const name = t(`sessionWork.${state}`);
  return <Hint label={name}><span role="img" aria-label={name} data-task-state={state} className={cn("flex size-(--size-icon-sm) shrink-0 items-center justify-center", GLYPH[state])}>
    {state === "done" ? <CheckIcon aria-hidden="true" className="size-(--size-icon-sm)" /> : state === "blocked" ? <span aria-hidden="true">⛔</span> : state === "question" ? <span aria-hidden="true" className="font-semibold">?</span> : <span aria-hidden="true">◐</span>}
  </span></Hint>;
}

function TaskRow({ item, session, words, actions }: { item: ChecklistTask; session: WorkSession; words: boolean; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const { task, state } = item;
  return <div className="py-xxs" data-record-task={task.id}>
    <div className="flex min-w-0 items-start gap-xs">
      <TaskMark state={state} />
      <span className={cn("min-w-0 flex-1 whitespace-normal [overflow-wrap:anywhere]", state === "done" ? "text-subtle-foreground" : "font-medium")}>{task.name}</span>
      {item.prs.map((output) => <button key={output.url} type="button" data-record-task-pr={output.number} className={cn(ROW, "shrink-0 px-xxs hover:bg-accent")} aria-label={[`#${output.number} ${t(PR_LOOK[outputPrState(output.state)].word)}`, output.title, staleHint(output, t("common.unavailable"))].filter(Boolean).join(" · ")} title={output.title} onClick={() => actions.openRecordedOutput(output)}><PrMark state={outputPrState(output.state)} number={output.number} stale={output.stale} /></button>)}
    </div>
    {words ? item.words.map((request) => <Bubble key={request.offset} request={request} />) : null}
    {item.missed.map((missed) => <MissedLine key={missed.id} missed={missed} session={session} owner={null} actions={actions} />)}
  </div>;
}

/** What is not one task's: the newest request not judged yet, the operator's other words, and the missed work with no task. */
function OtherWork({ list, words, actions }: { list: Checklist; words: boolean; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  return <div data-record-other="true">
    {list.now ? <div className="flex min-w-0 items-start gap-xs py-xs" data-record-now={list.now.offset}><LoaderCircleIcon aria-hidden="true" className="mt-xxs size-(--size-icon-sm) shrink-0 animate-spin text-agent-working" /><span className="shrink-0 font-medium text-agent-working">{t("sessionWork.now")}</span><span className="min-w-0 flex-1 whitespace-normal [overflow-wrap:anywhere]" title={list.now.text}>{list.now.text.split(/\r?\n/)[0]}</span></div> : null}
    {words && list.otherWords.length ? <div className="mt-xs"><h4 className="mb-xxs text-micro font-medium text-muted-foreground">{t("sessionWork.otherWords")}</h4>{list.otherWords.map((request) => <Bubble key={request.offset} request={request} quiet />)}</div> : null}
    {list.otherMissed.map(({ owner, missed, named }) => <MissedLine key={`${workKey(owner.session)}:${missed.id}`} missed={missed} session={owner.session} owner={named ? owner.title : null} actions={actions} />)}
  </div>;
}

/** A request's words in a bubble: two lines until pressed, then all of it and `접기`. The operator's carry a quiet person mark; a parent's do not. */
function Bubble({ request, quiet = false }: { request: WorkRequest; quiet?: boolean }) {
  const { t } = useInterfaceTranslation();
  const [open, setOpen] = useState(false);
  const [clipped, setClipped] = useState(false);
  const text = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    const node = text.current;
    if (!node) return;
    // A clamped box reports its clamped height as its scroll height, so the whole text's height is read with the clamp lifted.
    const measure = () => {
      if (open) return;
      const clamped = node.getBoundingClientRect().height;
      node.style.setProperty("-webkit-line-clamp", "unset");
      node.style.setProperty("display", "block");
      const whole = node.getBoundingClientRect().height;
      node.style.removeProperty("-webkit-line-clamp");
      node.style.removeProperty("display");
      setClipped(whole > clamped + 1);
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  }, [open, request.text]);
  const body = <><span ref={text} className={cn("block whitespace-normal [overflow-wrap:anywhere]", !open && "line-clamp-2", quiet ? "text-subtle-foreground" : "text-foreground")}>{request.text}</span>{open ? <span className="block text-micro text-muted-foreground">{t("sessionWork.collapse")}</span> : null}</>;
  // One element throughout, so the text node the measure reads never remounts; it answers to a press only while there is more to show.
  const interactive = clipped || open;
  const toggle = () => setOpen(!open);
  return <div className={cn("flex min-w-0 items-start gap-xs pt-xxs", LANE)} data-record-words-bubble={request.offset}>
    {request.author === "operator" ? <UserIcon aria-hidden="true" className="mt-xs size-(--size-icon-sm) shrink-0 text-muted-foreground" /> : <span aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />}
    <div role={interactive ? "button" : undefined} tabIndex={interactive ? 0 : undefined} aria-expanded={interactive ? open : undefined}
      className={cn("min-w-0 flex-1 rounded-sm bg-secondary px-sm py-xs text-left outline-none", interactive && "cursor-pointer focus-visible:ring-1 focus-visible:ring-ring")}
      onClick={interactive ? toggle : undefined} onKeyDown={interactive ? (event) => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); toggle(); } } : undefined}>{body}</div>
  </div>;
}

/**
 * Missed work. An open one is a warning line with its two actions; a closed
 * one is a muted line that says how it closed, and the action that reverses
 * it shows while the pointer or focus is on the line (an issue is its number).
 * `owner` names a descendant's session when the line is not its own.
 */
function MissedLine({ missed, session, owner, actions }: { missed: MissedWork; session: WorkSession; owner: string | null; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const rest = useShellStore((state) => state.rest);
  const connected = deviceConnected(rest, session.device_id);
  const draft = rest?.issue_work?.draft;
  const ownDraft = draft && workKey(draft.source.session) === workKey(session) && draft.source.id === missed.id ? draft : null;
  const drafting = ownDraft?.phase === "working";
  const busy = missed.creating || draft?.phase === "working";
  const open = missed.status === "open";
  const closedWord = missed.status === "resolved" ? t("sessionWork.resolved") : missed.status === "dismissed" ? t("sessionWork.dismissed") : null;
  return <div className={cn("group/missed min-w-0 pt-xxs", LANE)} data-missed-work={missed.id} data-missed-status={missed.status}>
    <p className="flex min-w-0 items-start gap-xs">
      <FlagIcon aria-hidden="true" className={cn("mt-xxs size-(--size-icon-sm) shrink-0", open ? "text-warning" : "text-muted-foreground")} />
      <span className={cn("min-w-0 flex-1 whitespace-normal [overflow-wrap:anywhere]", open ? "text-warning" : "text-muted-foreground")} title={missed.text}>{missed.text}{owner ? <span className="text-muted-foreground"> · {owner}</span> : null}{closedWord ? <span> · {closedWord}</span> : null}</span>
      {missed.status === "issue" && missed.issue_number && missed.issue_repository ? <Button size="sm" variant="link" className="h-auto shrink-0 p-0" onClick={() => actions.openRecordedIssue(session, missed.issue_repository!, missed.issue_number!)}>#{missed.issue_number}</Button> : null}
      {closedWord ? <Button size="sm" variant="ghost" disabled={busy} className="h-auto shrink-0 px-xs py-0 text-micro opacity-0 group-hover/missed:opacity-100 group-focus-within/missed:opacity-100 focus-visible:opacity-100 hoverless:opacity-100" onClick={() => actions.setMissedStatus(session, missed.id, "open")}>{t(missed.status === "resolved" ? "sessionWork.reopen" : "sessionWork.undo")}</Button> : null}
    </p>
    {open ? <div className={cn("mt-xs flex gap-xs", LANE)}>
      <Hint label={!connected ? t("devices.rail.notConnected") : t("sessionWork.issue")}><span><Button size="sm" variant="outline" disabled={!connected || busy} data-missed-issue={missed.id} onClick={() => actions.draftMissedIssue(session, missed.id)}>{t(drafting ? "sessionWork.drafting" : "sessionWork.issue")}</Button></span></Hint>
      <Button size="sm" variant="ghost" disabled={busy} onClick={() => actions.setMissedStatus(session, missed.id, "dismissed")}>{t("sessionWork.dismiss")}</Button>
      {drafting ? <Button size="sm" variant="ghost" onClick={() => actions.cancelMissedIssueDraft()}>{t("common.cancel")}</Button> : null}
    </div> : null}
    {ownDraft?.phase === "failed" ? <p role="status" className="mt-xxs text-micro text-destructive">{t("sessionWork.issue_draft_unavailable")}</p> : null}
  </div>;
}

/** The Sessions panel's group: an open missed item with its session's name and kind, where it was found. */
export function MissedRow({ missed, record, actions }: { missed: MissedWork; record: Pick<RecordValue, "session" | "title">; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  return <li className="min-w-0 py-xs">
    <MissedLine missed={missed} session={record.session} owner={null} actions={actions} />
    <button type="button" className={cn(ROW, "mt-xxs block w-full truncate text-micro text-muted-foreground", LANE)} title={record.title} onClick={() => actions.openSessionRecord(record.session)}>{record.title} · {t(`sessionWork.kind.${missed.kind}`)}</button>
  </li>;
}

/** Summaries off: the operator's requests as the session recorded them, oldest first with the newest few open. */
function Requests({ record }: { record: RecordValue }) {
  const { t } = useInterfaceTranslation();
  const [all, setAll] = useState(false);
  const requests = [...record.requests].sort((a, b) => a.at_unix_ms - b.at_unix_ms);
  const hidden = all ? 0 : Math.max(0, requests.length - FIRST_OPEN_REQUESTS);
  return <section className="mt-md" data-record-work="true">
    <SectionHead>{t("sessionWork.requests")} <Count>{requests.length}</Count></SectionHead>
    {!requests.length ? <p className="text-micro text-muted-foreground">{t("sessionWork.empty")}</p> : null}
    {hidden ? <button type="button" className={cn(ROW, "flex items-center gap-xxs py-xxs text-micro text-muted-foreground hover:text-foreground")} aria-expanded="false" onClick={() => setAll(true)}><ChevronRightIcon aria-hidden="true" className="size-(--size-icon-sm)" />{t("sessionWork.earlier", { count: hidden })}</button> : null}
    {requests.slice(hidden).map((request) => <RequestRow key={request.offset} request={request} />)}
  </section>;
}

function RequestRow({ request }: { request: WorkRequest }) {
  const { i18n } = useInterfaceTranslation();
  const time = formatDateTime(requireInterfaceLanguage(i18n.language), request.at_unix_ms, { hour: "2-digit", minute: "2-digit", hour12: false });
  return <div className="flex min-w-0 items-start gap-sm py-xxs" data-record-request="true"><time className="shrink-0 font-mono text-micro text-muted-foreground" dateTime={new Date(request.at_unix_ms).toISOString()}>{time}</time><span className="min-w-0 whitespace-normal [overflow-wrap:anywhere]" title={request.text}>{request.text.split(/\r?\n/)[0]}</span></div>;
}

function Children({ record, summaries, words, actions, depth = 1 }: { record: RecordValue; summaries: boolean; words: boolean; actions: Actions; depth?: number }) {
  const { t } = useInterfaceTranslation();
  const [visible, setVisible] = useState(5);
  return <div className={cn("min-w-0", depth <= SESSION_INDENT_LEVELS && "border-l border-lineage-rail pl-sm")}>{record.children.slice(0, visible).map((child) => <details key={workKey(child.session)} data-record-child={child.session.session_id}><summary className={cn(ROW, "flex min-h-(--size-control-sm) cursor-pointer items-center gap-xs py-xs")}><ChevronRightIcon aria-hidden="true" className="size-(--size-icon-sm)" /><span className="min-w-0 flex-1 truncate" title={`${recordTitle(child, summaries)}\n${agentProductName(child.session.agent)}`}>{recordTitle(child, summaries)}<span className="sr-only">, {agentProductName(child.session.agent)}</span></span><span className="shrink-0 text-micro text-muted-foreground">{child.state ? statusText(t, child.state as AgentStatusCode) : null}</span></summary>{summaries ? <><div className="flex items-center gap-xs px-xs pt-xs"><span className="font-medium text-subtle-foreground">{t("sessionWork.tasks")}</span><Count>{child.tasks.filter((task) => task.done).length}/{child.tasks.length}</Count></div><Work record={child} words={words} actions={actions} /></> : <Requests record={child} />}{child.children.length ? <Children record={child} summaries={summaries} words={words} actions={actions} depth={depth + 1} /> : null}</details>)}{record.children.length > visible ? <Button size="sm" variant="ghost" onClick={() => setVisible((count) => count + 5)}>{t("sessionWork.moreChildren", { count: record.children.length - visible })}</Button> : null}</div>;
}
