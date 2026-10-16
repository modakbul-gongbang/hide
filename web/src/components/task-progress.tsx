import { CheckIcon } from "lucide-react";
import { useInterfaceTranslation } from "../i18n/client";
import { cn } from "../lib/utils";
import type { TaskState } from "../sessionWork";
import type { ChildProgress, ChildWork } from "../snapshot";
import { Hint } from "./ui/tooltip";

/** A task's mark: its colour is its state, and the state's name is in its hint and accessible name. */
const GLYPH: Record<TaskState, string> = { done: "text-success", working: "text-agent-working", review: "text-success", ci_failed: "text-success", unfinished: "text-warning", blocked: "text-warning", question: "text-warning" };

export function TaskMark({ state }: { state: TaskState }) {
  const { t } = useInterfaceTranslation();
  const name = t(`sessionWork.${state}`);
  return <Hint label={name}><span role="img" aria-label={name} data-task-state={state} className={cn("flex size-(--size-icon-sm) shrink-0 items-center justify-center", GLYPH[state])}>
    {state === "done" ? <CheckIcon aria-hidden="true" className="size-(--size-icon-sm)" /> : state === "blocked" ? <span aria-hidden="true">⛔</span> : state === "question" ? <span aria-hidden="true" className="font-semibold">?</span> : <span aria-hidden="true">◐</span>}
  </span></Hint>;
}

const SEGMENT: Record<ChildWork, string> = { done: "bg-success", working: "bg-agent-working", other: "bg-muted-foreground", unjudged: "bg-muted" };

/**
 * `자식 일 4/5` and one segment per direct child (PRD task-split-checklist
 * B7, B8): done green, working blue, the rest grey. While any child has no
 * judgment the fraction and the segments give way to the plain count, so a
 * missing judgment never reads as not done.
 */
export function ChildProgressHead({ progress }: { progress: ChildProgress }) {
  const { t } = useInterfaceTranslation();
  const count = progress.children.length;
  if (progress.done === null) {
    return <span className="flex min-w-0 items-baseline gap-xs" data-child-progress="count">
      <span>{t("sessionWork.children")}</span>
      <span className="text-micro tabular-nums text-muted-foreground">{count}</span>
    </span>;
  }
  return <span className="flex min-w-0 items-center gap-xs" data-child-progress={`${progress.done}/${count}`}>
    <span>{t("sessionWork.childWork")}</span>
    <span className="text-micro tabular-nums text-muted-foreground">{progress.done}/{count}</span>
    <span className="flex items-center gap-xxs" role="list" aria-label={t("sessionWork.childWork")}>
      {progress.children.map((child, index) => {
        const name = t(`sessionWork.childState.${child}`);
        return <Hint key={index} label={name}><span role="listitem" aria-label={name} data-child-segment={child} className={cn("h-[calc(4*var(--size-hairline))] w-(--size-control-sm) rounded-full", SEGMENT[child])} /></Hint>;
      })}
    </span>
  </span>;
}
