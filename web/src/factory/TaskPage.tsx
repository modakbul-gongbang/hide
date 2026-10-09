import { Fragment, useEffect, useState } from "react";
import { ArrowLeftIcon, ArrowRightIcon, BanIcon, ChevronDownIcon, ChevronRightIcon, CircleAlertIcon, CircleCheckIcon, CircleHelpIcon, CircleIcon, CirclePauseIcon, CircleXIcon, ExternalLinkIcon, FileTextIcon, GitPullRequestIcon, LightbulbIcon, ListChecksIcon, LoaderCircleIcon, MessageSquareIcon, PaperclipIcon, PlayIcon, RefreshCwIcon, SparklesIcon, SquareTerminalIcon, TagIcon, Undo2Icon, UserIcon, WrenchIcon, XIcon } from "lucide-react";
import type { Actions } from "../actions";
import { Elapsed, useRemaining } from "../components/elapsed";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../components/ui/select";
import { agentAdapter } from "../agentAdapters";
import { MarkdownText } from "../MarkdownText";
import { Hint } from "../components/ui/tooltip";
import { useInterfaceTranslation } from "../i18n/client";
import type { MessageKey } from "../i18n/catalogs";
import { cn } from "../lib/utils";
import { useShellStore } from "../store";
import { useUiStore, type FactoryPlace } from "../ui";
import { inboxKey } from "./choices";
import { taskRef, type FactoryCommand } from "./commands";
import { Refusal } from "./Decisions";
import { StateMark, stateIcon } from "./FactoryCard";
import { TAB_LABEL, useTaskDetail } from "./FactoryScreen";
import { Revive } from "./FactoryBoard";
import { activityText, actionKey, CRITERION_LABEL, DECISION_SOURCE_LABEL, DIAGNOSIS_SOURCE_LABEL, GATE_LABEL, PAUSE_REASON_LABEL, STOP_LABEL, TONE_TEXT, stateTone } from "./labels";
import { FollowUpRow, nowText } from "./Line";
import type { Activity, AttemptView, CardView, CriterionState, DecisionView, FactorySummary, FactoryView, Question, TaskDetail, WorkerCandidate, WorkerReport } from "./model";
import { useFactoryRequest, type FactoryRequest } from "./request";
import { splitAtCuts, taskChain, wholeDecision } from "./view";

/** The engine's priority is a 32-bit integer; a larger one would not reach it. */
const PRIORITY_LIMIT = 2_147_483_647;

/** The actions a page offers as buttons; an answer goes to 결정 필요 and `dep-remove` sits on each predecessor. */
const PAGE_ACTIONS = ["merge", "request-changes", "retry", "resume", "pause", "priority", "edit", "cancel"] as const;

/** The states whose decisions are only read: a finished Task's record (B28). */
const FINISHED = new Set(["done", "landed", "cancelled", "outside"]);

/**
 * One Task's page (PRD factory-human-loop D-35, B27-B30): top to bottom the
 * title and its state in one sentence, the four-step track with where a
 * person or Factory AI decided, the summary, the completion checklist, the
 * decisions split between mine and Factory AI's, the activity timeline, the
 * original issue folded, and this Task's follow-up candidates. It offers only
 * what the engine allows in the Task's state; an open question is answered
 * in 결정 필요.
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
      {factory === null ? (
        <p className="pt-md text-body text-muted-foreground" data-factory-task-missing="true">{t("factory.task.missing")}</p>
      ) : section === null ? (
        <div className="flex flex-col gap-md pt-md" aria-busy="true" aria-label={t("factory.loading")} data-factory-task-loading="true">
          <div className="h-(--size-control-lg) w-2/5 rounded-sm bg-muted" />
          <div className="h-(--size-control) w-3/5 rounded-sm bg-muted" />
        </div>
      ) : section.detail === null ? (
        <Unreadable actions={actions} task={place.task} />
      ) : (
        <TaskBody detail={section.detail} factory={factory} summary={summary} actions={actions} />
      )}
    </div>
  );
}

/** The page could not be read (B32): one button reads it again. */
function Unreadable({ actions, task }: { actions: Actions; task: FactoryPlace["task"] }) {
  const { t } = useInterfaceTranslation();
  return (
    <p className="flex items-center gap-sm pt-lg text-body text-subtle-foreground" data-factory-task-missing="true">
      <CircleAlertIcon aria-hidden="true" className="size-(--size-icon) shrink-0" />
      {t("factory.task.unreadable")}
      {task ? (
        <Button variant="outline" size="sm" data-factory-task-reread="true" onClick={() => { actions.factoryTaskClose(); actions.factoryTaskOpen(task.factory, task.task); }}>
          <RefreshCwIcon />
          {t("factory.task.reread")}
        </Button>
      ) : null}
    </p>
  );
}

