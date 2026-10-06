import { useState } from "react";
import { ArrowLeftIcon, ArrowRightIcon, BanIcon, CircleIcon, ExternalLinkIcon, GitPullRequestIcon, ListChecksIcon, MessageSquareIcon, PaperclipIcon, SquareTerminalIcon, XIcon } from "lucide-react";
import type { Actions } from "../actions";
import { Elapsed, useRemaining } from "../components/elapsed";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { Hint } from "../components/ui/tooltip";
import { useInterfaceTranslation } from "../i18n/client";
import { cn } from "../lib/utils";
import { useUiStore, type FactoryPlace } from "../ui";
import { taskRef, type FactoryCommand } from "./commands";
import { StateMark } from "./FactoryCard";
import { useTaskDetail } from "./FactoryScreen";
import { ACTION_LABEL, OUTCOME_LABEL, STAGE_LABEL, TONE_TEXT, stateTone } from "./labels";
import type { AttemptView, CardView, FactorySummary, FactoryView, Question, TaskDetail } from "./model";
import { Refusal } from "./MyTurn";
import { Revive } from "./FactoryBoard";
import { useFactoryRequest } from "./request";
import { inboxKey, taskChain } from "./view";

const TAB_LABEL = { turn: "factory.tab.turn", board: "factory.tab.board", graph: "factory.tab.graph", settings: "factory.tab.settings" } as const;

/** The actions a page offers as buttons; `answer` goes to 내 차례 and `dep-remove` sits on each predecessor. */
const PAGE_ACTIONS = ["merge", "request-changes", "retry", "resume", "pause", "priority", "edit", "cancel"] as const;

/**
 * One Task's page (PRD software-factory-ui D-03, B18, B19): full width, the
 * small chain at the top, the card on the left and the progress and decision
 * record on the right. It offers only what the engine allows in the Task's
 * state; an open question is answered in 내 차례, the one place answers go.
 */
export function TaskPage({ summary, place, actions }: { summary: FactorySummary; place: FactoryPlace; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const section = useTaskDetail(actions, place.task);
  const factory = summary.factories.find((view) => view.id === place.task?.factory) ?? null;
  const back = () => useUiStore.getState().setFactoryPlace({ task: null });
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-auto px-lg pb-xl" data-factory-task-page={place.task?.task ?? ""}>
      <div className="flex shrink-0 items-center pt-md">
        <Button variant="ghost" size="sm" data-factory-back="true" onClick={back}>
          <ArrowLeftIcon />
          {t(TAB_LABEL[place.tab])}
        </Button>
      </div>
      {section === null || factory === null ? (
        <div className="flex flex-col gap-md pt-md" aria-busy="true" aria-label={t("factory.loading")} data-factory-task-loading="true">
          <div className="h-(--size-control-lg) w-2/5 rounded-sm bg-muted" />
          <div className="h-(--size-control) w-3/5 rounded-sm bg-muted" />
        </div>
      ) : section.detail === null ? (
        <p className="pt-md text-body text-muted-foreground" data-factory-task-missing="true">{t("factory.task.missing")}</p>
      ) : (
        <TaskBody detail={section.detail} factory={factory} actions={actions} />
      )}
    </div>
  );
}

