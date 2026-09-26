import { ChevronDownIcon, ChevronUpIcon, FolderIcon, XIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { Actions } from "./actions";
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from "./components/ui/alert-dialog";
import { Button } from "./components/ui/button";
import { Hint } from "./components/ui/tooltip";
import { Input } from "./components/ui/input";
import { useShellStore } from "./store";
import { focusTerminal } from "./terminals";
import { useUiStore } from "./ui";
import { markTone } from "./agentRow";
import { StatusMark } from "./components/status-mark";
import { Kbd } from "./components/ui/kbd";
import { hostKind } from "./host";
import { visibleWindow, type CycleItem } from "./recent";
import { displayCommand, hostRegistry } from "./shortcuts";
import { displayMark } from "./ViewAreas";
import { AgentMark } from "./AgentMark";
import { knownProvider } from "./workspace";

/**
 * Recent Panels or Recent Projects while the chord's modifier is held: at
 * most nine rows around the highlight, which is what release commits
 * (docs/UI_BEHAVIOR.md, Recent navigation).
 */
export function CycleOverlay() {
  const cycle = useUiStore((s) => s.cycle);
  const registry = useShellStore((s) => hostRegistry(s.rest?.ui_state, hostKind()).registry);
  if (!cycle) return null;
  const { start, rows } = visibleWindow(cycle.items, cycle.index);
  const title = cycle.kind === "panels" ? "Recent Panels" : "Recent Projects";
  const chord = displayCommand(cycle.kind === "panels" ? "recent_panel" : "recent_project", hostKind(), registry);
  return (
    <div className="absolute inset-x-0 top-[var(--size-tab-strip)] z-30 flex justify-center" data-cycle={cycle.kind}>
      <div role="listbox" aria-label={title} className="w-[var(--size-pr-popover)] rounded-md border border-border bg-popover py-xs shadow-lg">
        <div className="flex items-center justify-between px-md pb-xxs">
          <span className="text-caption font-semibold uppercase text-muted-foreground">{title}</span>
          {chord ? <Kbd>{chord}</Kbd> : null}
        </div>
        {rows.map((item, offset) => {
          const selected = start + offset === cycle.index;
          return (
            <div
              key={item.key}
              role="option"
              data-cycle-row={item.surface?.id ?? item.key}
              data-cycle-kind={item.kind}
              aria-selected={selected}
              aria-label={[item.title, item.agent && `${item.agent.agent_kind} agent`, item.agent?.status_label, item.detail].filter(Boolean).join(", ")}
              className={`flex items-center gap-sm px-md py-xxs ${selected ? "bg-secondary text-foreground" : "text-subtle-foreground"}`}
            >
              <CycleMarks item={item} />
              <span className="flex min-w-0 flex-1 flex-col">
                <span className="truncate text-body">{item.title}</span>
                <span className="truncate text-caption text-muted-foreground">{item.detail}</span>
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

/**
 * A row's marks, in the sidebar agent row's order and spacing: the one
 * agent's status mark, never its colour alone, then which agent it is; a
 * tab with no single agent wears the neutral mark the tab strip draws. Other
 * rows keep the empty status slot, so every title starts in one column.
 */
function CycleMarks({ item }: { item: CycleItem }) {
  return (
    <span className="flex shrink-0 items-center gap-xs" data-cycle-marks={item.agent ? (knownProvider(item.agent.agent_kind) ?? "neutral") : item.kind}>
      <span className="flex w-(--size-agent-mark) shrink-0 justify-center">
        {item.agent ? <StatusMark symbol={item.agent.symbol} className={markTone(item.agent)} data-cycle-status={item.agent.status_label} /> : null}
      </span>
      <span className="flex w-(--size-agent-badge-compact) shrink-0 justify-center">
        <KindMark item={item} />
      </span>
    </span>
  );
}

function KindMark({ item }: { item: CycleItem }) {
  if (item.kind === "herdr") return <AgentMark kind={item.agent?.agent_kind} />;
  if (item.kind === "project") return <FolderIcon aria-hidden="true" className="size-(--size-icon) text-muted-foreground" />;
  return displayMark({ kind: item.kind, label: item.title });
}

/** The Swift consequence sheet: Keep open, or stop the work and close. */
export function ConfirmClose({ actions }: { actions: Actions }) {
  const pending = useUiStore((s) => s.pendingClose);
  return (
    <AlertDialog open={pending != null} onOpenChange={(open) => { if (!open) actions.keepOpen(); }}>
      {pending ? (
        <AlertDialogContent data-confirm-close={pending.kind}>
          <AlertDialogHeader>
            <AlertDialogTitle>{pending.title}</AlertDialogTitle>
            <AlertDialogDescription>{pending.consequence}</AlertDialogDescription>
          </AlertDialogHeader>
          <ul className="text-caption text-subtle-foreground">
            {pending.affected.map((label) => (
              <li key={label} className="truncate">
                {label}
              </li>
            ))}
          </ul>
          <AlertDialogFooter>
            <AlertDialogCancel>Keep open</AlertDialogCancel>
            <AlertDialogAction onClick={() => actions.confirmClose()}>Stop work and close</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      ) : null}
    </AlertDialog>
  );
}

/** The trash confirmation: an irreversible effect is confirmed first (B10). */
export function ConfirmTrash({ actions }: { actions: Actions }) {
  const pending = useUiStore((s) => s.pendingTrash);
  return (
    <AlertDialog open={pending != null} onOpenChange={(open) => { if (!open) actions.cancelTrash(); }}>
      {pending ? (
        <AlertDialogContent data-confirm-trash={pending.path}>
          <AlertDialogHeader>
            <AlertDialogTitle>Move to Trash</AlertDialogTitle>
            <AlertDialogDescription>
              {pending.name} {pending.isDirectory ? "and its contents" : ""} will move to the Trash.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel data-trash-cancel="true">Cancel</AlertDialogCancel>
            <AlertDialogAction data-trash-confirm="true" onClick={() => actions.confirmTrash()}>
              Move to Trash
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      ) : null}
    </AlertDialog>
  );
}

/**
 * A one-line notice the operator can act on: the refreshable one offers
 * `refresh_status`, and a view whose unsaved work this page cannot save
 * offers Don't save, the only close that drops it (S7 B5). It stays its own
 * row rather than a toast, because its state is one the operator still has to
 * act on (design 13).
 */
export function NoticeBar({ actions }: { actions: Actions }) {
  const notice = useUiStore((s) => s.notice);
  const setNotice = useUiStore((s) => s.setNotice);
  if (!notice) return null;
  const dontSave = notice.dontSave;
  return (
    <div role="status" data-notice="true" className="flex items-center gap-md border-b border-border bg-card px-md py-xs text-caption text-subtle-foreground">
      <Hint label={notice.text} reveals>
      <span className="min-w-0 flex-1 truncate">
        {notice.text}
      </span>
      </Hint>
      {notice.refreshable ? (
        <Button variant="link" size="sm" className="h-auto px-none" onClick={() => actions.refreshStatus()}>
          Check status
        </Button>
      ) : null}
      {dontSave ? (
        <Button
          variant="link"
          size="sm"
          className="h-auto px-none text-destructive"
          data-notice-dont-save={dontSave.displayId}
          onClick={() => actions.closeViewWithoutSaving(dontSave)}
        >
          Don&apos;t save
        </Button>
      ) : null}
      <Hint label="Dismiss">
        <Button variant="ghost" size="icon-sm" aria-label="Dismiss" onClick={() => setNotice(null)}>
          <XIcon />
        </Button>
      </Hint>
    </div>
  );
}

/** ⌘F over the focused pane, through the core's `pane_find`; the count comes back in the snapshot. */
export function FindBar({ actions }: { actions: Actions }) {
  const open = useUiStore((s) => s.overlay === "find");
  const close = useUiStore((s) => s.closeOverlay);
  const pushEscape = useUiStore((s) => s.pushEscape);
  const find = useShellStore((s) => s.find);
  const paneId = useShellStore((s) => s.focusedPaneId);
  const [term, setTerm] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (open) inputRef.current?.focus();
  }, [open]);
  // Escape and × end the search and give the keyboard back to the pane it
  // searched, so typing continues where it was (B5).
  const dismiss = useRef(() => {});
  dismiss.current = () => {
    if (paneId) actions.find(paneId, "", 0);
    close("find");
    if (paneId) focusTerminal(paneId);
  };
  useEffect(() => (open ? pushEscape(() => dismiss.current()) : undefined), [open, pushEscape]);
  if (!open) return null;
  const submit = (step: -1 | 0 | 1) => {
    if (paneId) actions.find(paneId, term, step);
  };
  // The core's index is already 1-based, and 0 while nothing matches.
  const count = find && find.pane_id === paneId && find.term === term ? `${find.index}/${find.total}${find.truncated ? "+" : ""}` : "";
  return (
    <div data-find-bar="true" className="flex items-center gap-sm border-b border-border bg-card px-md py-xs text-caption">
      <Input
        ref={inputRef}
        value={term}
        placeholder="Find in pane"
        className="h-(--size-control-sm) flex-1"
        onChange={(event) => setTerm(event.target.value)}
        onKeyDown={(event) => {
          if (event.nativeEvent.isComposing) return;
          if (event.key === "Enter") {
            event.preventDefault();
            submit(event.shiftKey ? -1 : 1);
          }
        }}
      />
      <span className="text-muted-foreground">{find?.unavailable_reason && find.pane_id === paneId ? find.unavailable_reason : count}</span>
      <Hint label="Previous match">
        <Button variant="ghost" size="icon-sm" aria-label="Previous match" onClick={() => submit(-1)}>
          <ChevronUpIcon />
        </Button>
      </Hint>
      <Hint label="Next match">
        <Button variant="ghost" size="icon-sm" aria-label="Next match" onClick={() => submit(1)}>
          <ChevronDownIcon />
        </Button>
      </Hint>
      <Hint label="Close find">
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="Close find"
          onClick={() => dismiss.current()}
        >
          <XIcon />
        </Button>
      </Hint>
    </div>
  );
}