function TaskBody({ detail, factory, summary, actions }: { detail: TaskDetail; factory: FactoryView; summary: FactorySummary; actions: Actions }) {
  const { t, i18n } = useInterfaceTranslation();
  const request = useFactoryRequest(actions);
  // The comment and priority forms have their own request, so another action's answer cannot close them.
  const form = useFactoryRequest(actions);
  const card = detail.card;
  const chain = taskChain(factory, card.task);
  const open = detail.questions.filter((question) => question.answer === null);
  const send = (command: FactoryCommand) => request.send(command);
  const task = taskRef(factory.id, card.task);
  const item = summary.inbox.find((row) => row.factory === factory.id && row.task === card.task);
  const agents = useShellStore((state) => state.agents);
  const line = card.worker_pane ? agents.find((agent) => agent.pane_id === card.worker_pane)?.request?.line : undefined;
  return (
    <div className="flex flex-col gap-xl pt-sm" data-factory-task-state={card.state}>
      <div className="flex flex-col gap-xs">
        <div className="flex min-w-0 flex-wrap items-center gap-md">
          <h1 className="min-w-0 text-title font-semibold [overflow-wrap:anywhere]">{card.title}</h1>
          <span className="flex items-center gap-xs rounded-full bg-muted px-sm py-xxs">
            <StateMark card={card} />
          </span>
          <span className="flex-1" />
          <PageActions detail={detail} task={task} send={send} sending={request.state.phase === "sending"} form={form} actions={actions} />
        </div>
        <div className="flex min-w-0 items-start gap-md">
          <div className="flex min-w-0 flex-1 flex-col gap-xxs">
            <p className="text-body text-subtle-foreground [overflow-wrap:anywhere]" data-factory-task-now="true">{nowText(card, item, line, t, i18n.language)}</p>
            <MetaLine detail={detail} actions={actions} />
          </div>
          {card.worker_pane ? (
            <Button variant="outline" size="sm" data-factory-worker={card.worker_pane} onClick={() => actions.openAgent(card.worker_pane!)}>
              {t("factory.task.showWorker")}
            </Button>
          ) : null}
        </div>
        <StopLine detail={detail} />
        <RestLine detail={detail} />
        <Refusal state={request.state} />
        <Refusal state={form.state} />
        {card.state === "cancelled" || card.state === "outside" ? <Revive factory={factory.id} card={card} actions={actions} /> : null}
        {open.map((question) => (
          <OpenQuestion key={question.id} question={question} factory={factory.id} task={card.task} />
        ))}
        {detail.gate_codes.length > 0 ? (
          <p className="flex items-center gap-xs text-body text-warning" data-factory-gates={detail.gate_codes.join(" ")}>
            <ListChecksIcon aria-hidden="true" className="size-(--size-icon) shrink-0" />
            {detail.gate_codes.map((gate) => t(GATE_LABEL[gate])).join(" · ")}
          </p>
        ) : null}
      </div>
      <StageTrack detail={detail} factory={factory} />
      <Chain factory={factory} card={card} before={chain.before} after={chain.after} canRemove={detail.allowed.includes("dep-remove")} send={send} />
      {detail.worker === null && !FINISHED.has(card.state) && factory.workers.length > 1 ? <WorkerPick factory={factory} detail={detail} actions={actions} /> : null}
      <div className="flex max-w-4/5 flex-col gap-xl border-t border-border pt-lg">
        <Summary detail={detail} />
        <Checklist detail={detail} />
        <Decisions detail={detail} factory={factory} actions={actions} />
        <Timeline detail={detail} actions={actions} />
        {detail.issue_text ? <OriginalIssue detail={detail} /> : null}
        {detail.follow_ups.length > 0 ? (
          <Field title={t("factory.task.followUps")} count={detail.follow_ups.length}>
            <ul className="flex flex-col gap-sm">
              {detail.follow_ups.map((line) => <FollowUpRow key={line.discovery} view={factory} line={line} actions={actions} />)}
            </ul>
          </Field>
        ) : null}
      </div>
    </div>
  );
}

function Field({ title, count, extra, children }: { title: string; count?: number; extra?: string; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-sm">
      <h2 className="flex items-baseline gap-sm text-subhead font-semibold">
        {title}
        {count !== undefined ? <span className="text-caption font-normal text-muted-foreground">{count}</span> : null}
        {extra ? <span className="text-caption font-normal text-muted-foreground">{extra}</span> : null}
      </h2>
      {children}
    </section>
  );
}