function TaskBody({ detail, factory, actions }: { detail: TaskDetail; factory: FactoryView; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const request = useFactoryRequest(actions);
  const card = detail.card;
  const chain = taskChain(factory, card.task);
  const open = detail.questions.filter((question) => question.answer === null);
  const send = (command: FactoryCommand) => request.send(command);
  const task = taskRef(factory.id, card.task);
  return (
    <div className="flex flex-col gap-lg pt-sm" data-factory-task-state={card.state}>
      <div className="flex flex-col gap-xs">
        <div className="flex min-w-0 flex-wrap items-center gap-md">
          <h1 className="min-w-0 text-title font-semibold [overflow-wrap:anywhere]">{card.title}</h1>
          <span className="flex items-center gap-xs rounded-full bg-muted px-sm py-xxs">
            <StateMark card={card} />
          </span>
          {detail.stop ? <span className="text-caption text-warning" data-factory-stop="true">{detail.stop}</span> : null}
          <span className="flex-1" />
          <PageActions detail={detail} task={task} send={send} sending={request.state.phase === "sending"} actions={actions} />
        </div>
        <span className="flex min-w-0 flex-wrap gap-xs text-caption text-muted-foreground">
          <span className="font-mono">{card.display_id}</span>
          <span>·</span>
          <span>{factory.project_name}</span>
          {detail.branch ? (
            <>
              <span>·</span>
              <span className="font-mono [overflow-wrap:anywhere]">{detail.branch}</span>
            </>
          ) : null}
        </span>
        <Refusal state={request.state} />
        {card.state === "cancelled" || card.state === "outside" ? <Revive factory={factory.id} card={card} actions={actions} /> : null}
      </div>
      {open.map((question) => (
        <OpenQuestion key={question.id} question={question} factory={factory.id} task={card.task} />
      ))}
      {detail.gates.length > 0 ? (
        <p className="flex items-center gap-xs text-body text-warning" data-factory-gates="true">
          <ListChecksIcon aria-hidden="true" className="size-(--size-icon) shrink-0" />
          {detail.gates.join(" · ")}
        </p>
      ) : null}
      <Chain factory={factory} card={card} before={chain.before} after={chain.after} canRemove={detail.allowed.includes("dep-remove")} send={send} />
      <div className="grid grid-cols-2 gap-xl border-t border-border pt-lg">
        <div className="flex min-w-0 flex-col gap-lg" data-factory-card-fields="true">
          <Field title={t("factory.task.goal")}>
            <p className="text-body [overflow-wrap:anywhere]">{detail.goal}</p>
          </Field>
          {detail.criteria.length > 0 ? (
            <Field title={t("factory.task.criteria")}>
              <ul className="flex flex-col gap-xs">
                {detail.criteria.map((line, at) => (
                  <li key={at} className="flex items-start gap-sm text-body">
                    <CircleIcon aria-hidden="true" className="mt-xxs size-(--size-icon-sm) shrink-0 text-muted-foreground" />
                    <span className="[overflow-wrap:anywhere]">{line}</span>
                  </li>
                ))}
              </ul>
            </Field>
          ) : null}
          {detail.out_of_scope.length > 0 ? (
            <Field title={t("factory.task.outOfScope")}>
              <ul className="flex flex-col gap-xs">
                {detail.out_of_scope.map((line, at) => (
                  <li key={at} className="flex items-start gap-sm text-body text-subtle-foreground">
                    <BanIcon aria-hidden="true" className="mt-xxs size-(--size-icon-sm) shrink-0" />
                    <span className="[overflow-wrap:anywhere]">{line}</span>
                  </li>
                ))}
              </ul>
            </Field>
          ) : null}
          {detail.attachments.length > 0 ? (
            <Field title={t("factory.task.attachments")}>
              <ul className="flex flex-col gap-xs">
                {detail.attachments.map((file) => (
                  <li key={file.sha256} className="flex items-center gap-sm text-body">
                    <PaperclipIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0 text-muted-foreground" />
                    <span className="min-w-0 truncate">{file.original}</span>
                    <span className="text-caption text-muted-foreground">v{file.version}</span>
                  </li>
                ))}
              </ul>
            </Field>
          ) : null}
        </div>
        <div className="flex min-w-0 flex-col gap-lg">
          <Progress detail={detail} actions={actions} />
          <Decisions detail={detail} />
        </div>
      </div>
    </div>
  );
}

function Field({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-sm">
      <h2 className="text-subhead font-semibold">{title}</h2>
      {children}
    </section>
  );
}

function OpenQuestion({ question, factory, task }: { question: Question; factory: string; task: string }) {
  const { t } = useInterfaceTranslation();
  const left = useRemaining(question.deadline);
  return (
    <div className="flex min-w-0 flex-wrap items-center gap-sm text-body" data-factory-open-question={question.id}>
      <MessageSquareIcon aria-hidden="true" className="size-(--size-icon) shrink-0 text-warning" />
      <span className="min-w-0 [overflow-wrap:anywhere]">{t("factory.task.toAnswer", { question: question.text })}</span>
      {left && left !== "past" ? <span className="text-caption text-muted-foreground">{t("factory.turn.left", { time: left.text })}</span> : null}
      {question.default_action ? <span className="text-caption text-muted-foreground">{t("factory.turn.defaultAction", { action: question.default_action })}</span> : null}
      <Button
        variant="link"
        size="sm"
        data-factory-answer-in-turn={question.id}
        onClick={() => useUiStore.getState().setFactoryPlace({ task: null, tab: "turn", factory: null, focus: inboxKey({ factory, task, question: question.id, group: "answer" }) })}
      >
        {t("factory.task.answerInTurn")}
        <ArrowRightIcon />
      </Button>
    </div>
  );
}

/** The small chain (B18): what comes first, this Task, and what waits on it; a predecessor can be let go while the Task waits. */
function Chain({ factory, card, before, after, canRemove, send }: { factory: FactoryView; card: CardView; before: CardView[]; after: CardView[]; canRemove: boolean; send: (command: FactoryCommand) => void }) {
  const { t } = useInterfaceTranslation();
  if (before.length === 0 && after.length === 0) return null;
  return (
    <div className="flex min-w-0 items-start gap-sm" data-factory-chain="true">
      <ChainColumn title={t("factory.task.before")} cards={before} factory={factory} onRemove={canRemove ? (from) => send({ verb: "dep", task: taskRef(factory.id, card.task), on: taskRef(factory.id, from.task), remove: true }) : null} />
      <ArrowRightIcon aria-hidden="true" className="mt-xl size-(--size-icon) shrink-0 text-muted-foreground" />
      <div className="flex shrink-0 flex-col gap-xs">
        <span className="text-caption text-muted-foreground">{t("factory.task.this")}</span>
        <span className="flex items-center gap-xs rounded-md bg-muted px-md py-xs font-mono text-body font-semibold" data-factory-chain-this={card.task}>
          <CircleIcon aria-hidden="true" className={cn("size-(--size-icon-sm)", TONE_TEXT[stateTone(card.state, card.needs_person)])} />
          {card.display_id}
        </span>
      </div>
      <ArrowRightIcon aria-hidden="true" className="mt-xl size-(--size-icon) shrink-0 text-muted-foreground" />
      <ChainColumn title={t("factory.task.after")} cards={after} factory={factory} onRemove={null} />
    </div>
  );
}

function ChainColumn({ title, cards, factory, onRemove }: { title: string; cards: CardView[]; factory: FactoryView; onRemove: ((card: CardView) => void) | null }) {
  const { t } = useInterfaceTranslation();
  return (
    <div className="flex min-w-0 flex-1 flex-col gap-xs">
      <span className="text-caption text-muted-foreground">{title}</span>
      {cards.length === 0 ? <span className="text-caption text-muted-foreground">-</span> : null}
      {cards.map((other) => (
        <span key={other.task} className="flex min-w-0 items-center gap-xs rounded-md border border-border" data-factory-chain-card={other.task}>
          <button
            type="button"
            className="flex min-w-0 flex-1 items-center gap-xs px-md py-xs text-left text-body outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring"
            onClick={() => useUiStore.getState().setFactoryPlace({ task: { factory: factory.id, task: other.task } })}
          >
            <StateMark card={other} className="shrink-0" />
            <span className="shrink-0 font-mono text-caption text-muted-foreground">{other.display_id}</span>
            <span className="min-w-0 truncate">{other.title}</span>
          </button>
          {onRemove ? (
            <Hint label={t("factory.action.depRemove")}>
              <Button variant="ghost" size="icon-sm" aria-label={t("factory.action.depRemove")} data-factory-dep-remove={other.task} onClick={() => onRemove(other)}>
                <XIcon />
              </Button>
            </Hint>
          ) : null}
        </span>
      ))}
    </div>
  );
}

/** The state's actions as buttons (D-07, B19): nothing the engine would refuse in this state is drawn. */
function PageActions({ detail, task, send, sending, actions }: { detail: TaskDetail; task: string; send: (command: FactoryCommand) => void; sending: boolean; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const [comment, setComment] = useState<string | null>(null);
  const [priority, setPriority] = useState<string | null>(null);
  const allowed = PAGE_ACTIONS.filter((action) => detail.allowed.includes(action));
  if (allowed.length === 0) return null;
  const button = (action: (typeof PAGE_ACTIONS)[number], onClick: () => void, variant: "default" | "secondary" | "ghost" = "secondary") => (
    <Button key={action} variant={variant} size="sm" disabled={sending} data-factory-action={action} onClick={onClick}>
      {t(ACTION_LABEL[action]!)}
    </Button>
  );
  return (
    <div className="flex flex-wrap items-center gap-xs" data-factory-actions={allowed.join(" ")}>
      {allowed.map((action) => {
        switch (action) {
          case "merge":
            return button(action, () => send({ verb: "merge", task }), "default");
          case "request-changes":
            return comment === null ? (
              button(action, () => setComment(""))
            ) : (
              <form key={action} className="flex items-center gap-xs" onSubmit={(event) => { event.preventDefault(); if (comment.trim()) send({ verb: "request_changes", task, comment: comment.trim() }); setComment(null); }}>
                <Input autoFocus value={comment} onChange={(event) => setComment(event.target.value)} placeholder={t("factory.turn.commentPlaceholder")} aria-label={t("factory.turn.commentPlaceholder")} data-factory-comment="true" />
                <Button type="submit" size="sm" disabled={!comment.trim()}>{t("factory.action.requestChanges")}</Button>
              </form>
            );
          case "retry":
            return button(action, () => send({ verb: "retry", task }), "default");
          case "resume":
            return button(action, () => send({ verb: "resume", task }));
          case "pause":
            return button(action, () => send({ verb: "pause", task }));
          case "priority":
            return priority === null ? (
              button(action, () => setPriority(String(detail.card.priority)))
            ) : (
              <form key={action} className="flex items-center gap-xs" onSubmit={(event) => { event.preventDefault(); const value = Number(priority); if (Number.isInteger(value)) send({ verb: "priority", task, priority: value }); setPriority(null); }}>
                <Input autoFocus type="number" className="w-(--size-control-lg)" value={priority} onChange={(event) => setPriority(event.target.value)} aria-label={t("factory.action.priority")} data-factory-priority="true" />
                <Button type="submit" size="sm">{t("factory.task.set")}</Button>
              </form>
            );
          case "edit":
            // A card is changed by the agent the person talks to (D-12); the secretary reads the board.
            return button(action, () => actions.openSecretary(), "ghost");
          case "cancel":
            return button(action, () => send({ verb: "cancel", task }), "ghost");
        }
      })}
    </div>
  );
}

function Progress({ detail, actions }: { detail: TaskDetail; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const card = detail.card;
  return (
    <Field title={t("factory.task.progress")}>
      <div className="flex flex-col gap-sm text-body" data-factory-progress="true">
        {detail.pr ? (
          <span className="flex min-w-0 items-center gap-xs">
            <GitPullRequestIcon aria-hidden="true" className="size-(--size-icon) shrink-0 text-muted-foreground" />
            <Button variant="link" size="sm" className="px-none" data-factory-pr={detail.pr.number} onClick={() => actions.openLink(detail.pr!.url, true)}>
              {t("factory.task.pr", { number: detail.pr.number })}
            </Button>
            {!detail.pr.open ? <span className="text-caption text-muted-foreground">{t("factory.task.prClosed")}</span> : null}
          </span>
        ) : null}
        <span className={cn("flex items-center gap-xs", card.failures > 0 && "text-warning")} data-factory-verification={detail.verification}>
          <ListChecksIcon aria-hidden="true" className="size-(--size-icon) shrink-0" />
          {t("factory.task.verification", { value: detail.verification })}
        </span>
        {card.external.length > 0 ? (
          <span className="text-caption text-muted-foreground [overflow-wrap:anywhere]" data-factory-external-wait="true">
            {t("factory.task.externalWait", { refs: card.external.join(", ") })}
          </span>
        ) : null}
        {detail.merge_sha ? <span className="font-mono text-caption text-muted-foreground">{t("factory.task.merged", { sha: detail.merge_sha.slice(0, 12) })}</span> : null}
        {detail.worker_name || card.worker_pane ? (
          <span className="flex min-w-0 items-center gap-xs">
            <SquareTerminalIcon aria-hidden="true" className="size-(--size-icon) shrink-0 text-muted-foreground" />
            <span className="min-w-0 truncate">{detail.worker_name ?? t("factory.task.worker")}</span>
            {detail.worktree ? <span className="min-w-0 truncate font-mono text-caption text-muted-foreground">{detail.worktree.split("/").pop()}</span> : null}
            <span className="flex-1" />
            {card.worker_pane ? (
              <Button variant="outline" size="sm" data-factory-worker={card.worker_pane} onClick={() => actions.openAgent(card.worker_pane!)}>
                {t("factory.task.showWorker")}
              </Button>
            ) : null}
          </span>
        ) : null}
        {detail.attempts.map((attempt) => (
          <Attempt key={`${attempt.stage}:${attempt.number}`} attempt={attempt} actions={actions} />
        ))}
      </div>
    </Field>
  );
}

/** One verification attempt: its stage and outcome, the failed check, the kept tail of its log and the CI link for the rest (D-11). */
function Attempt({ attempt, actions }: { attempt: AttemptView; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const [open, setOpen] = useState(false);
  const tone = attempt.outcome === "passed" ? "text-success" : attempt.outcome === "running" ? "text-agent-working" : "text-warning";
  const link = attempt.link && /^https?:\/\//.test(attempt.link) ? attempt.link : null;
  return (
    <div className="flex flex-col gap-xxs" data-factory-attempt={attempt.number} data-factory-attempt-outcome={attempt.outcome}>
      <span className="flex min-w-0 flex-wrap items-center gap-xs text-caption">
        <span className="text-muted-foreground">{t("factory.task.attempt", { number: attempt.number })}</span>
        <span className="text-muted-foreground">{t(STAGE_LABEL[attempt.stage])}</span>
        <span className={tone}>{t(OUTCOME_LABEL[attempt.outcome])}</span>
        {attempt.check ? <span className="min-w-0 truncate font-mono">{attempt.check}</span> : null}
        <Elapsed since={attempt.started_at} className="text-muted-foreground" />
        {attempt.log_tail ? (
          <Button variant="link" size="sm" aria-expanded={open} onClick={() => setOpen(!open)}>
            {open ? t("factory.task.hideLog") : t("factory.task.showLog")}
          </Button>
        ) : null}
        {link ? (
          <Button variant="link" size="sm" onClick={() => actions.openLink(link, true)}>
            {t("factory.task.ciLink")}
            <ExternalLinkIcon />
          </Button>
        ) : null}
      </span>
      {open && attempt.log_tail ? (
        <pre className="max-h-(--size-pr-popover) overflow-auto rounded-sm bg-muted p-sm font-mono text-caption whitespace-pre-wrap [overflow-wrap:anywhere]" data-factory-log="true">
          {attempt.log_tail}
        </pre>
      ) : null}
    </div>
  );
}

function Decisions({ detail }: { detail: TaskDetail }) {
  const { t } = useInterfaceTranslation();
  if (detail.decisions.length === 0) return null;
  return (
    <Field title={t("factory.task.decisions")}>
      <ol className="flex flex-col gap-sm" data-factory-decisions={detail.decisions.length}>
        {[...detail.decisions].reverse().map((decision, at) => (
          <li key={`${decision.at}:${at}`} className="flex min-w-0 items-start gap-sm text-body">
            <span className="min-w-0 flex-1 [overflow-wrap:anywhere]">{decision.text}</span>
            <span className="flex shrink-0 gap-xxs text-caption text-muted-foreground">
              {decision.by}
              <span>·</span>
              <Elapsed since={decision.at} />
            </span>
          </li>
        ))}
      </ol>
    </Field>
  );
}
