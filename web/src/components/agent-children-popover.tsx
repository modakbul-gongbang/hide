import { ArrowRightIcon, ListTreeIcon } from "lucide-react";
import { AgentMark } from "../AgentMark";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { branchChip, markTone } from "../agentRow";
import { useInterfaceTranslation } from "../i18n/client";
import { statusText } from "../agentStatus";
import type { AgentRow } from "../snapshot";
import { Command, CommandGroup, CommandItem, CommandList, CommandSeparator } from "./ui/command";
import { Kbd } from "./ui/kbd";
import { StatusMark } from "./status-mark";
import { DeviceChip } from "./device-chip";
import { Elapsed } from "./elapsed";
import { Popover, PopoverContent, PopoverTrigger } from "./ui/popover";
import { useUiStore } from "../ui";
import { useShellStore } from "../store";
import { relationState } from "../lineage";
import { Hint } from "./ui/tooltip";

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
  triggerLabel,
}: {
  parent: AgentRow;
  childRows: AgentRow[];
  onOpenChild: (paneId: string) => void;
  onUnfold: (() => void) | null;
  /** The row control focus goes back to when the list closes. */
  returnFocus: () => void;
  trigger: ReactNode;
  triggerLabel: string;
}) {
  const { t } = useInterfaceTranslation();
  const [open, setOpen] = useState(false);
  const [opening, setOpening] = useState<string | null>(null);
  const relation = useUiStore((state) => state.relation);
  const outcome = useShellStore((state) => state.rest?.status?.pane_focus_request);
  const tracked = relation?.sourcePaneId === parent.pane_id && relation.targetPaneId === opening ? relation : null;
  const progress = relationState(tracked, outcome, t);
  const list = useRef<HTMLDivElement>(null);
  const empty = childRows.length === 0;
  useEffect(() => {
    if (open && empty) setOpen(false);
  }, [open, empty]);
  useEffect(() => {
    if (opening && tracked && !progress) {
      setOpen(false);
      setOpening(null);
    }
  }, [opening, tracked, progress]);
  const choose = (action: () => void) => {
    setOpen(false);
    action();
  };
  return (
    <Popover open={open && !empty} onOpenChange={setOpen}>
      <Hint label={triggerLabel}><PopoverTrigger asChild>{trigger}</PopoverTrigger></Hint>
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
        <Command ref={list} tabIndex={-1} label={t("agents.children.label", { name: parent.identity_label })} className="outline-none">
          <div className="flex items-center gap-sm border-b border-border px-md py-sm text-caption">
            <span className="flex-1 font-medium text-subtle-foreground">{t("agents.children.title", { count: childRows.length })}</span>
            <span className="inline-flex items-center gap-xxs text-muted-foreground">
              <Kbd>Enter</Kbd> {t("agents.children.goTo")}
            </span>
          </div>
          <CommandList>
            <CommandGroup>
              {childRows.map((child) => (
                <ChildItem key={child.pane_id} parent={parent} child={child} pending={progress?.phase === "pending"} onOpen={() => {
                  onOpenChild(child.pane_id);
                  const request = useUiStore.getState().relation;
                  if (request?.sourcePaneId === parent.pane_id && request.targetPaneId === child.pane_id) setOpening(child.pane_id);
                  else setOpen(false);
                }} />
              ))}
            </CommandGroup>
            {onUnfold ? (
              <>
                <CommandSeparator />
                <CommandItem value="__unfold" onSelect={() => choose(onUnfold)} data-agent-children-unfold="true" className="text-subtle-foreground">
                  <ListTreeIcon />
                  <span className="flex-1">{t("agentSessions.allChildren")}</span>
                  <ArrowRightIcon />
                </CommandItem>
              </>
            ) : null}
          </CommandList>
          {progress ? <div className="flex items-center gap-xs border-t border-border px-md py-sm text-caption" role={progress.phase === "failed" ? "alert" : "status"} data-child-navigation={progress.phase}>
            <span className={`min-w-0 flex-1 truncate ${progress.phase === "failed" ? "text-destructive" : "text-muted-foreground"}`}>{progress.phase === "pending" ? t("panes.relation.opening", { name: tracked!.label }) : progress.message}</span>
            {progress.phase === "failed" && progress.retryable ? <button type="button" onClick={() => onOpenChild(opening!)} className="shrink-0 rounded-xs px-xs outline-none focus-visible:ring-1 focus-visible:ring-ring">{t("common.retry")}</button> : null}
          </div> : null}
        </Command>
      </PopoverContent>
    </Popover>
  );
}

function ChildItem({ parent, child, pending, onOpen }: { parent: AgentRow; child: AgentRow; pending: boolean; onOpen: () => void }) {
  const { t } = useInterfaceTranslation();
  const branch = branchChip(child);
  const tone = markTone(child);
  return (
    <CommandItem value={child.pane_id} onSelect={onOpen} disabled={pending} data-agent-child={child.pane_id} className="group/child items-start">
      <StatusMark symbol={child.symbol} className={`mt-xxs ${tone}`} />
      <AgentMark kind={child.agent_kind} />
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="flex items-baseline gap-xs">
          <span className="min-w-0 flex-1 truncate font-medium text-foreground" title={child.identity_label}>{child.identity_label}</span>
          <Elapsed since={child.state.request_since} className="shrink-0 text-micro text-muted-foreground" />
        </span>
        <span className="flex min-w-0 items-center gap-xs text-caption">
          <span className={`shrink-0 ${tone}`}>{statusText(t, child.status_code)}</span>
          {branch ? (
            <span className="min-w-0 truncate font-mono text-muted-foreground" data-branch-chip={branch}>
              {branch}
            </span>
          ) : null}
          {child.device_id !== parent.device_id && child.device_label ? <DeviceChip label={child.device_label} className="max-w-2/5" /> : null}
          {child.request?.pull_requests.filter((pull) => pull.live).slice(0, 1).map((pull) => <span key={pull.url} className="shrink-0 text-muted-foreground" title={pull.title}>#{pull.number}</span>)}
        </span>
        {child.request?.line ? <span className="truncate text-caption text-muted-foreground" title={child.request.line}>{child.request.line}</span> : null}
      </span>
      <button
        type="button"
        tabIndex={-1}
        disabled={pending}
        aria-label={t("agents.children.openChild", { name: child.identity_label })}
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
