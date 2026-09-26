import { ArrowRightIcon, ListTreeIcon } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { branchChip, markTone } from "../agentRow";
import type { AgentRow } from "../snapshot";
import { Command, CommandGroup, CommandItem, CommandList, CommandSeparator } from "./ui/command";
import { Kbd } from "./ui/kbd";
import { StatusMark } from "./status-mark";
import { Popover, PopoverContent, PopoverTrigger } from "./ui/popover";

/**
 * The list a descendant badge opens (PRD sidebar-agent-status D-03, D-04, B5,
 * B6): the row's direct children with their mark, name, status word, branch
 * when it differs, and elapsed time. Arrow keys move the highlight, Enter or
 * a row's arrow opens that child's pane, and the last item unfolds the
 * children in the list. It offers no Stop: stopping is irreversible and
 * needs its own confirmed flow. Grandchildren are counted on the badge and
 * appear under their own parent.
 *
 * `children` is read from the live rows on every render, so a child that
 * leaves the projection leaves the list, and the list closes when none is
 * left. Esc closes it and hands focus back to the row it belongs to.
 */
export function AgentChildrenPopover({
  parent,
  childRows,
  onOpenChild,
  onUnfold,
  returnFocus,
  trigger,
}: {
  parent: AgentRow;
  childRows: AgentRow[];
  onOpenChild: (paneId: string) => void;
  onUnfold: (() => void) | null;
  /** The row control focus goes back to when the list closes. */
  returnFocus: () => void;
  trigger: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const list = useRef<HTMLDivElement>(null);
  const empty = childRows.length === 0;
  useEffect(() => {
    if (open && empty) setOpen(false);
  }, [open, empty]);
  const choose = (action: () => void) => {
    setOpen(false);
    action();
  };
  return (
    <Popover open={open && !empty} onOpenChange={setOpen}>
      <PopoverTrigger asChild>{trigger}</PopoverTrigger>
      <PopoverContent
        align="start"
        className="w-(--size-agent-children-popover) p-none"
        data-agent-children={parent.pane_id}
        onOpenAutoFocus={(event) => {
          event.preventDefault();
          list.current?.focus();
        }}
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          returnFocus();
        }}
      >
        <Command ref={list} tabIndex={-1} label={`${parent.identity_label}의 하위 에이전트`} className="outline-none">
          <div className="flex items-center gap-sm border-b border-border px-md py-sm text-caption">
            <span className="flex-1 font-medium text-subtle-foreground">하위 에이전트 {childRows.length}</span>
            <span className="inline-flex items-center gap-xxs text-muted-foreground">
              <Kbd>Enter</Kbd> 이동
            </span>
          </div>
          <CommandList>
            <CommandGroup>
              {childRows.map((child) => (
                <ChildItem key={child.pane_id} child={child} onOpen={() => choose(() => onOpenChild(child.pane_id))} />
              ))}
            </CommandGroup>
            {onUnfold ? (
              <>
                <CommandSeparator />
                <CommandItem value="__unfold" onSelect={() => choose(onUnfold)} data-agent-children-unfold="true" className="text-subtle-foreground">
                  <ListTreeIcon />
                  <span className="flex-1">목록에서 펼치기</span>
                  <ArrowRightIcon />
                </CommandItem>
              </>
            ) : null}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}

function ChildItem({ child, onOpen }: { child: AgentRow; onOpen: () => void }) {
  const branch = branchChip(child);
  const tone = markTone(child);
  return (
    <CommandItem value={child.pane_id} onSelect={onOpen} data-agent-child={child.pane_id} className="group/child items-start">
      <StatusMark symbol={child.symbol} className={`mt-xxs ${tone}`} />
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="flex items-baseline gap-xs">
          <span className="min-w-0 flex-1 truncate font-medium text-foreground">{child.identity_label}</span>
          <span className="shrink-0 text-micro text-muted-foreground">{child.elapsed}</span>
        </span>
        <span className="flex min-w-0 items-center gap-xs text-caption">
          <span className={`shrink-0 ${tone}`}>{child.status_label}</span>
          {branch ? (
            <span className="min-w-0 truncate font-mono text-muted-foreground" data-branch-chip={branch}>
              {branch}
            </span>
          ) : null}
        </span>
      </span>
      <button
        type="button"
        tabIndex={-1}
        aria-label={`${child.identity_label} 열기`}
        data-agent-child-open={child.pane_id}
        className="invisible shrink-0 self-center rounded-xs p-xxs text-subtle-foreground hover:bg-secondary hover:text-foreground group-data-[selected=true]/child:visible"
        onClick={(event) => {
          event.stopPropagation();
          onOpen();
        }}
      >
        <ArrowRightIcon />
      </button>
    </CommandItem>
  );
}
