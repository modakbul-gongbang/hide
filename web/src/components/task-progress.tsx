import { CheckIcon } from "lucide-react";
import type { HTMLAttributes } from "react";
import { markName } from "../agentRow";
import { statusText } from "../agentStatus";
import { useInterfaceTranslation } from "../i18n/client";
import { cn } from "../lib/utils";
import type { TaskState } from "../sessionWork";
import type { ChildProgress, ChildWork, RowMark } from "../snapshot";
import { StatusMark } from "./status-mark";
import { Hint } from "./ui/tooltip";

/**
 * The state a task is drawn in: work in progress only while its agent's row
 * draws a ring; a task the label left in progress on a stopped row is work
 * left, as its row says (status-marks D-17, B9, B34).
 */
export function shownTaskState(state: TaskState, mark: RowMark | null): TaskState {
  return state === "working" && mark !== "going" && mark !== "waiting" ? "unfinished" : state;
}

/**
 * A task's mark, the row's marks (status-marks D-23, B34): a finished task's
 * `✓`, a task in progress the ring its agent's row draws (dashed while the row
 * waits), a question or a block the operator's `!`, and an empty slot for work
 * left and for a task a pull request holds, whose name's small word or PR mark
 * says the state. Each drawn mark says its state in its hint and accessible
 * name with the row's words; an empty slot says nothing a screen reader would
 * hear twice.
 */
export function TaskMark({ state, mark }: { state: TaskState; mark: RowMark | null }) {
  const { t } = useInterfaceTranslation();
  if (state === "unfinished" || state === "review" || state === "ci_failed") return <span aria-hidden="true" data-task-state={state} className="flex size-(--size-icon-sm) shrink-0" />;
  const ring: RowMark = mark === "waiting" ? "waiting" : "going";
  const name = state === "question" || state === "blocked" ? `${markName(t, "turn")} · ${statusText(t, state === "blocked" ? "error" : "question")}`
    : state === "working" ? (ring === "waiting" ? `${markName(t, ring)} · ${statusText(t, "waiting")}` : markName(t, ring))
      : t("sessionWork.done");
  return <Hint label={name}><span role="img" aria-label={name} data-task-state={state} className="flex size-(--size-icon-sm) shrink-0 items-center justify-center">
    {state === "done" ? <CheckIcon aria-hidden="true" className="size-(--size-icon-sm) text-success" /> : state === "working" ? <StatusMark mark={ring} /> : <StatusMark mark="turn" />}
  </span></Hint>;
}

/**
 * A task's name, with its state in small text after it where its slot is
 * empty and no PR mark beside it says it (B34): `일 남음` while work is left,
 * and a pull request's state on a task of a request whose PR mark hangs on
 * its first task (`prShown` false).
 */
export function TaskName({ state, name, prShown, className, ...rest }: { state: TaskState; name: string; prShown: boolean } & HTMLAttributes<HTMLSpanElement>) {
  const { t } = useInterfaceTranslation();
  const word = state === "unfinished" ? statusText(t, "unfinished") : (state === "review" || state === "ci_failed") && !prShown ? t(`sessionWork.${state}`) : null;
  return <span className={cn("min-w-0 flex-1 whitespace-normal [overflow-wrap:anywhere]", className)} {...rest}>
    {name}
    {word ? <span className="ml-xs text-micro text-muted-foreground" data-task-word={state}>{word}</span> : null}
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
