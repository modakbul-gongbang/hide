import { useMemo, useState } from "react";
import { ChevronDownIcon, ChevronRightIcon, GitPullRequestIcon, LightbulbIcon, ListIcon, SparklesIcon, TagIcon } from "lucide-react";
import type { Actions } from "../actions";
import { Elapsed } from "../components/elapsed";
import { Button } from "../components/ui/button";
import { useInterfaceTranslation } from "../i18n/client";
import type { MessageKey } from "../i18n/catalogs";
import { cn } from "../lib/utils";
import { useShellStore } from "../store";
import { useUiStore } from "../ui";
import { Decisions, Refusal } from "./Decisions";
import { activityText, GATE_LABEL, PAUSE_REASON_LABEL, STATE_LABEL, STOP_LABEL, waitingText, type Translate } from "./labels";
import type { CardView, FactorySummary, FactoryView, FollowUpView, InboxItem } from "./model";
import { useFactoryRequest } from "./request";
import { lineRows, rowItem, shownFactories, shownInbox, type LineRow } from "./view";



/** The label a GitHub issue carries to become a Task; the engine creates it with this name. */
export const FACTORY_LABEL = "factory";

const STAGE_WORD: readonly MessageKey[] = ["factory.line.stage.intake", "factory.line.stage.work", "factory.line.stage.verify", "factory.line.stage.merge", "factory.line.stage.merged"];

/**
 * 라인 (PRD factory-human-loop D-09, D-34, B25, B26, B31, B32): the
 * Factory's first tab. 결정 필요 on top, then one row per Task with its
 * track, what it does now, how long and how many of its decisions Factory AI
 * made; the person's rows alone carry the warning band. The follow-up
 * candidates and the Factory's own activity wait folded at the foot.
 */
export function Line({ summary, factory, actions }: { summary: FactorySummary; factory: string | null; actions: Actions }) {
  const factories = useMemo(() => shownFactories(summary, factory), [summary, factory]);
  const items = useMemo(() => shownInbox(summary, factory), [summary, factory]);
  const rows = useMemo(() => lineRows(factories, items), [factories, items]);
  const followUps = factories.flatMap((view) => view.follow_ups.map((line) => ({ view, line })));
  const activity = factories.flatMap((view) => view.activity.map((entry) => ({ view, entry }))).sort((a, b) => b.entry.at - a.entry.at);
  // A GitHub Factory that has not finished its first read draws dim rows, never an empty state that may be wrong (B31).
  const reading = rows.length === 0 && factories.some((view) => view.source === "github" && view.outside_read_at === null);
  return (
    <div className="flex min-h-full flex-col gap-lg px-lg pb-xl" data-factory-line="true">
      <Decisions items={items} factories={factories} actions={actions} />
      {rows.length > 0 ? <LineTable rows={rows} inbox={items} /> : reading ? <ReadingTable /> : items.length === 0 ? <EmptyLine factories={factories} /> : null}
      {followUps.length > 0 ? <FollowUps lines={followUps} count={factories.reduce((sum, view) => sum + view.follow_ups_open, 0)} actions={actions} /> : null}
      {activity.length > 0 ? <FactoryActivity entries={activity} showProject={factory === null} /> : null}
    </div>
  );
}

function TableHead() {
  const { t } = useInterfaceTranslation();
  return (
    <div role="row" className="factory-line-grid border-b border-border px-md pb-xs text-caption text-muted-foreground">
      <span role="columnheader">{t("factory.line.task")}</span>
      <span role="columnheader">{t("factory.line.stage")}</span>
      <span role="columnheader">{t("factory.line.now")}</span>
      <span role="columnheader" className="text-right">{t("factory.line.elapsed")}</span>
      <span role="columnheader" className="text-right">{t("factory.line.ai")}</span>
    </div>
  );
}

function LineTable({ rows, inbox }: { rows: LineRow[]; inbox: InboxItem[] }) {
  const { t } = useInterfaceTranslation();
  return (
    <div role="table" aria-label={t("factory.tab.line")} className="flex flex-col" data-factory-line-table={rows.length}>
      <TableHead />
      {rows.map((row) => (
        <Row key={`${row.factory.id}/${row.card.task}`} row={row} item={rowItem(inbox, row)} />
      ))}
    </div>
  );
}

