import { CheckIcon } from "lucide-react";
import type { HTMLAttributes } from "react";
import { markName } from "../agentRow";
import { statusText } from "../agentStatus";
import { useInterfaceTranslation } from "../i18n/client";
import { cn } from "../lib/utils";
import type { TaskState } from "../sessionWork";
import type { ChildProgress, ChildWork } from "../snapshot";
import { StatusMark } from "./status-mark";
import { Hint } from "./ui/tooltip";

/**
 * A task's mark, the row's marks (status-marks D-23, B34): a finished task's
 * `✓`, a task in progress the ring its agent's row draws (dashed while the row
 * waits), a question or a block the operator's `!`, and an empty slot for work
 * left (its name says 일 남음) and for a task a pull request holds, whose PR
 * mark says the state. Each says its state in its hint and accessible name.
 */
export function TaskMark({ state, waiting = false }: { state: TaskState; waiting?: boolean }) {
  const { t } = useInterfaceTranslation();
  const turn = state === "question" || state === "blocked";
  const name = turn ? `${markName(t, "turn")} · ${statusText(t, state === "blocked" ? "error" : "question")}` : t(`sessionWork.${state}`);
  return <Hint label={name}><span role="img" aria-label={name} data-task-state={state} className="flex size-(--size-icon-sm) shrink-0 items-center justify-center">
    {state === "done" ? <CheckIcon aria-hidden="true" className="size-(--size-icon-sm) text-success" /> : turn ? <StatusMark mark="turn" /> : state === "working" ? <StatusMark mark={waiting ? "waiting" : "going"} /> : null}
  </span></Hint>;
}

/** A task's name, with `일 남음` after it in small text while work is left (B34). */
export function TaskName({ state, name, className, ...rest }: { state: TaskState; name: string } & HTMLAttributes<HTMLSpanElement>) {
  const { t } = useInterfaceTranslation();
  return <span className={cn("min-w-0 flex-1 whitespace-normal [overflow-wrap:anywhere]", className)} {...rest}>
    {name}
    {state === "unfinished" ? <span className="ml-xs text-micro text-muted-foreground" data-task-left="true">{statusText(t, "unfinished")}</span> : null}
  </span>;
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
  return <span className="flex min-w-0 flex-wrap items-center gap-xs" data-child-progress={`${progress.done}/${count}`}>
    <span>{t("sessionWork.childWork")}</span>
    <span className="text-micro tabular-nums text-muted-foreground">{progress.done}/{count}</span>
    <span className="flex flex-wrap items-center gap-xxs" role="list" aria-label={t("sessionWork.childWork")}>
      {progress.children.map((child, index) => {
        const name = t(`sessionWork.childState.${child}`);
        return <Hint key={index} label={name}><span role="listitem" aria-label={name} data-child-segment={child} className={cn("h-[calc(4*var(--size-hairline))] w-(--size-control-sm) rounded-full", SEGMENT[child])} /></Hint>;
      })}
    </span>
  </span>;
}
