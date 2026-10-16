import { ArrowLeftIcon, ChevronRightIcon, CircleDotIcon, FlagIcon, HouseIcon, LoaderCircleIcon, MessageCircleIcon, UserIcon } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { Actions } from "./actions";
import { AgentMark } from "./AgentMark";
import { agentProductName } from "./agentAdapters";
import { deviceConnected, readsHere } from "./devices";
import { Button } from "./components/ui/button";
import { PrMark } from "./components/pr-mark";
import { ChildProgressHead, TaskMark } from "./components/task-progress";
import { Hint } from "./components/ui/tooltip";
import { formatDateTime } from "./i18n/format";
import { requireInterfaceLanguage } from "./i18n/locale";
import { useInterfaceTranslation } from "./i18n/client";
import { cn } from "./lib/utils";
import { checklist, orderedOutputs, outputPrState, recordTitle, recordTree, sessionPlace, workKey, type Checklist, type ChecklistTask, type TaskState } from "./sessionWork";
import { boardProjects } from "./navigation";
import { cardLine, cardOfPane, cardQuestion, everyAgent, itemToken } from "./overviewTasks";
import { destinationOf } from "./overviewBacklog";
import { ageWords } from "./overviewLens";
import { machineName } from "./screenMachine";
import { catalogWorkspaces } from "./snapshot";
import type { AgentStatusCode, MissedWork, SessionRecord as RecordValue, WorkOutput, WorkRequest, WorkSession } from "./snapshot";
import { statusText } from "./agentStatus";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { summariesEnabled } from "./hideAi";
import { PR_LOOK } from "./prMark";

const ROW = "min-w-0 rounded-xs text-left outline-none focus-visible:ring-1 focus-visible:ring-ring";
/** Under a task's line: the icon column's width and its gap, so what hangs from a task lines up with its name. */
const LANE = "pl-[calc(var(--size-icon-sm)+var(--spacing-xs))]";
const FIRST_OPEN_REQUESTS = 5;

/** `작업 → PR → 검토 → 머지` filled up to the task's stage (PRD overview-device-tasks B9). */
function StageBar({ stage, tone }: { stage: 1 | 2 | 3 | 4; tone: string }) {
  const { t } = useInterfaceTranslation();
  return (
    <span role="img" aria-label={t("overviewTasks.stage", { stage })} className="flex w-(--size-control-lg) shrink-0 gap-px" data-stage-bar={stage}>
      {[1, 2, 3, 4].map((step) => <span key={step} className={cn("h-(--size-hairline) flex-1 rounded-full", step <= stage ? tone : "bg-muted")} />)}
    </span>
  );
}

/**
 * Where a record is drawn (PRD overview-device-tasks D-07, B17-B21): the
 * lane's 보기 slot, under the slot's head, or the Overview's panel, whose head the
 * panel gives (✕ and the ways to the work). Both draw the same record in the
 * same order and the same loading, failed and disconnected states.
 */
export type RecordFrame = {
  kind: "lane" | "overview";
  /** The Overview panel's head; the 보기 slot draws its own. */
  head?: ReactNode;
  /** A line above the record (the dashed card's B14 note). */
  note?: string | null;
  /** The children drilled into, outermost first (B20). */
  trail: readonly WorkSession[];
  onTrail: (trail: readonly WorkSession[]) => void;
  /** Reads the record again: a retry, or the device answering again. */
  onRead: () => void;
  /** The record leaves the screen. */
  onLeave: () => void;
};

/**
 * The 보기 slot's record (PRD sidebar-task-lens D-44, B16): the slot's head
 * is the panel's, a child opens as the slot's next item so ← comes back to
 * its parent, and the core reads what the slot shows.
 */
