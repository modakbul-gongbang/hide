import { useMemo, useRef } from "react";
import { CircleIcon, CircleDotIcon, CircleCheckIcon, CircleHelpIcon, CirclePauseIcon, CircleDashedIcon, GitMergeIcon, GitPullRequestIcon, LoaderCircleIcon, LockIcon, TriangleAlertIcon, SparklesIcon } from "lucide-react";
import type { Actions } from "../actions";
import { statusText } from "../agentStatus";
import { AgentLogo } from "../components/agent-logo";
import { DescendantBadge } from "../components/agent-row";
import { Elapsed } from "../components/elapsed";
import { StatusMark } from "../components/status-mark";
import { Button } from "../components/ui/button";
import { useInterfaceTranslation } from "../i18n/client";
import { cn } from "../lib/utils";
import { localDeviceId } from "../snapshot";
import { useShellStore } from "../store";
import { useUiStore } from "../ui";
import { ACTION_LABEL, GATE_LABEL, STATE_LABEL, STOP_LABEL, TONE_TEXT, stateTone, waitingText } from "./labels";
import type { CardView, FactoryView, InboxItem, TaskState } from "./model";
import { choiceCommand, itemChoices, Refusal } from "./MyTurn";
import { useFactoryRequest } from "./request";
import { inboxKey } from "./view";
import "./cards.css";

const STATE_ICON: Partial<Record<TaskState, typeof CircleIcon>> = {
  drafting: CircleDashedIcon, running: CircleDotIcon, verifying: LoaderCircleIcon,
  relanding: CircleDotIcon, outside: CircleDotIcon, blocked: CircleHelpIcon,
  stopped: CirclePauseIcon, paused: CirclePauseIcon, merge_waiting: GitMergeIcon,
  done: CircleCheckIcon, landed: CircleCheckIcon,
};

export function stateIcon(state: TaskState): typeof CircleIcon {
  return STATE_ICON[state] ?? CircleIcon;
}

export function StateMark({ card, className }: { card: CardView; className?: string }) {
  const { t } = useInterfaceTranslation();
  const Icon = stateIcon(card.state);
  const words = card.stop ? `${t(STATE_LABEL[card.state])} · ${t(STOP_LABEL[card.stop])}` : t(STATE_LABEL[card.state]);
  return <span className={cn("flex min-w-0 items-center gap-xxs text-caption", TONE_TEXT[stateTone(card.state, card.needs_person)], className)} data-factory-state={card.state} data-factory-stop-reason={card.stop ?? undefined}>
    <Icon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" /><span className="truncate">{words}</span>
  </span>;
}