/** Whether the row is the person's turn: the only rows the warning band marks (B26). */
function personRow(card: CardView, item: InboxItem | undefined): boolean {
  return card.needs_person || card.waiting_group === "person" || item !== undefined;
}

function Row({ row, item }: { row: LineRow; item: InboxItem | undefined }) {
  const { t, i18n } = useInterfaceTranslation();
  const { card, factory } = row;
  const person = personRow(card, item);
  const done = card.column === "done";
  const agents = useShellStore((state) => state.agents);
  const summaries = useShellStore((state) => state.rest?.ui_state?.agent_summary !== false);
  const line = summaries && card.worker_pane ? agents.find((agent) => agent.pane_id === card.worker_pane)?.request?.line : undefined;
  const now = nowText(card, item, line, t, i18n.language);
  return (
    <button
      type="button"
      role="row"
      className={cn("factory-line-grid relative min-w-0 border-b border-border px-md py-sm text-left outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring", done && "opacity-(--opacity-dimmed)")}
      data-factory-row={card.task}
      data-factory-row-turn={person ? "true" : undefined}
      onClick={() => useUiStore.getState().setFactoryPlace({ task: { factory: factory.id, task: card.task } })}
    >
      {person ? <span role="img" aria-label={t("factory.board.person")} className="factory-line-band absolute inset-y-0 left-0 bg-warning" /> : null}
      <span role="cell" className="flex min-w-0 flex-col">
        <span className="truncate text-body font-semibold text-foreground" title={card.title}>{card.title}</span>
        <span className="flex min-w-0 items-center gap-xs font-mono text-caption text-muted-foreground">
          <span className="truncate">{card.display_id}</span>
          {card.pr ? (
            <span className="flex shrink-0 items-center gap-xxs">
              <GitPullRequestIcon aria-hidden="true" className="size-(--size-icon-sm)" />
              {card.pr.number}
            </span>
          ) : null}
        </span>
      </span>
      <span role="cell">
        <Track card={card} person={person} />
      </span>
      <span role="cell" className={cn("min-w-0 truncate text-body", person ? "text-warning" : "text-subtle-foreground")} title={now} data-factory-row-now="true">
        {now}
      </span>
      <span role="cell" className={cn("text-right text-caption", person ? "text-warning" : "text-muted-foreground")}>
        <Elapsed since={card.since} />
      </span>
      <span role="cell" className="flex items-center justify-end gap-xxs text-caption text-muted-foreground" data-factory-row-ai={card.ai_decisions}>
        {card.ai_decisions > 0 ? (
          <>
            <SparklesIcon aria-label={t("factory.line.aiDecided")} className="size-(--size-icon-sm)" />
            {card.ai_decisions}
          </>
        ) : null}
      </span>
    </button>
  );
}

/** The four-cell track 접수 · 작업 · 검증 · 머지 and the current cell's word, in the row's tone. */
export function Track({ card, person }: { card: CardView; person: boolean }) {
  const { t } = useInterfaceTranslation();
  const done = card.stage >= 4;
  // The board's stage 0 is "not started"; on the track a Task past intake waits at 작업.
  const stage = card.stage === 0 && card.state !== "drafting" ? 1 : card.stage;
  const working = card.state === "running" || card.state === "verifying" || card.state === "relanding";
  const tone = done ? "text-success" : person ? "text-warning" : working ? "text-agent-working" : "text-muted-foreground";
  const current = done ? "bg-success" : person ? "bg-warning" : working ? "bg-agent-working" : "bg-muted-foreground";
  return (
    <span className="flex flex-col gap-xxs" data-factory-track={stage}>
      <span className="factory-line-track" aria-hidden="true">
        {[0, 1, 2, 3].map((cell) => (
          <span key={cell} className={cn("factory-line-cell", done || cell < stage ? "bg-success" : cell === stage ? current : "bg-border")} />
        ))}
      </span>
      <span className={cn("text-caption", tone)}>{t(STAGE_WORD[Math.min(stage, 4)]!)}</span>
    </span>
  );
}