export function SlotSessionRecord({ actions, target }: { actions: Actions; target: WorkSession }) {
  const frame = useMemo<RecordFrame>(() => ({
    kind: "lane",
    trail: [],
    onTrail: (next) => {
      const child = next.at(-1);
      if (child) actions.openChildRecord(child);
    },
    onRead: () => actions.openChildRecord(target),
    // The core starts and stops the slot's read with the slot itself.
    onLeave: () => undefined,
  }), [actions, target]);
  return <SessionRecord actions={actions} target={target} frame={frame} />;
}

export function SessionRecord({ actions, target, frame }: { actions: Actions; target: WorkSession; frame: RecordFrame }) {
  const { t } = useInterfaceTranslation();
  const section = useShellStore((state) => state.sessionRecord);
  const rest = useShellStore((state) => state.rest);
  const words = useUiStore((state) => state.sessionRecordWords);
  const summaries = summariesEnabled(rest?.status?.background_ai);
  const connected = deviceConnected(rest, target.device_id);
  const own = section?.session && workKey(section.session) === workKey(target) ? section : null;
  const records = useMemo(() => own?.record ? recordTree(own.record) : [], [own?.record]);
  const sourceNames = useMemo(() => new Map(records.map((record) => [workKey(record.session), recordTitle(record, summaries)])), [records, summaries]);
  // The drilled child, when it is still in the tree; a vanished one leaves its parent shown.
  const shown = useMemo(() => {
    for (let depth = frame.trail.length - 1; depth >= 0; depth -= 1) {
      const found = records.find((record) => workKey(record.session) === workKey(frame.trail[depth]!));
      if (found) return found;
    }
    return own?.record ?? null;
  }, [frame.trail, records, own?.record]);
  const parent = frame.trail.length > 0 ? (records.find((record) => record.children.some((child) => shown && workKey(child.session) === workKey(shown.session))) ?? null) : null;
  const descendants = useMemo(() => (shown ? recordTree(shown).slice(1) : []), [shown]);
  const wasConnected = useRef(connected);
  const asked = useRef(false);
  const leave = useRef(frame.onLeave);
  leave.current = frame.onLeave;
  useEffect(() => () => leave.current(), []);
  // Read again on a reconnect, and once when the slot draws a record with no
  // read behind it: one restored after a relaunch was opened by no event. The
  // Overview's panel starts its own read when it opens.
  useEffect(() => {
    const unread = frame.kind === "lane" && !own && !asked.current;
    if (connected && (!wasConnected.current || unread)) {
      asked.current = true;
      frame.onRead();
    }
    wasConnected.current = connected;
  }, [frame, connected, own]);
  const up = () => frame.onTrail(frame.trail.slice(0, -1));
  return <div className="flex min-h-0 min-w-0 flex-1 flex-col" data-session-record={frame.kind}>
    {frame.kind === "lane" ? null : <>
      {frame.head}
      {parent ? <Button size="sm" variant="ghost" className="min-w-0 self-start px-md" onClick={up} data-record-up="true"><ArrowLeftIcon /><span className="truncate">{t("record.backTo", { parent: recordTitle(parent, summaries) })}</span></Button> : null}
    </>}
    {frame.note ? <p className="px-md pb-xs text-caption text-muted-foreground" data-record-note="true">{frame.note}</p> : null}
    {!own || own.loading ? <div className="space-y-md px-md py-sm" aria-busy="true" data-record-loading="true">{[0, 1, 2].map((key) => <div key={key} className="h-(--size-control-lg) rounded-sm bg-muted" />)}</div> : own.failure ? <div role="status" className="px-md py-sm text-caption"><p>{t("sessionWork.failed")}</p><Button size="sm" variant="ghost" onClick={frame.onRead}>{t("common.retry")}</Button></div> : shown ? <div className={cn("min-h-0 flex-1 overflow-y-auto px-md pb-md text-caption [word-break:keep-all]", !connected && "opacity-(--opacity-disabled)")} data-record-content="true">
      <h2 className="flex min-w-0 items-center gap-xs py-sm text-subhead font-semibold"><AgentMark kind={shown.session.agent} /><span className="min-w-0 truncate" title={recordTitle(shown, summaries)}>{recordTitle(shown, summaries)}</span></h2>
      <RecordPlace record={shown} children={recordTree(shown).length - 1} />
      <Outputs outputs={shown.outputs} target={shown.session} sourceNames={sourceNames} actions={actions} />
      {summaries
        ? <Work record={shown} descendants={descendants} words={words} actions={actions} head />
        : <Requests record={shown} />}
      {shown.children.length ? <><SectionHead className="mt-md">{summaries && shown.child_progress ? <ChildProgressHead progress={shown.child_progress} /> : <>{t("sessionWork.children")} <Count>{recordTree(shown).length - 1}</Count></>}</SectionHead><Children record={shown} summaries={summaries} onOpen={(child) => frame.onTrail([...frame.trail, child.session])} /></> : null}
      <RecordWorktrees record={shown} />
    </div> : null}
  </div>;
}

