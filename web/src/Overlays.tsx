import { ChevronDownIcon, ChevronUpIcon, XIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { Actions } from "./actions";
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from "./components/ui/alert-dialog";
import { Button } from "./components/ui/button";
import { Hint } from "./components/ui/tooltip";
import { Input } from "./components/ui/input";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";

/** The held-⌥ list of recent tabs or projects; the row at `index` is what release commits. */
export function CycleOverlay() {
  const cycle = useUiStore((s) => s.cycle);
  if (!cycle) return null;
  return (
    <div className="absolute inset-x-0 top-[var(--size-tab-strip)] z-30 flex justify-center" data-cycle={cycle.kind}>
      <ul className="w-[var(--size-pr-popover)] rounded-md border border-border bg-popover py-xs text-body shadow-lg">
        {cycle.items.map((item, index) => (
          <li
            key={item.id}
            data-cycle-row={item.id}
            aria-selected={index === cycle.index}
            className={`flex items-baseline gap-sm px-md py-xxs ${index === cycle.index ? "bg-secondary text-foreground" : "text-subtle-foreground"}`}
          >
            <span className="min-w-0 flex-1 truncate">{item.label}</span>
            <span className="truncate text-caption text-muted-foreground">{item.detail}</span>
          </li>
        ))}
      </ul>
    </div>
  );
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
      <span className="min-w-0 flex-1 truncate" title={notice.text}>
        {notice.text}
      </span>
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
  const find = useShellStore((s) => s.find);
  const paneId = useShellStore((s) => s.focusedPaneId);
  const [term, setTerm] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (open) inputRef.current?.focus();
  }, [open]);
  if (!open) return null;
  const submit = (step: -1 | 0 | 1) => {
    if (paneId) actions.find(paneId, term, step);
  };
  const count = find && find.pane_id === paneId && find.term === term ? `${find.total ? find.index + 1 : 0}/${find.total}${find.truncated ? "+" : ""}` : "";
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
          onClick={() => {
            if (paneId) actions.find(paneId, "", 0);
            close("find");
          }}
        >
          <XIcon />
        </Button>
      </Hint>
    </div>
  );
}