/** What the Task does now, in one sentence (B25): its turn, its wait, or what its worker last said. */
export function nowText(card: CardView, item: InboxItem | undefined, line: string | undefined, t: Translate, language: string): string {
  if (card.permission_wait) return t("factory.now.permission");
  if (card.state === "merge_waiting") return card.pr ? t("factory.now.merge", { gates: item && item.gates.length > 0 ? item.gates.map((gate) => t(GATE_LABEL[gate])).join(", ") : t("factory.now.mergeManual") }) : t("factory.state.merge_waiting");
  if (item && item.group === "answer") return t("factory.now.answer", { text: item.text });
  if (card.state === "stopped") return card.recovering ? t("factory.now.recovering") : t("factory.now.stopped", { reason: card.stop ? t(STOP_LABEL[card.stop]) : t("factory.state.stopped") });
  if (card.state === "paused") return card.pause_reason ? t(PAUSE_REASON_LABEL[card.pause_reason]) : t("factory.state.paused");
  if (card.resume_at !== null) return t("factory.now.resting", { time: new Date(card.resume_at).toLocaleTimeString(language, { hour: "2-digit", minute: "2-digit", hourCycle: "h23" }) });
  if (card.state === "outside" && card.pr) return t("factory.now.outside", { number: card.pr.number });
  if (card.state === "drafting") return t("factory.now.drafting");
  if (card.state === "done" || card.state === "landed") return card.pr ? t("factory.now.merged", { number: card.pr.number }) : t("factory.state.done");
  // While its check runs the worker is idle, so the check says what happens.
  if (card.state === "verifying") return card.pr ? t("factory.now.verifying", { number: card.pr.number }) : t("factory.state.verifying");
  if (line) return line;
  return waitingText(card, t) ?? t(STATE_LABEL[card.state]);
}

/** The first read of GitHub has not finished: the table's frame with dim rows (B31). */
export function ReadingTable() {
  const { t } = useInterfaceTranslation();
  return (
    <div role="table" aria-busy="true" aria-label={t("factory.loading")} className="flex flex-col" data-factory-line-reading="true">
      <TableHead />
      {[0, 1, 2].map((at) => (
        <div key={at} className="factory-line-grid border-b border-border px-md py-sm">
          <span className="flex flex-col gap-xs">
            <span className="h-(--size-icon-sm) w-4/5 rounded-sm bg-muted" />
            <span className="h-(--size-icon-sm) w-1/4 rounded-sm bg-muted" />
          </span>
          <span className="flex flex-col gap-xs">
            <span className="factory-line-track">{[0, 1, 2, 3].map((cell) => <span key={cell} className="factory-line-cell bg-muted" />)}</span>
            <span className="h-(--size-icon-sm) w-1/4 rounded-sm bg-muted" />
          </span>
          <span className="h-(--size-icon-sm) w-4/5 self-center rounded-sm bg-muted" />
          <span />
          <span className="h-(--size-icon-sm) self-center rounded-sm bg-muted" />
        </div>
      ))}
    </div>
  );
}

/** No Task yet (B31): how one arrives, with the label a GitHub issue takes. */
function EmptyLine({ factories }: { factories: FactoryView[] }) {
  const { t } = useInterfaceTranslation();
  const github = factories.some((view) => view.source === "github");
  return (
    <p className="flex flex-wrap items-center gap-sm text-body text-subtle-foreground" data-factory-line-empty={github ? "github" : "local"}>
      <TagIcon aria-hidden="true" className="size-(--size-icon) shrink-0" />
      {github ? t("factory.line.empty") : t("factory.line.emptyLocal")}
      {github ? <code className="rounded-full border border-border px-sm py-xxs font-mono text-caption">{FACTORY_LABEL}</code> : null}
    </p>
  );
}