/**
 * The record's place line, then what to do now or the stage bar (B17): device
 * · project · ⌂ checkout · state and age · children. The task's own facts (its
 * question, its block, a PR to merge, its stage) are the root's, so a child's
 * record shows the place line alone.
 */
function RecordPlace({ record, children }: { record: RecordValue; children: number }) {
  const { t, i18n } = useInterfaceTranslation();
  const rest = useShellStore((state) => state.rest);
  const agents = useShellStore((state) => state.agents);
  const projects = useMemo(() => boardProjects(rest, agents), [rest, agents]);
  const rows = useMemo(() => everyAgent(rest, agents), [rest, agents]);
  const place = useMemo(() => sessionPlace(projects, record.session, record.cwd, rest), [projects, record.session, record.cwd, rest]);
  const card = place?.agent ? cardOfPane(projects, place.agent.pane_id) : null;
  const root = card && place?.agent?.pane_id === card.card.root ? card.card : null;
  const device = machineName(rest?.navigator?.devices, record.session.device_id, t);
  const language = requireInterfaceLanguage(i18n.language);
  const changed = record.ended_at_unix_ms ?? place?.agent?.changed_at_unix_ms ?? null;
  const state = [record.state ? statusText(t, record.state as AgentStatusCode) : null, changed !== null ? ageWords(language, Date.now() - changed, t) : null].filter(Boolean).join(" ");
  const line = root ? cardLine(root, rows) : null;
  const question = root ? cardQuestion(root, rows) : null;
  const merge = line?.kind === "pr" && line.state === "mergeable" ? line.number : null;
  const now = line?.kind === "needs_you" || line?.kind === "blocked" || merge !== null;
  return <>
    <p className="mb-sm flex min-w-0 flex-wrap items-center gap-x-xs text-micro text-muted-foreground" data-record-place="true">
      {[device, place?.project.label ?? null].filter(Boolean).join(" · ")}
      {place?.checkout ? <span className="inline-flex min-w-0 items-center gap-xxs">· <HouseIcon aria-hidden="true" className="size-(--size-icon-sm)" /><span className="truncate">{place.checkout.label}</span></span> : null}
      {state ? <span>· {state}</span> : null}
      <span>· {t("overviewTasks.children", { count: children })}</span>
    </p>
    {root && now ? (
      <section className="mb-sm" data-record-now="true">
        <SectionHead>{t("record.now")}</SectionHead>
        {line?.kind === "needs_you" ? <p className="whitespace-normal text-warning [overflow-wrap:anywhere]">{question ?? t("overviewTasks.state.needs_you")}</p> : null}
        {line?.kind === "blocked" ? <p className="whitespace-normal text-destructive [overflow-wrap:anywhere]">{line.what ?? t("overviewTasks.state.blocked")}</p> : null}
        {merge !== null ? <p className="text-success">{t("record.mergeTurn", { number: merge })}</p> : null}
      </section>
    ) : root?.stage && line ? <span className="mb-sm flex"><StageBar stage={root.stage} tone={line.kind === "merged" ? "bg-pr-merged" : line.kind === "done" ? "bg-success" : line.kind === "resting" ? "bg-muted-foreground" : "bg-agent-working"} /></span> : null}
  </>;
}

