import { useEffect, useRef, useState } from "react";
import type { Actions } from "./actions";
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
  if (!pending) return null;
  return (
    <div className="absolute inset-0 z-40 flex items-center justify-center" role="presentation">
      <div className="absolute inset-0 bg-background opacity-[var(--opacity-secondary)]" />
      <div
        role="alertdialog"
        aria-label={pending.title}
        data-confirm-close={pending.kind}
        className="relative w-[var(--size-add-device-sheet-w)] rounded-lg border border-border bg-popover p-lg text-body text-foreground shadow-lg"
      >
        <h2 className="mb-xs text-title">{pending.title}</h2>
        <p className="mb-sm text-subtle-foreground">{pending.consequence}</p>
        <ul className="mb-md text-caption text-subtle-foreground">
          {pending.affected.map((label) => (
            <li key={label} className="truncate">
              {label}
            </li>
          ))}
        </ul>
        <div className="flex justify-end gap-sm">
          <button
            type="button"
            className="rounded-sm bg-secondary px-md py-xs text-foreground hover:bg-border"
            onClick={() => actions.keepOpen()}
          >
            Keep open
          </button>
          <button
            type="button"
            className="rounded-sm bg-destructive px-md py-xs text-destructive-foreground"
            onClick={() => actions.confirmClose()}
          >
            Stop work and close
          </button>
        </div>
      </div>
    </div>
  );
}

/** The trash confirmation: an irreversible effect is confirmed first (B10). */
export function ConfirmTrash({ actions }: { actions: Actions }) {
  const pending = useUiStore((s) => s.pendingTrash);
  if (!pending) return null;
  return (
    <div className="absolute inset-0 z-40 flex items-center justify-center" role="presentation">
      <div className="absolute inset-0 bg-background opacity-[var(--opacity-secondary)]" />
      <div
        role="alertdialog"
        aria-label="Move to Trash"
        data-confirm-trash={pending.path}
        className="relative w-[var(--size-add-device-sheet-w)] rounded-lg border border-border bg-popover p-lg text-body text-foreground shadow-lg"
      >
        <h2 className="mb-xs text-title">Move to Trash</h2>
        <p className="mb-md text-subtle-foreground">
          {pending.name} {pending.isDirectory ? "and its contents" : ""} will move to the Trash.
        </p>
        <div className="flex justify-end gap-sm">
          <button
            type="button"
            className="rounded-sm bg-secondary px-md py-xs text-foreground hover:bg-border"
            data-trash-cancel="true"
            onClick={() => actions.cancelTrash()}
          >
            Cancel
          </button>
          <button
            type="button"
            className="rounded-sm bg-destructive px-md py-xs text-destructive-foreground"
            data-trash-confirm="true"
            onClick={() => actions.confirmTrash()}
          >
            Move to Trash
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * A one-line notice the operator can act on: the refreshable one offers
 * `refresh_status`, and a view whose unsaved work this page cannot save
 * offers Don't save, the only close that drops it (S7 B5).
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
        <button type="button" className="text-foreground underline" onClick={() => actions.refreshStatus()}>
          Check status
        </button>
      ) : null}
      {dontSave ? (
        <button type="button" className="text-destructive underline" data-notice-dont-save={dontSave.displayId} onClick={() => actions.closeViewWithoutSaving(dontSave)}>
          Don&apos;t save
        </button>
      ) : null}
      <button type="button" className="text-muted-foreground" aria-label="Dismiss" onClick={() => setNotice(null)}>
        ×
      </button>
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
      <input
        ref={inputRef}
        value={term}
        placeholder="Find in pane"
        className="min-w-0 flex-1 rounded-xs bg-secondary px-xs py-xxs text-body text-foreground outline-none"
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
      <button type="button" className="text-subtle-foreground" onClick={() => submit(-1)} aria-label="Previous match">
        ↑
      </button>
      <button type="button" className="text-subtle-foreground" onClick={() => submit(1)} aria-label="Next match">
        ↓
      </button>
      <button
        type="button"
        className="text-muted-foreground"
        aria-label="Close find"
        onClick={() => {
          if (paneId) actions.find(paneId, "", 0);
          close("find");
        }}
      >
        ×
      </button>
    </div>
  );
}
