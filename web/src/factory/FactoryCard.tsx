import { CircleIcon, CircleDotIcon, CircleCheckIcon, CircleHelpIcon, CirclePauseIcon, GitMergeIcon, LockIcon } from "lucide-react";
import { Elapsed } from "../components/elapsed";
import { useInterfaceTranslation } from "../i18n/client";
import { cn } from "../lib/utils";
import { useUiStore } from "../ui";
import { STATE_LABEL, STOP_LABEL, TONE_TEXT, stateTone, waitingText } from "./labels";
import type { CardView, FactoryView, TaskState } from "./model";

const STATE_ICON: Partial<Record<TaskState, typeof CircleIcon>> = {
  running: CircleDotIcon,
  verifying: CircleDotIcon,
  relanding: CircleDotIcon,
  outside: CircleDotIcon,
  blocked: CircleHelpIcon,
  stopped: CirclePauseIcon,
  paused: CirclePauseIcon,
  merge_waiting: GitMergeIcon,
  done: CircleCheckIcon,
  landed: CircleCheckIcon,
};

/** A Task's state as a mark and its word, with a stopped card's reason, in the tone the board and graph share (B15, B17). */
export function StateMark({ card, className }: { card: CardView; className?: string }) {
  const { t } = useInterfaceTranslation();
  const tone = stateTone(card.state, card.needs_person);
  const Icon = STATE_ICON[card.state] ?? CircleIcon;
  const words = card.stop ? `${t(STATE_LABEL[card.state])} · ${t(STOP_LABEL[card.stop])}` : t(STATE_LABEL[card.state]);
  return (
    <span className={cn("flex min-w-0 items-center gap-xxs text-caption", TONE_TEXT[tone], className)} data-factory-state={card.state} data-factory-stop-reason={card.stop ?? undefined}>
      <Icon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
      <span className="truncate">{words}</span>
    </span>
  );
}

/**
 * One Task card, on the board and in the graph (B15, B17): its id, project,
 * title and state, what a waiting Task waits for, an unread completion's
 * dot, and the warning border when it is the person's turn. A press opens the
 * Task page.
 */
export function TaskCardView({ factory, card, showProject, dim = false }: { factory: FactoryView; card: CardView; showProject: boolean; dim?: boolean }) {
  const { t } = useInterfaceTranslation();
  const waiting = waitingText(card, t);
  return (
    <button
      type="button"
      className={cn(
        "flex w-full min-w-0 flex-col gap-xxs rounded-md border bg-card px-md py-sm text-left outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring",
        card.needs_person ? "border-warning" : "border-border",
        dim && "opacity-(--opacity-disabled)",
      )}
      aria-label={`${card.display_id} ${card.title}, ${t(STATE_LABEL[card.state])}`}
      data-factory-card={card.task}
      data-factory-card-turn={card.needs_person ? "true" : undefined}
      onClick={() => useUiStore.getState().setFactoryPlace({ task: { factory: factory.id, task: card.task } })}
    >
      <span className="flex min-w-0 items-center gap-xs text-caption text-muted-foreground">
        <span className="shrink-0 font-mono">{card.display_id}</span>
        {showProject ? <span className="min-w-0 truncate">{factory.project_name}</span> : null}
        <span className="flex-1" />
        {card.unread ? <span aria-label={t("factory.card.unread")} role="img" className="size-(--size-tab-status-dot) shrink-0 rounded-full bg-agent-working" data-factory-unread="true" /> : null}
      </span>
      <span className="line-clamp-2 text-body [overflow-wrap:anywhere]">{card.title}</span>
      <span className="flex min-w-0 items-center gap-xs">
        <StateMark card={card} />
        {waiting ? (
          <span className="flex min-w-0 items-center gap-xxs text-caption text-muted-foreground" data-factory-waiting-for={card.waiting_code ?? undefined}>
            <LockIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
            <span className="truncate">{waiting}</span>
          </span>
        ) : null}
        {card.external.length > 0 ? <span className="shrink-0 text-caption text-muted-foreground" data-factory-external="true">{t("factory.card.external")}</span> : null}
        {card.state === "done" || card.state === "landed" ? <Elapsed since={card.since} className="shrink-0 text-caption text-muted-foreground" /> : null}
      </span>
    </button>
  );
}