/** The task's worktrees (B17): the root's card's checkouts, else the record's own. */
function RecordWorktrees({ record }: { record: RecordValue }) {
  const { t } = useInterfaceTranslation();
  const rest = useShellStore((state) => state.rest);
  const agents = useShellStore((state) => state.agents);
  const projects = useMemo(() => boardProjects(rest, agents), [rest, agents]);
  const place = useMemo(() => sessionPlace(projects, record.session, record.cwd, rest), [projects, record.session, record.cwd, rest]);
  const card = place?.agent ? cardOfPane(projects, place.agent.pane_id) : null;
  const checkouts = card && place?.agent?.pane_id === card.card.root
    ? card.card.checkouts.map((id) => card.project.checkouts.find((row) => row.id === id)).filter((row): row is NonNullable<typeof row> => row !== undefined)
    : place?.checkout ? [place.checkout] : [];
  if (checkouts.length === 0) return null;
  return <section className="mt-md" data-record-worktrees="true">
    <SectionHead>{t("record.worktrees")} <Count>{checkouts.length}</Count></SectionHead>
    {checkouts.map((checkout) => <p key={checkout.id} className={cn("flex min-w-0 items-center gap-xs py-xxs font-mono", !checkout.exists && "text-muted-foreground")} data-record-worktree={checkout.id}>
      <HouseIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
      <span className="truncate">{checkout.exists ? checkout.label : t("record.worktreeGone", { label: checkout.label })}</span>
    </p>)}
  </section>;
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
const NO_RECORDS: readonly RecordValue[] = [];
function Work({ record, descendants = NO_RECORDS, words, actions, head = false }: { record: RecordValue; descendants?: readonly RecordValue[]; words: boolean; actions: Actions; head?: boolean }) {
  const { t } = useInterfaceTranslation();
  // The record at the top lists every descendant's missed work, named, at its end; a child's own checklist leaves it there instead of drawing it twice.
  const list = useMemo(() => checklist(head ? record : { ...record, missed: [] }, descendants), [head, record, descendants]);
  const received = record.requests.some((request) => request.author === "parent");
  const label = t(received ? "sessionWork.received" : "sessionWork.words");
  const nothing = !list.groups.length && !(words && list.otherWords.length) && !list.otherMissed.length && !list.now;
  return <section className={head ? "mt-md" : "mt-xs"} data-record-work="true">
    {head ? <SectionHead aside={<WordsButton label={label} />}>{t("sessionWork.tasks")} {list.total ? <Count>{list.done}/{list.total}</Count> : null}</SectionHead> : list.total ? <div className="flex items-center gap-xs px-xs pt-xs"><span className="font-medium text-subtle-foreground">{t("sessionWork.tasks")}</span><Count>{list.done}/{list.total}</Count></div> : null}
    {nothing ? <p className="text-micro text-muted-foreground">{t("sessionWork.empty")}</p> : null}
    {list.groups.map((group) => <div key={group.id ?? "outside"}>
      {group.goal ? <GoalLine goal={group.goal} at={group.at} /> : null}
      {group.tasks.map((item) => <TaskRow key={item.task.id} item={item} session={record.session} cwd={record.cwd} words={words} actions={actions} />)}
    </div>)}
    {list.otherMissed.length || (words && list.otherWords.length) || list.now ? <OtherWork list={list} words={words} actions={actions} /> : null}
    {head && !list.now ? <LastWords session={record.session} /> : null}
  </section>;
}

/** When the session is not working, the last thing its agent said (B17): the newest line of what it last reported, while its pane is still known. */
function LastWords({ session }: { session: WorkSession }) {
  const { t } = useInterfaceTranslation();
  const said = useShellStore((state) => {
    const row = [...everyAgent(state.rest, state.agents).values()].find((agent) => agent.session_id === session.session_id);
    return row?.message?.trim().split(/\r?\n/).filter((line) => line.trim()).at(-1) ?? null;
  });
  if (!said) return null;
  return <p className="flex min-w-0 items-start gap-xs py-xs text-subtle-foreground" data-record-last-words="true">
    <span className="shrink-0 font-medium text-muted-foreground">{t("sessionWork.lastWords")}</span>
    <span className="min-w-0 flex-1 whitespace-normal [overflow-wrap:anywhere]" title={said}>{said}</span>
  </p>;
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

function TaskRow({ item, session, cwd, words, actions }: { item: ChecklistTask; session: WorkSession; cwd: string | null; words: boolean; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const { task, state } = item;
  return <div className="py-xxs" data-record-task={task.id}>
    <div className="flex min-w-0 items-start gap-xs">
      <TaskMark state={state} />
      <span className={cn("min-w-0 flex-1 whitespace-normal [overflow-wrap:anywhere]", state === "done" ? "text-subtle-foreground" : "font-medium")}>{task.name}</span>
      {item.prs.map((output) => <button key={output.url} type="button" data-record-task-pr={output.number} className={cn(ROW, "shrink-0 px-xxs hover:bg-accent")} aria-label={[`#${output.number} ${t(PR_LOOK[outputPrState(output.state)].word)}`, output.title, staleHint(output, t("common.unavailable"))].filter(Boolean).join(" · ")} title={output.title} onClick={() => actions.openRecordedOutput(output)}><PrMark state={outputPrState(output.state)} number={output.number} stale={output.stale} /></button>)}
    </div>
    {words ? item.words.map((request) => <Bubble key={request.offset} request={request} />) : null}
    {item.missed.map((missed) => <MissedLine key={missed.id} missed={missed} session={session} cwd={cwd} owner={null} actions={actions} />)}
  </div>;
}

/** What is not one task's: the newest request not judged yet, the operator's other words, and the missed work with no task. */
function OtherWork({ list, words, actions }: { list: Checklist; words: boolean; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  return <div data-record-other="true">
    {list.now ? <div className="flex min-w-0 items-start gap-xs py-xs" data-record-now={list.now.offset}><LoaderCircleIcon aria-hidden="true" className="mt-xxs size-(--size-icon-sm) shrink-0 animate-spin text-agent-working" /><span className="shrink-0 font-medium text-agent-working">{t("sessionWork.now")}</span><span className="min-w-0 flex-1 whitespace-normal [overflow-wrap:anywhere]" title={list.now.text}>{list.now.text.split(/\r?\n/)[0]}</span></div> : null}
    {words && list.otherWords.length ? <div className="mt-xs"><h4 className="mb-xxs text-micro font-medium text-muted-foreground">{t("sessionWork.otherWords")}</h4>{list.otherWords.map((request) => <Bubble key={request.offset} request={request} quiet />)}</div> : null}
    {list.otherMissed.map(({ owner, missed, named }) => <MissedLine key={`${workKey(owner.session)}:${missed.id}`} missed={missed} session={owner.session} cwd={owner.cwd} owner={named ? owner.title : null} actions={actions} />)}
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
 * Missed work (PRD overview-device-tasks B43-B46). An open one is a warning
 * line with `백로그로 · 무시`; `백로그로` makes the item at once in the project
 * the session works in, shows `만드는 중` meanwhile, and the line then dims
 * with `· 백로그 #N` that opens the item. A failure keeps the line with its
 * reason and a retry that converges on one item. A closed line says how it
 * closed, and the action that reverses it shows while the pointer or focus is
 * on it. `owner` names a descendant's session when the line is not its own.
 */
function MissedLine({ missed, session, cwd, owner, actions }: { missed: MissedWork; session: WorkSession; cwd: string | null; owner: string | null; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const rest = useShellStore((state) => state.rest);
  const agents = useShellStore((state) => state.agents);
  const connected = deviceConnected(rest, session.device_id);
  const project = useMemo(() => {
    const place = sessionPlace(boardProjects(rest, agents, session.device_id), session, cwd, rest);
    // A device the core dials keeps no backlog (B46).
    return place && readsHere(place.project, rest?.navigator?.devices) && place.project.tasks?.source ? place.project : null;
  }, [rest, agents, session, cwd]);
  const item = useMemo(() => [...(rest?.issue_work?.backlog ?? [])].reverse().find((row) => row.missed && workKey(row.missed.session) === workKey(session) && row.missed.id === missed.id) ?? null, [rest?.issue_work?.backlog, session, missed.id]);
  const making = missed.creating || item?.phase === "waiting" || item?.phase === "creating";
  const open = missed.status === "open";
  const failed = open && item?.phase === "failed" ? item : null;
  const destination = destinationOf(project?.tasks?.source ?? null);
  const backlogKey = missed.status === "backlog" ? missed.backlog_key : null;
  const backlogProject = backlogKey ? project ?? catalogWorkspaces(rest).find((row) => row.device_id === session.device_id && (row.tasks?.tasks ?? []).some((task) => task.key === backlogKey)) ?? null : null;
  const token = backlogKey ? (backlogProject ? itemToken(backlogKey, backlogProject) : null) ?? backlogKey : null;
  const closedWord = missed.status === "resolved" ? t("sessionWork.resolved") : missed.status === "dismissed" ? t("sessionWork.dismissed") : null;
  const toBacklog = () => {
    if (project) actions.addBacklog(project.id, missed.text, { session, id: missed.id });
  };
  return <div className={cn("group/missed min-w-0 pt-xxs", LANE)} data-missed-work={missed.id} data-missed-status={missed.status}>
    <p className="flex min-w-0 items-start gap-xs">
      <FlagIcon aria-hidden="true" className={cn("mt-xxs size-(--size-icon-sm) shrink-0", open ? "text-warning" : "text-muted-foreground")} />
      <span className={cn("min-w-0 flex-1 whitespace-normal [overflow-wrap:anywhere]", open ? "text-warning" : "text-muted-foreground")} title={missed.text}>{missed.text}{owner ? <span className="text-muted-foreground"> · {owner}</span> : null}{closedWord ? <span> · {closedWord}</span> : null}</span>
      {making ? <span className="flex shrink-0 items-center gap-xxs text-micro text-muted-foreground" data-missed-making="true"><LoaderCircleIcon aria-hidden="true" className="size-(--size-icon-sm) animate-spin" />{t("backlog.creating")}</span> : null}
      {backlogKey && token ? <Button size="sm" variant="link" className="h-auto shrink-0 p-0 text-micro" data-missed-backlog={token} onClick={() => backlogProject ? actions.openBacklogItem(backlogProject.device_id, backlogProject.id, backlogKey) : undefined}>{t("missed.inBacklog", { token })}</Button> : null}
      {missed.status === "issue" && missed.issue_number && missed.issue_repository ? <Button size="sm" variant="link" className="h-auto shrink-0 p-0" onClick={() => actions.openRecordedIssue(session, missed.issue_repository!, missed.issue_number!)}>#{missed.issue_number}</Button> : null}
      {closedWord ? <Button size="sm" variant="ghost" disabled={making} className="h-auto shrink-0 px-xs py-0 text-micro opacity-0 group-hover/missed:opacity-100 group-focus-within/missed:opacity-100 focus-visible:opacity-100 hoverless:opacity-100" onClick={() => actions.setMissedStatus(session, missed.id, "open")}>{t(missed.status === "resolved" ? "sessionWork.reopen" : "sessionWork.undo")}</Button> : null}
    </p>
    {failed ? <p role="status" className={cn("mt-xxs flex items-center gap-xs text-micro text-destructive", LANE)} data-missed-failed={failed.failure ?? "refused"}>
      <span className="min-w-0 flex-1">{t(`backlog.failure.${failed.failure ?? "refused"}`)}</span>
      {failed.retry ? <Button size="sm" variant="ghost" className="h-auto px-xs py-0 text-micro" disabled={!connected} onClick={() => actions.retryBacklog(failed.intent)}>{t("common.retry")}</Button>
        : project ? <Button size="sm" variant="ghost" className="h-auto px-xs py-0 text-micro" disabled={!connected} onClick={toBacklog} data-missed-backlog-add={missed.id}>{t("missed.toBacklog")}</Button> : null}
      <Button size="sm" variant="ghost" className="h-auto px-xs py-0 text-micro" onClick={() => actions.dismissBacklog(failed.intent)} data-missed-failed-dismiss={missed.id}>{t("sessionWork.dismiss")}</Button>
    </p> : null}
    {open && !making && !failed ? <div className={cn("mt-xs flex items-center gap-xs", LANE)}>
      {project ? <Hint label={!connected ? t("devices.rail.notConnected") : t(`quickAdd.dest.${destination}`)}><span><Button size="sm" variant="outline" disabled={!connected} data-missed-backlog-add={missed.id} onClick={toBacklog}>
        {t("missed.toBacklog")}
        {destination === "public" ? <span className="text-micro text-warning" data-missed-public="true">{t("missed.public")}</span> : destination === "unknown" ? <span className="text-micro text-warning" data-missed-public="unknown">{t("missed.maybePublic")}</span> : null}
      </Button></span></Hint> : null}
      <Button size="sm" variant="ghost" onClick={() => actions.setMissedStatus(session, missed.id, "dismissed")}>{t("sessionWork.dismiss")}</Button>
    </div> : null}
  </div>;
}

/** The Overview backlog's missed group (D-52): an open missed item with its session's name and kind, where it was found. */
export function MissedRow({ missed, record, actions }: { missed: MissedWork; record: Pick<RecordValue, "session" | "title" | "cwd">; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  return <li className="min-w-0 py-xs">
    <MissedLine missed={missed} session={record.session} cwd={record.cwd} owner={null} actions={actions} />
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

/** The record's children (B20): each opens in this place, its record in place of this one. */
function Children({ record, summaries, onOpen }: { record: RecordValue; summaries: boolean; onOpen: (child: RecordValue) => void }) {
  const { t } = useInterfaceTranslation();
  const [visible, setVisible] = useState(5);
  return <div className="min-w-0 border-l border-lineage-rail pl-sm">{record.children.slice(0, visible).map((child) => <button key={workKey(child.session)} type="button" data-record-child={child.session.session_id} className={cn(ROW, "flex min-h-(--size-control-sm) w-full items-center gap-xs py-xs hover:bg-accent")} title={`${recordTitle(child, summaries)}\n${agentProductName(child.session.agent)}`} onClick={() => onOpen(child)}><span className="min-w-0 flex-1 truncate">{recordTitle(child, summaries)}<span className="sr-only">, {agentProductName(child.session.agent)}</span></span>{child.children.length ? <span className="shrink-0 text-micro text-muted-foreground">{t("overviewTasks.children", { count: recordTree(child).length - 1 })}</span> : null}<span className="shrink-0 text-micro text-muted-foreground">{child.state ? statusText(t, child.state as AgentStatusCode) : null}</span><ChevronRightIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" /></button>)}{record.children.length > visible ? <Button size="sm" variant="ghost" onClick={() => setVisible((count) => count + 5)}>{t("sessionWork.moreChildren", { count: record.children.length - visible })}</Button> : null}</div>;
}