/** Card navigation and sibling controls avoid nested buttons. Its own width selects the information tier. */
export function TaskCardView({ factory, card, showProject, dim = false, actions, item }: {
  factory: FactoryView; card: CardView; showProject: boolean; dim?: boolean; actions?: Actions; item?: InboxItem;
}) {
  const { t, i18n } = useInterfaceTranslation();
  const main = useRef<HTMLButtonElement>(null);
  const agents = useShellStore((state) => state.agents);
  const navigator = useShellStore((state) => state.rest?.navigator);
  const localDevice = useShellStore((state) => localDeviceId(state.rest));
  const localProject = navigator?.workspaces?.find((project) => project.device_id === localDevice && project.path === factory.project);
  const localIssue = localProject?.tasks?.tasks.find((issue) => issue.source === "local" && issue.id === card.issue);
  const summaries = useShellStore((state) => state.rest?.ui_state?.agent_summary !== false);
  const byPane = useMemo(() => new Map(agents.map((agent) => [agent.pane_id, agent])), [agents]);
  const worker = card.worker_pane ? byPane.get(card.worker_pane) : undefined;
  const children = (worker?.lineage_child_pane_ids ?? []).flatMap((pane) => {
    const child = byPane.get(pane);
    return child ? [child] : [];
  });
  const descendants = worker?.close_descendant_pane_ids?.length ?? children.length;
  const line = summaries ? worker?.request?.line : undefined;
  const person = card.waiting_group === "person" || card.needs_person;
  const resting = card.resume_at !== null;
  const Icon = resting ? CirclePauseIcon : stateIcon(card.state);
  const state = resting ? t("factory.card.resting") : card.stop ? t(STOP_LABEL[card.stop]) : t(STATE_LABEL[card.state]);
  const tone = card.state === "stopped" ? "text-destructive" : card.state === "outside" ? "text-muted-foreground" : TONE_TEXT[stateTone(card.state, person)];
  const problem = card.state === "stopped" && card.stop ? t(STOP_LABEL[card.stop])
    : card.state === "merge_waiting" && item ? item.gates.map((gate) => t(GATE_LABEL[gate])).join(" · ")
    : resting ? t("factory.card.resumeAt", { time: new Date(card.resume_at!).toLocaleTimeString(i18n.language, { hour: "2-digit", minute: "2-digit" }) })
    : card.state === "outside" && card.pr ? t("factory.card.outsidePr", { number: card.pr.number })
    : card.failures > 0 ? t("factory.card.failures", { count: card.failures })
    : card.waiting_code === "predecessors" ? waitingText(card, t) : null;
  const ProblemIcon = resting ? CirclePauseIcon : card.state === "outside" ? GitPullRequestIcon : card.failures > 0 || card.state === "stopped" ? TriangleAlertIcon : LockIcon;
  const open = () => useUiStore.getState().setFactoryPlace({ task: { factory: factory.id, task: card.task } });
  return <div className="factory-card-container">
    <article className={cn("factory-card relative min-w-0 overflow-hidden rounded-md border border-border bg-card text-muted-foreground hover:bg-accent", (dim || card.state === "done") && "opacity-(--opacity-dimmed)")} data-factory-card={card.task} data-factory-card-turn={person ? "true" : undefined}>
      <button ref={main} type="button" className="absolute inset-0 rounded-md outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring" data-factory-card-open="true" aria-label={`${card.display_id} ${card.title}, ${state}`} onClick={open} />
      {person ? <span role="img" aria-label={t("factory.board.person")} className={cn("factory-card-band pointer-events-none absolute inset-y-0 left-0", card.state === "stopped" ? "bg-destructive" : "bg-warning")} /> : null}
      <div className="factory-card-content pointer-events-none relative flex min-w-0 flex-col gap-sm px-md py-sm">
        <div className="flex min-w-0 items-center gap-xs text-caption" data-factory-card-top="true">
          {card.issue ? card.issue_url ? <a className="factory-card-control min-w-0 truncate font-mono hover:underline" href={card.issue_url} target="_blank" rel="noreferrer">{card.issue}</a> : localIssue && localProject && actions ? <button type="button" className="factory-card-control min-w-0 truncate font-mono hover:underline" onClick={() => actions.openOverview(localProject.device_id, localProject.id, { issue: localIssue.key })}>{card.issue}</button> : <span className="min-w-0 truncate font-mono">{card.issue}</span> : null}
          {card.pr ? <a className="factory-card-control flex min-w-0 items-center gap-xxs font-mono hover:underline" href={card.pr.url} target="_blank" rel="noreferrer" aria-label={`PR ${card.pr.number}`}><GitPullRequestIcon className="size-(--size-icon-sm) shrink-0" /><span className="truncate">{card.pr.number}</span></a> : null}
          <span className="flex-1" />
          {card.worker_runtime ? <span role="img" aria-label={card.worker_runtime} title={card.worker_runtime} className="factory-card-logo shrink-0"><AgentLogo agent={card.worker_runtime} label={card.worker_runtime} /></span> : null}
          {worker && descendants > 0 && children.length > 0 && actions ? <span className="factory-card-normal factory-card-control"><DescendantBadge agent={worker} descendants={descendants} childRows={children} onOpenChild={(pane) => actions.openAgent(pane)} onUnfold={null} returnFocus={() => main.current?.focus({ preventScroll: true })} /></span> : null}
          <Elapsed since={card.since} className="factory-card-normal shrink-0 text-caption" />
        </div>
        <div className="flex min-w-0 items-start gap-xs" data-factory-card-title-line="true">
          <span role="img" aria-label={state} title={state} data-factory-state={card.state} data-factory-stop-reason={card.stop ?? undefined} className={cn("mt-xxs shrink-0", tone)}><Icon aria-hidden="true" className="size-(--size-icon)" /></span>
          <span className="factory-card-title min-w-0 flex-1 text-subhead font-semibold text-foreground">{card.title}</span>
          <StageBar card={card} />
        </div>
        <p className="factory-card-normal factory-card-summary text-body text-subtle-foreground" title={card.summary}>{card.summary}</p>
        {problem ? <div className={cn("factory-card-normal flex min-w-0 items-center gap-xs text-caption", card.state === "stopped" ? "text-destructive" : card.failures > 0 || card.state === "merge_waiting" ? "text-warning" : "text-muted-foreground")} data-factory-problem="true" title={problem}><ProblemIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" /><span className="truncate">{problem}</span></div> : null}
        {person && item && actions && !resting ? <CardAction key={inboxKey(item)} card={card} item={item} actions={actions} onDetail={open} /> : null}
        <div className="factory-card-wide flex min-w-0 items-center gap-md text-caption"><span>{state}</span>{worker ? <span className="flex min-w-0 items-center gap-xxs"><StatusMark symbol={worker.symbol} className="text-muted-foreground" /><span className="truncate">{statusText(t, worker.status_code)}</span></span> : null}{showProject ? <span className="truncate">{factory.project_name}</span> : null}</div>
        {line ? <div className="factory-card-wide flex min-w-0 items-center gap-xs border-t border-dashed border-border pt-sm text-caption" data-factory-ai="true"><SparklesIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" /><span className="truncate" title={line}>{line}</span></div> : null}
      </div>
    </article>
  </div>;
}

function StageBar({ card }: { card: CardView }) {
  const { t } = useInterfaceTranslation();
  const names = [t("factory.card.stage.waiting"), t("factory.card.stage.work"), t("factory.card.stage.verify"), t("factory.card.stage.merge")];
  return <span role="img" aria-label={card.stage === 4 ? t("factory.state.done") : t("factory.card.stage", { stage: names[card.stage]! })} className="factory-card-normal factory-stage-bar shrink-0 self-center" data-factory-stage={card.stage}>
    {names.map((name, index) => <span key={name} title={name} className={cn("factory-stage-cell", index < card.stage ? "factory-stage-complete" : index === card.stage ? card.column === "stuck" ? "factory-stage-stuck" : "factory-stage-current" : "")} />)}
  </span>;
}

function CardAction({ card, item, actions, onDetail }: { card: CardView; item: InboxItem; actions: Actions; onDetail: () => void }) {
  const { t } = useInterfaceTranslation();
  const request = useFactoryRequest(actions);
  const busy = request.state.phase === "sending" || request.state.phase === "taken";
  const actionItem: InboxItem = card.state === "stopped" ? { ...item, kind: "stopped", suggestion: "retry" } : item;
  const verb = actionItem.kind === "merge" || actionItem.kind === "stopped";
  const choices = itemChoices(item).filter((choice) => !choice.own);
  const send = (value: string) => {
    if (busy) return;
    const command = choiceCommand(actionItem, { value, own: false }, "");
    if (command) request.send(command);
  };
  const primary = verb ? t(ACTION_LABEL[actionItem.suggestion] ?? "factory.turn.send") : item.suggestion;
  return <div className={cn("factory-card-normal flex min-w-0 flex-col gap-sm", !verb && "rounded-sm bg-secondary p-sm")}>
    {!verb ? <p className="factory-card-question text-body font-semibold text-foreground">{item.text}</p> : null}
    <div className="factory-card-control flex min-w-0 flex-wrap items-center gap-xs">
      <Button size="sm" variant="secondary" className="factory-card-action" disabled={busy} onClick={() => send(actionItem.suggestion)} data-factory-card-send="true">{busy ? t("factory.turn.sending") : primary}</Button>
      {!verb ? <>{choices.filter((choice) => choice.value !== item.suggestion).map((choice) => <Button key={choice.value} size="sm" variant="outline" className="factory-card-wide factory-card-action" disabled={busy} onClick={() => send(choice.value)}>{choice.value}</Button>)}<Button size="sm" variant="ghost" className="factory-card-action" disabled={busy} onClick={() => useUiStore.getState().setFactoryPlace({ tab: "turn", task: null, focus: inboxKey(item) })}>{t("factory.card.otherAnswer")}</Button></> : item.kind === "merge" && card.pr ? <a className="factory-card-wide rounded-sm border border-border px-sm py-xs text-caption hover:underline" href={card.pr.url} target="_blank" rel="noreferrer">{t("factory.card.viewPr")}</a> : <Button size="sm" variant="outline" className="factory-card-wide factory-card-action" onClick={onDetail}>{t("factory.card.history")}</Button>}
    </div>
    <Refusal state={request.state} />
  </div>;
}
