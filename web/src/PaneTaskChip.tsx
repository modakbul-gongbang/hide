import { ListChecksIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { Actions } from "./actions";
import { lineText } from "./agentRow";
import { ChildProgressHead, shownTaskState, TaskMark, TaskName } from "./components/task-progress";
import { PrMark } from "./components/pr-mark";
import { Kbd } from "./components/ui/kbd";
import { Popover, PopoverAnchor, PopoverContent } from "./components/ui/popover";
import { useInterfaceTranslation } from "./i18n/client";
import { agentsByPane } from "./paneAgents";
import { usePaneAgents } from "./PaneHeaderBand";
import { PR_LOOK } from "./prMark";
import { groupPrs, orderedOutputs, outputPrState, paneProgress, taskGroup, taskState } from "./sessionWork";
import { commandLabel } from "./shortcutLabels";
import type { AgentRow, WorkProgress } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";

/** How long the pointer may cross the gap between the chip and its popover before the popover closes. */
const LEAVE_MS = 150;

/** The live pane's agent and the progress `paneProgress` finds for it. */
export function usePaneProgress(paneId: string): { agent: AgentRow; progress: WorkProgress } | null {
  const agent = agentsByPane(usePaneAgents()).get(paneId);
  const progress = useShellStore((state) => paneProgress(state, paneId));
  return agent && progress ? { agent, progress } : null;
}

/**
 * The pane head's task chip (PRD task-split-checklist D-05, D-06, B9-B14):
 * `<done>/<tasks>` beside the PR chip, from one task. The pointer or the
 * keyboard's focus on it opens the checklist; a press or ⇧⌘U pins it, and a
 * second press, ⇧⌘U or Esc closes it. Opening never takes the keyboard from
 * the terminal.
 */
export function PaneTaskChip({ paneId, actions }: { paneId: string; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const found = usePaneProgress(paneId);
  const pinned = useUiStore((s) => s.taskListPane === paneId);
  const setPinned = useUiStore((s) => s.setTaskListPane);
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const leave = useRef<number | null>(null);
  const chipRef = useRef<HTMLButtonElement>(null);
  useEffect(() => () => {
    if (leave.current !== null) window.clearTimeout(leave.current);
  }, []);
  // A pin outlives a pane that loses its tasks only as long as the pane does.
  const gone = !found;
  useEffect(() => {
    if (!gone) return;
    if (pinned) setPinned(null);
    // A chip that comes back opens only when the pointer arrives again.
    setHovered(false);
    setFocused(false);
  }, [gone, pinned, setPinned]);
  if (!found) return null;
  const { agent, progress } = found;
  const open = pinned || hovered || focused;
  const enter = () => {
    if (leave.current !== null) window.clearTimeout(leave.current);
    leave.current = null;
    setHovered(true);
  };
  const exit = () => {
    if (leave.current !== null) window.clearTimeout(leave.current);
    leave.current = window.setTimeout(() => setHovered(false), LEAVE_MS);
  };
  const close = () => {
    setPinned(null);
    setHovered(false);
    setFocused(false);
  };
  const label = t("sessionWork.taskChip", { done: progress.done, total: progress.total });
  return (
    <Popover open={open} onOpenChange={(next) => (next ? undefined : close())} modal={false}>
      <PopoverAnchor asChild>
        <button
          ref={chipRef}
          type="button"
          aria-label={label}
          aria-expanded={open}
          aria-pressed={pinned}
          data-pane-task-chip={paneId}
          data-state={open ? "open" : "closed"}
          className="flex h-(--size-sidebar-line-detail) shrink-0 items-center gap-xxs rounded-xs border border-border bg-background px-xs font-mono text-micro text-muted-foreground outline-none hover:bg-popover hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring data-[state=open]:border-ring data-[state=open]:text-foreground"
          onMouseDown={(event) => event.preventDefault()}
          // A second press closes the list outright, though the pointer still rests on the chip.
          onClick={() => (pinned ? close() : setPinned(paneId))}
          onPointerEnter={enter}
          onPointerLeave={exit}
          onFocus={() => setFocused(true)}
          onBlur={() => setFocused(false)}
        >
          <ListChecksIcon aria-hidden="true" className="size-(--size-icon-sm)" />
          <span className="tabular-nums">{progress.done}/{progress.total}</span>
        </button>
      </PopoverAnchor>
      <PopoverContent
        side="bottom"
        align="start"
        collisionPadding={8}
        className="max-h-(--radix-popover-content-available-height) overflow-y-auto p-0 text-caption"
        data-pane-task-list={paneId}
        onOpenAutoFocus={(event) => event.preventDefault()}
        onCloseAutoFocus={(event) => event.preventDefault()}
        onPointerEnter={enter}
        onPointerLeave={exit}
        // The chip is outside the list, so its own press would dismiss the
        // list before its click toggles the pin; the click alone decides.
        onInteractOutside={(event) => {
          if (event.target instanceof Node && chipRef.current?.contains(event.target)) event.preventDefault();
        }}
        onEscapeKeyDown={(event) => {
          // Esc closes a pinned list and stops there, so an agent CLI in the
          // focused terminal is not interrupted by it; a list the pointer
          // only shows leaves Esc to the terminal.
          if (!pinned && !focused) return event.preventDefault();
          event.stopPropagation();
          close();
        }}
      >
        <TaskList agent={agent} progress={progress} onRecord={() => {
          close();
          actions.openSessionRecord(progress.session);
        }} />
      </PopoverContent>
    </Popover>
  );
}

/** The checklist: the tasks with their marks and pull requests, the children's progress, the current line, and `기록 열기`. */
function TaskList({ agent, progress, onRecord }: { agent: AgentRow; progress: WorkProgress; onRecord: () => void }) {
  const { t } = useInterfaceTranslation();
  const line = agent.state.line;
  // A request's pull requests show once, on its first task, as in the record.
  const shownGroups = new Set<string>();
  return (
    <div className="flex min-w-0 flex-col [word-break:keep-all]">
      <div className="px-sm pt-sm">
        <h3 className="mb-xs flex items-baseline gap-xs font-medium text-subtle-foreground">
          {t("sessionWork.tasks")}
          <span className="text-micro tabular-nums text-muted-foreground">{progress.done}/{progress.total}</span>
        </h3>
        <ul className="flex flex-col">
          {progress.tasks.map((task) => {
            const group = taskGroup(task.id);
            const prs = shownGroups.has(group) ? [] : orderedOutputs(groupPrs(task, progress.outputs));
            shownGroups.add(group);
            const state = shownTaskState(taskState(task, progress.outputs), agent.mark);
            return (
              <li key={task.id} className="flex min-w-0 items-start gap-xs py-xxs" data-task-list-item={task.id}>
                <span className="mt-xxs flex"><TaskMark state={state} mark={agent.mark} /></span>
                <TaskName state={state} name={task.name} prShown={prs.length > 0} className={state === "done" ? "text-subtle-foreground" : "text-foreground"} data-task-list-name="true" />
                {prs.map((output) => {
                  const look = outputPrState(output.state);
                  return <span key={output.url} role="img" aria-label={`#${output.number} ${t(PR_LOOK[look].word)}`} title={output.title} className="shrink-0"><PrMark state={look} number={output.number} stale={output.stale} /></span>;
                })}
              </li>
            );
          })}
        </ul>
        {progress.children ? <div className="mt-xs text-subtle-foreground" data-task-list-children="true"><ChildProgressHead progress={progress.children} /></div> : null}
        {line && line.text ? (
          <p className="mt-xs flex min-w-0 items-start gap-xs text-muted-foreground" data-task-list-line="true">
            <span className="min-w-0 flex-1 whitespace-normal [overflow-wrap:anywhere]">{lineText(t, agent, line)}</span>
          </p>
        ) : null}
      </div>
      <div className="mt-sm border-t border-border p-xxs">
        <button type="button" className="flex w-full items-center justify-between gap-sm rounded-xs px-xs py-xs text-left outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring" onClick={onRecord} data-task-list-record="true">
          <span>{t("sessionWork.openRecord")}</span>
          <Kbd>{commandLabel("toggle_task_list")}</Kbd>
        </button>
      </div>
    </div>
  );
}