/** A folded section at the line's foot: its icon, title, count and one line of what it holds. */
function Fold({ icon: Icon, title, count, hint, children, name }: { icon: typeof ListIcon; title: string; count: number; hint: string; children: React.ReactNode; name: string }) {
  const [open, setOpen] = useState(false);
  return (
    <section className="flex flex-col gap-sm" data-factory-fold={name} data-factory-fold-open={open ? "true" : "false"}>
      <button type="button" className="flex min-w-0 items-center gap-xs self-start text-left text-caption text-subtle-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring" aria-expanded={open} onClick={() => setOpen(!open)}>
        {open ? <ChevronDownIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" /> : <ChevronRightIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />}
        <Icon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
        <span className="font-semibold text-foreground">{title}</span>
        <span>{count}</span>
        <span className="min-w-0 truncate">{hint}</span>
      </button>
      {open ? children : null}
    </section>
  );
}

/** The open follow-up candidates (B17-B19), folded. */
function FollowUps({ lines, count, actions }: { lines: { view: FactoryView; line: FollowUpView }[]; count: number; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  return (
    <Fold icon={LightbulbIcon} title={t("factory.followUp.title")} count={count} hint={t("factory.followUp.hint")} name="follow-ups">
      <ul className="flex flex-col gap-sm pl-lg">
        {lines.map(({ view, line }) => (
          <FollowUpRow key={`${view.id}/${line.task}/${line.discovery}`} view={view} line={line} actions={actions} />
        ))}
      </ul>
    </Fold>
  );
}

/** One follow-up candidate with its three buttons; a failure stays on its line and the button can be pressed again (B19). */
export function FollowUpRow({ view, line, actions }: { view: FactoryView; line: FollowUpView; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const request = useFactoryRequest(actions);
  const busy = request.state.phase === "sending" || request.state.phase === "taken";
  const send = (choice: "issue" | "factory" | "discard") => request.send({ verb: "follow_up", task: `${view.id}/${line.task}`, discovery: line.discovery, choice });
  return (
    <li className="flex min-w-0 flex-col gap-xxs" data-factory-follow-up={`${line.task}/${line.discovery}`}>
      <div className="flex min-w-0 items-start gap-sm">
        <LightbulbIcon aria-hidden="true" className="mt-xxs size-(--size-icon-sm) shrink-0 text-muted-foreground" />
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="text-body [overflow-wrap:anywhere]">{line.text}</span>
          <span className="flex items-center gap-xs text-caption text-muted-foreground">
            <span className="font-mono">{line.display_id}</span>·<Elapsed since={line.at} />
          </span>
        </span>
        {line.state === "open" ? (
          <span className="flex shrink-0 items-center gap-xs">
            <Button variant="outline" size="sm" disabled={busy} data-factory-follow-up-action="issue" onClick={() => send("issue")}>{t("factory.followUp.issue")}</Button>
            <Button variant="outline" size="sm" disabled={busy} data-factory-follow-up-action="factory" onClick={() => send("factory")}>{t("factory.followUp.factory")}</Button>
            <Button variant="ghost" size="sm" disabled={busy} data-factory-follow-up-action="discard" onClick={() => send("discard")}>{t("factory.followUp.discard")}</Button>
          </span>
        ) : null}
      </div>
      {line.failure ? <p className="pl-lg text-caption text-destructive [overflow-wrap:anywhere]" data-factory-follow-up-failure="true">{t("factory.followUp.failed", { reason: line.failure })}</p> : null}
      <Refusal state={request.state} />
    </li>
  );
}

/** What the Factory did that no person has to act on (B11, B13, B15), newest first, folded. */
function FactoryActivity({ entries, showProject }: { entries: { view: FactoryView; entry: FactoryView["activity"][number] }[]; showProject: boolean }) {
  const { t, i18n } = useInterfaceTranslation();
  return (
    <Fold icon={ListIcon} title={t("factory.line.activity")} count={entries.length} hint={t("factory.line.activityHint")} name="activity">
      <ol className="flex flex-col gap-xs pl-lg">
        {entries.map(({ view, entry }, at) => {
          const { text, detail } = activityText(entry, t);
          return (
            <li key={`${view.id}/${entry.at}/${at}`} className="flex min-w-0 items-baseline gap-sm text-body" data-factory-factory-activity={entry.kind}>
              <span className="shrink-0 font-mono text-caption text-muted-foreground">{new Date(entry.at).toLocaleTimeString(i18n.language, { hour: "2-digit", minute: "2-digit", hourCycle: "h23" })}</span>
              <span className="min-w-0 [overflow-wrap:anywhere]">{text}</span>
              {detail ? <span className="min-w-0 truncate text-caption text-muted-foreground">{detail}</span> : null}
              {showProject ? <span className="shrink-0 text-caption text-muted-foreground">{view.project_name}</span> : null}
            </li>
          );
        })}
      </ol>
    </Fold>
  );
}