/** The issue and PR, the branch and the worker's agent, model and effort, on one muted line. */
function MetaLine({ detail, actions }: { detail: TaskDetail; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const card = detail.card;
  const worker = detail.worker ? candidateText({ agent: detail.worker.agent, model: detail.worker.model, effort: detail.worker.effort, description: "" }, t("factory.settings.cliDefault")) : null;
  return (
    <span className="flex min-w-0 flex-wrap items-center gap-xs text-caption text-muted-foreground" data-factory-task-meta="true">
      <span className="font-mono">{card.display_id}</span>
      {detail.pr ? (
        <Button variant="link" size="sm" className="h-auto gap-xxs px-none font-mono text-caption text-muted-foreground" data-factory-pr={detail.pr.number} onClick={() => actions.openLink(detail.pr!.url, true)}>
          <GitPullRequestIcon aria-hidden="true" />
          {detail.pr.number}
        </Button>
      ) : null}
      {detail.branch ? <span className="font-mono [overflow-wrap:anywhere]">· {detail.branch}</span> : null}
      {worker ? <span>· {worker}</span> : null}
      {card.external.length > 0 ? <span data-factory-external-wait="true">· {t("factory.task.externalWait", { refs: card.external.join(", ") })}</span> : null}
      {detail.merge_sha ? <span className="font-mono">· {t("factory.task.merged", { sha: detail.merge_sha.slice(0, 12) })}</span> : null}
    </span>
  );
}

/**
 * The four steps 접수 · 작업 · 검증 · 머지 (B27): each done, current or
 * ahead, with where Factory AI or a person decided under its step and the
 * verification count under 검증.
 */
function StageTrack({ detail, factory }: { detail: TaskDetail; factory: FactoryView }) {
  const { t } = useInterfaceTranslation();
  const card = detail.card;
  const steps: { word: MessageKey; marks: React.ReactNode[] }[] = [
    { word: "factory.line.stage.intake", marks: [] },
    { word: "factory.line.stage.work", marks: [] },
    { word: "factory.line.stage.verify", marks: [] },
    { word: "factory.line.stage.merge", marks: [] },
  ];
  const count = (source: DecisionView["source"], by: DecisionView["by"]) => detail.decisions.filter((decision) => decision.source === source && decision.by === by).length;
  const mark = (key: string, by: "ai" | "person", text: string) => (
    <span key={key} className={cn("flex items-center gap-xxs", by === "person" ? "text-warning" : "text-muted-foreground")} data-factory-track-mark={key}>
      {by === "person" ? <UserIcon aria-hidden="true" className="size-(--size-icon-sm)" /> : <SparklesIcon aria-hidden="true" className="size-(--size-icon-sm)" />}
      {text}
    </span>
  );
  const assumptions = count("assumption", "ai");
  if (assumptions > 0) steps[0]!.marks.push(mark("assumption", "ai", t("factory.track.assumptions", { count: assumptions })));
  const sentBack = count("send_back", "ai");
  if (sentBack > 0) steps[1]!.marks.push(mark("send-back", "ai", t("factory.track.sentBack", { count: sentBack })));
  const aiAnswers = count("answer", "ai");
  if (aiAnswers > 0) steps[1]!.marks.push(mark("ai-answer", "ai", t("factory.track.aiAnswers", { count: aiAnswers })));
  const mine = detail.decisions.filter((decision) => decision.by === "person").length;
  if (mine > 0) steps[1]!.marks.push(mark("person", "person", t("factory.track.mine", { count: mine })));
  const attempts = detail.attempts.filter((attempt) => attempt.stage === "task").length;
  steps[2]!.marks.push(<span key="verify">{factory.verification === "none" ? t("factory.task.noVerification") : t("factory.track.verify", { attempts, value: detail.verification })}</span>);
  steps[3]!.marks.push(<span key="merge">{factory.merge_mode === "auto" && factory.verification !== "none" ? t("factory.track.autoMerge") : t("factory.track.manualMerge")}</span>);
  const done = card.stage >= 4;
  const working = card.state === "running" || card.state === "verifying" || card.state === "relanding";
  const person = card.needs_person;
  const currentTone = person ? "text-warning" : working ? "text-agent-working" : "text-foreground";
  return (
    <ol className="grid grid-cols-4 gap-md" data-factory-stage-track={card.stage}>
      {steps.map((step, at) => {
        const complete = done || at < card.stage;
        const current = !done && at === card.stage;
        const Icon = complete ? CircleCheckIcon : current ? (working ? LoaderCircleIcon : person ? CircleHelpIcon : CircleIcon) : CircleIcon;
        return (
          <li key={step.word} className="flex min-w-0 flex-col gap-xxs" data-factory-step={at} data-factory-step-state={complete ? "done" : current ? "current" : "ahead"}>
            <span className="flex min-w-0 items-center gap-xs">
              <span className={cn("flex shrink-0 items-center gap-xxs text-body", complete ? "text-success" : current ? currentTone : "text-muted-foreground")}>
                <Icon aria-hidden="true" className={cn("size-(--size-icon-sm)", current && working && "animate-spin")} />
                {t(step.word)}
              </span>
              {at < 3 ? <span aria-hidden="true" className={cn("h-px min-w-0 flex-1", complete ? "bg-success" : "bg-border")} /> : null}
            </span>
            <span className="flex min-w-0 flex-col gap-xxs pl-lg text-caption text-muted-foreground">{step.marks}</span>
          </li>
        );
      })}
    </ol>
  );
}

/** The card's summary, the goal where no issue holds it, the out-of-scope list and the attachments. */
function Summary({ detail }: { detail: TaskDetail }) {
  const { t } = useInterfaceTranslation();
  const goal = detail.issue_text === null && detail.goal.trim() !== "" && detail.goal.trim() !== detail.card.summary.trim();
  return (
    <Field title={t("factory.task.summary")}>
      <p className="text-body [overflow-wrap:anywhere]" data-factory-summary="true">{detail.card.summary}</p>
      {goal ? <Goal text={detail.goal} /> : null}
      {detail.out_of_scope.length > 0 ? (
        <div className="flex flex-col gap-xxs" data-factory-out-of-scope="true">
          <span className="text-caption text-muted-foreground">{t("factory.task.outOfScope")}</span>
          <ul className="flex flex-col gap-xxs">
            {detail.out_of_scope.map((line, at) => (
              <li key={at} className="flex items-start gap-sm text-body text-subtle-foreground">
                <BanIcon aria-hidden="true" className="mt-xxs size-(--size-icon-sm) shrink-0" />
                <span className="[overflow-wrap:anywhere]">{line}</span>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
      {detail.attachments.length > 0 ? (
        <div className="flex flex-col gap-xxs" data-factory-attachments="true">
          <span className="text-caption text-muted-foreground">{t("factory.task.attachments")}</span>
          <ul className="flex flex-col gap-xs">
            {detail.attachments.map((file) => (
              <li key={file.sha256} className="flex items-center gap-sm text-body">
                <PaperclipIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0 text-muted-foreground" />
                <span className="min-w-0 truncate">{file.original}</span>
                <span className="text-caption text-muted-foreground">v{file.version}</span>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </Field>
  );
}

const CRITERION_ICON: Record<CriterionState | "open", { icon: typeof CircleIcon; tone: string }> = {
  met: { icon: CircleCheckIcon, tone: "text-success" },
  unmet: { icon: CircleXIcon, tone: "text-destructive" },
  unknown: { icon: CircleHelpIcon, tone: "text-muted-foreground" },
  open: { icon: CircleIcon, tone: "text-muted-foreground" },
};

/** The completion criteria as the last check judged them (B10); before any check each reads open. */
function Checklist({ detail }: { detail: TaskDetail }) {
  const { t } = useInterfaceTranslation();
  if (detail.checklist.length === 0) return null;
  const tally = (["met", "unmet", "unknown"] as const).flatMap((state) => {
    const count = detail.checklist.filter((criterion) => criterion.state === state).length;
    return count > 0 ? [`${t(CRITERION_LABEL[state])} ${count}`] : [];
  });
  return (
    <Field title={t("factory.task.criteria")} extra={tally.join(" · ") || undefined}>
      <ul className="flex flex-col gap-sm" data-factory-checklist={detail.checklist.length}>
        {detail.checklist.map((criterion, at) => {
          const state = criterion.state ?? "open";
          const { icon: Icon, tone } = CRITERION_ICON[state];
          return (
            <li key={at} className="flex min-w-0 items-start gap-sm text-body" data-factory-criterion={state}>
              <Icon aria-hidden="true" className={cn("mt-xxs size-(--size-icon-sm) shrink-0", tone)} />
              <span className="flex min-w-0 flex-1 flex-col">
                <span className="[overflow-wrap:anywhere]">{criterion.text}</span>
                {criterion.reason ? <span className="text-caption text-muted-foreground [overflow-wrap:anywhere]">{criterion.reason}</span> : null}
              </span>
              {criterion.state ? <span className={cn("shrink-0 text-caption", tone)}>{t(CRITERION_LABEL[criterion.state])}</span> : null}
            </li>
          );
        })}
      </ul>
    </Field>
  );
}

/**
 * The decisions (B28, B29): mine, Factory AI's and the worker's, each oldest
 * first. Factory AI's answer, assumption or send-back takes 다른 답 while
 * the Task is not finished; the changed decision becomes mine and a running
 * worker hears it by letter.
 */
function Decisions({ detail, factory, actions }: { detail: TaskDetail; factory: FactoryView; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  if (detail.decisions.length === 0) return null;
  const groups = [
    { by: "person", title: "factory.task.decisions.mine" },
    { by: "ai", title: "factory.task.decisions.ai" },
    { by: "worker", title: "factory.task.decisions.worker" },
  ] as const;
  return (
    <Field title={t("factory.task.decisions")}>
      <div className="flex flex-col gap-md" data-factory-decisions={detail.decisions.length}>
        {groups.map((group) => {
          const rows = detail.decisions.filter((decision) => decision.by === group.by);
          if (rows.length === 0) return null;
          return (
            <div key={group.by} className="flex flex-col gap-sm" data-factory-decision-group={group.by}>
              <span className="text-caption text-subtle-foreground">{t(group.title, { count: rows.length })}</span>
              <ol className="flex flex-col gap-sm">
                {rows.map((decision) => (
                  <DecisionRow key={decision.id} decision={decision} whole={wholeDecision(decision, detail.questions)} task={taskRef(factory.id, detail.card.task)} actions={actions} />
                ))}
              </ol>
            </div>
          );
        })}
      </div>
    </Field>
  );
}

/** A decision recorded as `question -> answer` reads as the question above its answer. */
function splitDecision(text: string): { question: string | null; answer: string } {
  const at = text.lastIndexOf(" -> ");
  return at < 0 ? { question: null, answer: text } : { question: text.slice(0, at), answer: text.slice(at + 4) };
}

function DecisionRow({ decision, whole, task, actions }: { decision: DecisionView; whole: string | null; task: string; actions: Actions }) {
  const { t, i18n } = useInterfaceTranslation();
  const request = useFactoryRequest(actions);
  const [text, setText] = useState<string | null>(null);
  const [open, setOpen] = useState(false);
  const busy = request.state.phase === "sending" || request.state.phase === "taken";
  const { question, answer } = splitDecision(open && whole !== null ? whole : decision.text);
  const Icon = decision.by === "person" ? UserIcon : decision.by === "ai" ? SparklesIcon : SquareTerminalIcon;
  const label = decision.source ? t(DECISION_SOURCE_LABEL[decision.source]) : null;
  return (
    <li className="flex min-w-0 flex-col gap-xs" data-factory-decision={decision.id} data-factory-decision-by={decision.by}>
      <div className="flex min-w-0 items-start gap-sm">
        <Icon aria-hidden="true" className={cn("mt-xxs size-(--size-icon-sm) shrink-0", decision.by === "person" ? "text-warning" : "text-muted-foreground")} />
        <span className="flex min-w-0 flex-1 flex-col gap-xxs">
          {question ? <ShortenedText text={question} className="text-caption text-subtle-foreground [overflow-wrap:anywhere]" /> : null}
          <ShortenedText text={answer} className="text-body font-semibold [overflow-wrap:anywhere]" />
          {whole !== null ? (
            <Button variant="link" size="sm" className="h-auto self-start px-none" aria-expanded={open} data-factory-decision-whole={open ? "open" : "closed"} onClick={() => setOpen(!open)}>
              {open ? t("factory.task.showLess") : t("factory.task.showAll")}
            </Button>
          ) : null}
          {decision.reason ? <span className="text-caption text-muted-foreground [overflow-wrap:anywhere]">{t("factory.task.reason", { text: decision.reason })}</span> : null}
          {decision.changed ? <span className="text-caption text-muted-foreground [overflow-wrap:anywhere]" data-factory-decision-changed="true">{t("factory.task.changedFrom", { text: decision.changed.from })}</span> : null}
        </span>
        {label ? <span className="shrink-0 text-caption text-muted-foreground">{label}</span> : null}
        {decision.overridable && text === null ? (
          <Button variant="ghost" size="sm" data-factory-decision-override={decision.id} onClick={() => setText("")}>
            {t("factory.task.override")}
          </Button>
        ) : null}
        <span className="shrink-0 font-mono text-caption text-muted-foreground">{new Date(decision.at).toLocaleTimeString(i18n.language, { hour: "2-digit", minute: "2-digit", hourCycle: "h23" })}</span>
      </div>
      {decision.overridable && text !== null ? (
        <form className="flex flex-col gap-xxs pl-lg" onSubmit={(event) => { event.preventDefault(); if (text.trim() && !busy) request.send({ verb: "answer", task, question: null, choice: null, text: text.trim(), decision: decision.id }); }}>
          <span className="flex items-center gap-xs">
            <Input autoFocus value={text} onChange={(event) => setText(event.target.value)} placeholder={t("factory.turn.overridePlaceholder")} aria-label={t("factory.turn.overridePlaceholder")} data-factory-override-text="true" />
            <Button type="submit" variant="outline" size="sm" disabled={!text.trim() || busy}>{t("factory.task.overrideSend")}</Button>
          </span>
          <span className="text-caption text-muted-foreground">{t("factory.task.overrideHint")}</span>
        </form>
      ) : null}
      <Refusal state={request.state} />
    </li>
  );
}

const ACTIVITY_ICON: Record<Activity["kind"], typeof CircleIcon> = {
  intake: TagIcon,
  started: PlayIcon,
  report: FileTextIcon,
  pull_request: GitPullRequestIcon,
  verification: CircleCheckIcon,
  sent_back: Undo2Icon,
  recovery: WrenchIcon,
  follow_up: LightbulbIcon,
  ai_decision: SparklesIcon,
  outside: ExternalLinkIcon,
  main_broken: CircleXIcon,
  cleanup_kept: WrenchIcon,
  watch: CircleAlertIcon,
  daily_limit: SparklesIcon,
  note: MessageSquareIcon,
};

/**
 * The activity timeline (B30): what happened, by time, the worker's reports
 * in their four parts with the letter as written folded, verification with
 * its link and log, and while the Task verifies, where it is now.
 */
function Timeline({ detail, actions }: { detail: TaskDetail; actions: Actions }) {
  const { t, i18n } = useInterfaceTranslation();
  const card = detail.card;
  const time = (at: number) => new Date(at).toLocaleTimeString(i18n.language, { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
  const live = card.state === "verifying" ? t("factory.track.verifyNow", { attempts: detail.attempts.filter((attempt) => attempt.stage === "task").length, value: detail.verification }) : null;
  if (detail.activity.length === 0 && live === null) return null;
  return (
    <Field title={t("factory.task.activity")}>
      <ol className="flex flex-col gap-sm" data-factory-timeline={detail.activity.length}>
        {detail.activity.map((entry, at) => {
          const { text, detail: under } = activityText(entry, t);
          const Icon = entry.kind === "verification" && entry.outcome !== "passed" ? CircleXIcon : ACTIVITY_ICON[entry.kind];
          const tone = entry.kind === "verification" ? (entry.outcome === "passed" ? "text-success" : "text-warning") : entry.kind === "main_broken" ? "text-destructive" : entry.kind === "pull_request" ? "text-success" : "text-muted-foreground";
          const attempt = entry.kind === "verification" ? detail.attempts.find((row) => row.number === entry.number && row.stage === "task") : undefined;
          return (
            <li key={`${entry.at}/${at}`} className="grid grid-cols-[max-content_max-content_1fr] items-start gap-x-sm" data-factory-activity={entry.kind}>
              <span className="font-mono text-caption text-muted-foreground">{time(entry.at)}</span>
              <Icon aria-hidden="true" className={cn("mt-xxs size-(--size-icon-sm)", tone)} />
              <span className="flex min-w-0 flex-col gap-xxs">
                <span className="flex min-w-0 flex-wrap items-center gap-xs text-body">
                  <span className="[overflow-wrap:anywhere]">{text}</span>
                  {entry.kind === "verification" && entry.link && /^https?:\/\//.test(entry.link) ? (
                    <Button variant="link" size="sm" className="h-auto px-none" onClick={() => actions.openLink(entry.link!, true)}>
                      {t("factory.task.ciLink")}
                      <ExternalLinkIcon />
                    </Button>
                  ) : null}
                </span>
                {under ? <span className="text-caption text-muted-foreground [overflow-wrap:anywhere]">{under}</span> : null}
                {entry.kind === "report" ? <ReportParts report={entry.report} /> : null}
                {attempt?.log_tail ? <LogTail attempt={attempt} /> : null}
              </span>
            </li>
          );
        })}
        {live ? (
          <li className="grid grid-cols-[max-content_max-content_1fr] items-start gap-x-sm" data-factory-activity="now">
            <span className="font-mono text-caption text-muted-foreground">{time(Date.now())}</span>
            <LoaderCircleIcon aria-hidden="true" className="mt-xxs size-(--size-icon-sm) animate-spin text-agent-working" />
            <span className="text-body">{live}</span>
          </li>
        ) : null}
      </ol>
    </Field>
  );
}

/** A report's four parts, what could not be checked in the warning tone, and the letter as written folded (B30). */
function ReportParts({ report }: { report: WorkerReport }) {
  const { t } = useInterfaceTranslation();
  const [raw, setRaw] = useState(false);
  const rows: { key: MessageKey; items: string[]; warn?: boolean }[] = [
    { key: "factory.report.result", items: [report.result] },
    { key: "factory.report.changed", items: report.changed ?? [] },
    { key: "factory.report.verified", items: report.verified ?? [] },
    { key: "factory.report.unverified", items: report.unverified ?? [], warn: true },
  ];
  return (
    <div className="flex flex-col gap-xxs pt-xxs" data-factory-report="true">
      <dl className="grid grid-cols-[max-content_1fr] gap-x-lg gap-y-xxs text-body">
        {rows.filter((row) => row.items.length > 0 && row.items.some((part) => part.trim() !== "")).map((row) => (
          <Fragment key={row.key}>
            <dt className="text-caption text-muted-foreground">{t(row.key)}</dt>
            <dd className={cn("[overflow-wrap:anywhere]", row.warn && "text-warning")}>{row.items.join(", ")}</dd>
          </Fragment>
        ))}
      </dl>
      {report.raw ? (
        <>
          <Button variant="link" size="sm" className="h-auto self-start px-none" aria-expanded={raw} data-factory-report-raw={raw ? "open" : "closed"} onClick={() => setRaw(!raw)}>
            {raw ? <ChevronDownIcon /> : <ChevronRightIcon />}
            {t("factory.report.raw")}
          </Button>
          {raw ? <pre className="max-h-(--size-pr-popover) overflow-auto rounded-sm bg-muted p-sm font-mono text-caption whitespace-pre-wrap [overflow-wrap:anywhere]">{report.raw}</pre> : null}
        </>
      ) : null}
    </div>
  );
}

/** The kept tail of a failed verification's log (D-11). */
function LogTail({ attempt }: { attempt: AttemptView }) {
  const { t } = useInterfaceTranslation();
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button variant="link" size="sm" className="h-auto self-start px-none" aria-expanded={open} onClick={() => setOpen(!open)}>
        {open ? t("factory.task.hideLog") : t("factory.task.showLog")}
      </Button>
      {open && attempt.log_tail ? (
        <pre className="max-h-(--size-pr-popover) overflow-auto rounded-sm bg-muted p-sm font-mono text-caption whitespace-pre-wrap [overflow-wrap:anywhere]" data-factory-log="true">
          <ShortenedText text={attempt.log_tail} />
        </pre>
      ) : null}
    </>
  );
}

/** The issue as written, read as Markdown, folded at the foot (B27). */
function OriginalIssue({ detail }: { detail: TaskDetail }) {
  const { t } = useInterfaceTranslation();
  const [open, setOpen] = useState(false);
  return (
    <section className="flex flex-col gap-sm" data-factory-issue={open ? "open" : "closed"}>
      <button type="button" className="flex items-center gap-xs self-start text-subhead font-semibold outline-none focus-visible:ring-1 focus-visible:ring-ring" aria-expanded={open} onClick={() => setOpen(!open)}>
        {open ? <ChevronDownIcon aria-hidden="true" className="size-(--size-icon-sm)" /> : <ChevronRightIcon aria-hidden="true" className="size-(--size-icon-sm)" />}
        {t("factory.task.issue")}
        {detail.card.issue ? <span className="font-mono text-caption font-normal text-muted-foreground">{detail.card.issue}</span> : null}
      </button>
      {open ? (
        <div className="rounded-md border border-border px-lg py-md">
          <Goal text={detail.issue_text ?? ""} />
        </div>
      ) : null}
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
      <Button
        variant="link"
        size="sm"
        data-factory-answer-in-line={question.id}
        onClick={() => useUiStore.getState().setFactoryPlace({ task: null, tab: "line", factory: null, focus: inboxKey({ factory, task, question: question.id, group: "answer" }) })}
      >
        {t("factory.task.answerInLine")}
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
          <ThisState card={card} />
          {card.display_id}
        </span>
      </div>
      <ArrowRightIcon aria-hidden="true" className="mt-xl size-(--size-icon) shrink-0 text-muted-foreground" />
      <ChainColumn title={t("factory.task.after")} cards={after} factory={factory} onRemove={null} />
    </div>
  );
}

/** This Task's state in the chain, drawn with the mark its neighbours carry; the words are in the title row. */
function ThisState({ card }: { card: CardView }) {
  const Icon = stateIcon(card.state);
  return <Icon aria-hidden="true" className={cn("size-(--size-icon-sm)", TONE_TEXT[stateTone(card.state, card.needs_person)])} />;
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
function PageActions({ detail, task, send, sending, form, actions }: { detail: TaskDetail; task: string; send: (command: FactoryCommand) => void; sending: boolean; form: FactoryRequest; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const [comment, setComment] = useState<string | null>(null);
  const [priority, setPriority] = useState<string | null>(null);
  const asking = form.state.phase === "sending";
  // A form stays open with what was typed until the engine takes it, so a refusal loses nothing.
  useEffect(() => {
    if (form.state.phase !== "taken") return;
    setComment(null);
    setPriority(null);
  }, [form.state]);
  const allowed = PAGE_ACTIONS.filter((action) => detail.allowed.includes(action));
  if (allowed.length === 0) return null;
  const button = (action: (typeof PAGE_ACTIONS)[number], onClick: () => void, variant: "default" | "secondary" | "ghost" = "secondary") => (
    <Button key={action} variant={variant} size="sm" disabled={sending} data-factory-action={action} onClick={onClick}>
      {t(actionKey(action, detail.card.state === "paused"))}
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
              <form key={action} className="flex items-center gap-xs" onSubmit={(event) => { event.preventDefault(); if (comment.trim() && !asking) form.send({ verb: "request_changes", task, comment: comment.trim() }); }}>
                <Input autoFocus value={comment} onChange={(event) => setComment(event.target.value)} placeholder={t("factory.turn.commentPlaceholder")} aria-label={t("factory.turn.commentPlaceholder")} data-factory-comment="true" />
                <Button type="submit" size="sm" disabled={!comment.trim() || asking}>{t("factory.action.requestChanges")}</Button>
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
              <form key={action} className="flex items-center gap-xs" onSubmit={(event) => { event.preventDefault(); const value = Number(priority); if (priority.trim() !== "" && Number.isInteger(value) && Math.abs(value) <= PRIORITY_LIMIT && !asking) form.send({ verb: "priority", task, priority: value }); }}>
                <Input autoFocus type="number" className="w-[calc(var(--size-control-lg)*3)]" value={priority} onChange={(event) => setPriority(event.target.value)} aria-label={t("factory.action.priority")} data-factory-priority="true" />
                <Button type="submit" size="sm" disabled={asking}>{t("factory.task.set")}</Button>
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


/**
 * The card goal, often an issue body, read as the Markdown it is written in
 * with the renderer an issue's body uses; a goal the engine shortened ends in
 * the muted cue, since the rest is kept only in its issue.
 */
function Goal({ text }: { text: string }) {
  const { t } = useInterfaceTranslation();
  const pieces = splitAtCuts(text);
  return (
    <div className="flex min-w-0 flex-col gap-xs" data-factory-goal="true">
      <MarkdownText text={pieces.join("\n")} />
      {pieces.length > 1 ? <span className="text-caption text-muted-foreground" data-factory-cut="true">… {t("factory.task.cut")}</span> : null}
    </div>
  );
}

/** A text the engine shortened shows a muted cue where each `[cut N bytes]` mark stood. */
function ShortenedText({ text, className }: { text: string; className?: string }) {
  const { t } = useInterfaceTranslation();
  const pieces = splitAtCuts(text);
  return (
    <span className={className}>
      {pieces.map((piece, at) => (
        <Fragment key={at}>
          {at > 0 ? <span className="text-muted-foreground" data-factory-cut="true"> … {t("factory.task.cut")} </span> : null}
          {piece}
        </Fragment>
      ))}
    </span>
  );
}


/** Why the Task stopped or paused, in red when a person has to act, with Factory AI's reading under it (B23, D-25, D-26). */
function StopLine({ detail }: { detail: TaskDetail }) {
  const { t } = useInterfaceTranslation();
  const card = detail.card;
  if (card.state === "paused" && card.pause_reason) {
    return (
      <p className="flex items-center gap-xs text-body text-subtle-foreground" data-factory-pause-reason={card.pause_reason}>
        <CirclePauseIcon aria-hidden="true" className="size-(--size-icon) shrink-0" />
        {t(PAUSE_REASON_LABEL[card.pause_reason])}
      </p>
    );
  }
  if (!detail.stop_code) return null;
  const more = detail.stop_code === "worker_gone" ? t("factory.turn.goneWhy") : detail.stop_code === "no_report" ? t("factory.card.noReply") : null;
  return (
    <div className="flex flex-col gap-xxs">
      <p className="flex items-center gap-xs text-body text-destructive" data-factory-stop={detail.stop_code}>
        <CircleAlertIcon aria-hidden="true" className="size-(--size-icon) shrink-0" />
        {[t(STOP_LABEL[detail.stop_code]), more].filter((part) => part !== null).join(" · ")}
      </p>
      {detail.diagnosis ? (
        <p className="flex items-center gap-xs pl-(--size-icon) text-caption text-subtle-foreground" data-factory-diagnosis="true">
          <SparklesIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
          <span className="[overflow-wrap:anywhere]">{[t("factory.task.diagnosis", { text: detail.diagnosis }), detail.diagnosed_from ? t(DIAGNOSIS_SOURCE_LABEL[detail.diagnosed_from]) : null].filter((part) => part !== null).join(" · ")}</span>
        </p>
      ) : null}
    </div>
  );
}

/** A candidate as one line: agent · model · effort, the CLI's own default where none is set. */
function candidateText(candidate: Pick<WorkerCandidate, "agent" | "model" | "effort" | "description">, cliDefault: string): string {
  const label = agentAdapter(candidate.agent)?.label ?? candidate.agent;
  const parts = [label, candidate.model, candidate.effort].filter((part): part is string => !!part);
  return parts.length > 1 ? parts.join(" · ") : `${label} · ${cliDefault}`;
}

function PickedLine({ description, reason }: { description: string; reason: string | null }) {
  const { t } = useInterfaceTranslation();
  return (
    <span className="flex min-w-0 items-center gap-xs text-caption text-subtle-foreground" data-factory-picked="true">
      <SparklesIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
      <span className="[overflow-wrap:anywhere]">{t("factory.task.picked", { description: reason ? `${description} · ${reason}` : description })}</span>
    </span>
  );
}

/**
 * Picking the worker before one starts (D-41): the candidates, Factory AI's
 * pick marked; another choice pins it and Factory AI no longer picks.
 */
function WorkerPick({ factory, detail, actions }: { factory: FactoryView; detail: TaskDetail; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const request = useFactoryRequest(actions);
  const picked = detail.pinned_worker ?? detail.ai_picked_worker ?? 1;
  const ai = detail.ai_picked_worker;
  const task = taskRef(factory.id, detail.card.task);
  return (
    <div className="flex flex-col gap-xs" data-factory-worker-pick={picked}>
      <div className="flex min-w-0 items-center gap-xs">
        <SquareTerminalIcon aria-hidden="true" className="size-(--size-icon) shrink-0 text-muted-foreground" />
        <span className="flex-1">{t("factory.settings.workers")}</span>
        <Select value={String(picked)} onValueChange={(value) => request.send({ verb: "worker", task, worker: value === String(ai) && detail.pinned_worker !== null ? null : Number(value) })}>
          <SelectTrigger size="sm" className="w-[calc(var(--size-settings-control-w)*1.2)] max-w-3/5" aria-label={t("factory.settings.workers")} data-factory-worker-select="true">
            {/* The trigger names the candidate; its line and the AI mark stay in the menu. */}
            <SelectValue>{factory.workers[picked - 1] ? candidateText(factory.workers[picked - 1]!, t("factory.settings.cliDefault")) : null}</SelectValue>
          </SelectTrigger>
          <SelectContent>
            {factory.workers.map((candidate, at) => (
              <SelectItem key={at} value={String(at + 1)} data-factory-worker-option={at + 1}>
                <span className="flex flex-col">
                  <span>{candidateText(candidate, t("factory.settings.cliDefault"))}</span>
                  <span className="text-caption text-muted-foreground">{[candidate.description, at + 1 === ai ? t("factory.task.pickAuto") : null].filter((part) => part).join(" · ")}</span>
                </span>
              </SelectItem>
            ))}
            <p className="px-sm py-xs text-caption text-muted-foreground">{t("factory.task.pickHint")}</p>
          </SelectContent>
        </Select>
      </div>
      {ai !== null && detail.pinned_worker === null ? <PickedLine description={factory.workers[ai - 1]?.description ?? ""} reason={detail.ai_pick_reason} /> : null}
      <span className="text-caption text-muted-foreground">{t("factory.task.pickEffect")}</span>
      <Refusal state={request.state} />
    </div>
  );
}


/** How the engine treated a quiet worker: how long it rested, the one wake, no reply, one diagnosis, the automatic restart (D-22-D-25). */
function RestLine({ detail }: { detail: TaskDetail }) {
  const { t } = useInterfaceTranslation();
  const parts: React.ReactNode[] = [];
  if (detail.resting_since !== null) parts.push(<span key="rest">{t("factory.task.rest")} <Elapsed since={detail.resting_since} /></span>);
  if (detail.woke_at !== null) parts.push(<span key="woke">{t("factory.task.woke")}</span>);
  if (detail.woke_at !== null && detail.stop_code === "no_report") parts.push(<span key="reply" className="text-destructive">{t("factory.task.noReply")}</span>);
  if (detail.diagnosed_at !== null) parts.push(<span key="diagnosed">{t("factory.task.diagnosed")}</span>);
  if (detail.auto_restarts > 0) parts.push(<span key="restarts">{t("factory.task.restarted", { count: detail.auto_restarts })}</span>);
  if (parts.length === 0) return null;
  return (
    <span className="flex min-w-0 flex-wrap items-center gap-md text-caption text-muted-foreground" data-factory-rest="true">
      {parts}
    </span>
  );
}
