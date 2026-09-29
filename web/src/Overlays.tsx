import { ChevronDownIcon, ChevronUpIcon, FolderIcon, HouseIcon, LayoutDashboardIcon, XIcon } from "lucide-react";
import { useEffect, useRef, useState, type RefObject } from "react";
import { createPortal } from "react-dom";
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
import { closeSheet, statusUnknownNotice, stopWorkCopy, subtreeTitle, type StopWork, type Subtree } from "./close";
import { SubtreeList } from "./components/subtree-list";
import { knownProvider } from "./workspace";

/**
 * Recent Panels or Recent Projects while the chord's modifier is held: at
 * most nine rows around the highlight, which is what release commits
 * (docs/UI_BEHAVIOR.md, Recent navigation). It is a layer of the body, like
 * the palette and the menus, so a browser page it meets freezes under it
 * (`web/src/browserViews.ts`).
 */
export function CycleOverlay() {
  const cycle = useUiStore((s) => s.cycle);
  const registry = useShellStore((s) => hostRegistry(s.rest?.ui_state, hostKind()).registry);
  if (!cycle) return null;
  const { start, rows } = visibleWindow(cycle.items, cycle.index);
  const title = cycle.kind === "panels" ? "Recent Panels" : "Recent Projects";
  const chord = displayCommand(cycle.kind === "panels" ? "recent_panel" : "recent_project", hostKind(), registry);
  return createPortal(
    <div className="fixed inset-x-0 top-[var(--size-tab-strip)] z-30 flex justify-center" data-cycle={cycle.kind}>
      <div role="listbox" aria-label={title} data-slot="cycle-overlay" className="w-[var(--size-pr-popover)] rounded-md border border-border bg-popover py-xs shadow-lg">
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
              data-cycle-row={item.target.kind === "surface" ? item.target.surface.id : item.key}
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
    </div>,
    document.body,
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

/** The every-project Overview and a Project's Overview wear the marks their sidebar rows wear. */
function KindMark({ item }: { item: CycleItem }) {
  if (item.kind === "herdr") return <AgentMark kind={item.agent?.agent_kind} />;
  if (item.kind === "project") return <FolderIcon aria-hidden="true" className="size-(--size-icon) text-muted-foreground" />;
  if (item.kind === "main") return <HouseIcon aria-hidden="true" className="size-(--size-icon) text-muted-foreground" />;
  if (item.kind === "overview") return <LayoutDashboardIcon aria-hidden="true" className="size-(--size-icon) text-muted-foreground" />;
  return displayMark({ kind: item.kind, label: item.title });
}

/**
 * The close confirmation: the Stop-work sheet (Keep open, or stop the work
 * and close), or, for a target with live descendants outside what closes,
 * the subtree sheet (PRD close-agent-subtree B2). Both are live (B13, B28):
 * every snapshot re-derives which of the two it is, who it lists and in what
 * state, so a sheet turns into the other in place, a press sends what is on
 * screen, and the sheet closes by itself only when its target is gone (D-40).
 */
export function ConfirmClose({ actions }: { actions: Actions }) {
  const pending = useUiStore((s) => s.pendingClose);
  useShellStore((s) => s.rest);
  useShellStore((s) => s.agents);
  const target = pending ? actions.closeTarget(pending) : null;
  const gone = pending != null && target === null;
  useEffect(() => {
    if (gone) actions.keepOpen();
  }, [gone, actions]);
  const sheet = target ? closeSheet(target.panes, target.agents, actions.everyAgent()) : null;
  const blocked = sheet?.sheet === "subtree" ? sheet.subtree.unknown || sheet.subtree.targetUnknown : sheet?.stopWork.unknown != null;
  // A pane or descendant whose status turns unknown while the sheet is open
  // disables a close under the keyboard; the cancel button takes it, as when
  // the subtree sheet opens blocked (D-19, B28). A sheet that turns into the
  // other in place leaves the keyboard on the dialog itself, never on a
  // destructive button an Enter meant for the old one would press (D-40).
  const footer = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!blocked) return;
    const active = document.activeElement;
    if (active && active !== document.body && !active.matches(":disabled")) return;
    footer.current?.querySelector<HTMLElement>("[data-close-cancel]")?.focus();
  }, [blocked]);
  return (
    <AlertDialog open={pending != null && target != null} onOpenChange={(open) => { if (!open) actions.keepOpen(); }}>
      {pending && sheet ? (
        <AlertDialogContent
          data-confirm-close={pending.kind}
          data-confirm-subtree={sheet.sheet === "subtree" ? "true" : undefined}
          initialFocus={sheet.sheet === "subtree" ? (blocked ? "cancel" : "action") : "container"}
        >
          {sheet.sheet === "subtree" ? (
            <SubtreeClose actions={actions} kind={pending.kind} targetId={pending.targetId} subtree={sheet.subtree} footer={footer} />
          ) : (
            <StopWorkClose actions={actions} kind={pending.kind} stopWork={sheet.stopWork} footer={footer} />
          )}
        </AlertDialogContent>
      ) : null}
    </AlertDialog>
  );
}

/**
 * The Stop-work sheet: every pane that closes with its mark and status word,
 * a quiet one dimmed. While a pane's activity is unknown, Stop work and close
 * waits for the status check the sheet offers (B28, D-39).
 */
function StopWorkClose({ actions, kind, stopWork, footer }: { actions: Actions; kind: "pane" | "tab"; stopWork: StopWork; footer: RefObject<HTMLDivElement | null> }) {
  const copy = stopWorkCopy(kind);
  return (
    <>
      <AlertDialogHeader>
        <AlertDialogTitle>{copy.title}</AlertDialogTitle>
        <AlertDialogDescription>{copy.consequence}</AlertDialogDescription>
      </AlertDialogHeader>
      <ul className="flex min-w-0 flex-col gap-xxs text-caption" data-stop-work-list="true">
        {stopWork.rows.map((row) => (
          <li
            key={row.pane.id}
            data-stop-work-row={row.pane.id}
            data-stop-work-state={row.state}
            className={`flex min-w-0 items-start gap-xs ${row.state === "quiet" ? "opacity-(--opacity-read-status)" : ""}`}
          >
            <span className="flex w-(--size-agent-mark) shrink-0 justify-center">{row.agent ? <StatusMark symbol={row.agent.symbol} className={markTone(row.agent)} /> : null}</span>
            <span className="flex min-w-0 flex-1 flex-wrap items-baseline gap-x-xs">
              <span className="min-w-0 break-words text-foreground">{row.label}</span>
              <span className={`shrink-0 ${row.agent ? markTone(row.agent) : "text-subtle-foreground"}`}>{row.agent?.status_label ?? row.pane.status_label}</span>
            </span>
          </li>
        ))}
      </ul>
      {stopWork.unknown ? (
        <p className="flex flex-wrap items-center gap-xs text-caption text-subtle-foreground" data-stop-work-blocked="true">
          <span className="min-w-0 break-words">{statusUnknownNotice(stopWork.unknown.label)}</span>
          <Button size="sm" variant="secondary" onClick={() => actions.refreshStatus()} data-stop-work-check-status="true">
            Check status
          </Button>
        </p>
      ) : null}
      <AlertDialogFooter ref={footer}>
        <AlertDialogCancel data-close-cancel="true">Keep open</AlertDialogCancel>
        <AlertDialogAction disabled={stopWork.unknown != null} onClick={() => actions.confirmClose()}>
          Stop work and close
        </AlertDialogAction>
      </AlertDialogFooter>
    </>
  );
}

/**
 * The close of an agent with descendants (D-07, D-17, D-18, D-19): one list
 * of what closes with it, and 취소 / 이것만 닫기 / 모두 닫기 with 모두 닫기 the
 * Enter default. While a listed descendant's activity is unknown, 모두 닫기
 * waits for a status check the sheet itself offers, and the default is 취소.
 */
function SubtreeClose({ actions, kind, targetId, subtree, footer }: { actions: Actions; kind: "pane" | "tab"; targetId: string | null; subtree: Subtree; footer: RefObject<HTMLDivElement | null> }) {
  const count = subtree.rows.filter((row) => !row.target).length;
  const targetDevice = subtree.rows.find((row) => row.target)?.agent.device_id;
  const closeAllBlocked = subtree.unknown || subtree.targetUnknown;
  return (
    <>
      <AlertDialogHeader>
        <AlertDialogTitle>{subtreeTitle(kind, count)}</AlertDialogTitle>
        <AlertDialogDescription data-subtree-consequence="true">이것만 닫기: 자식은 계속 실행되고 내 목록으로 올라옵니다.</AlertDialogDescription>
      </AlertDialogHeader>
      <SubtreeList subtree={subtree} targetDevice={targetDevice ?? (targetId ?? undefined)} />
      {closeAllBlocked ? (
        <p className="flex flex-wrap items-center gap-xs text-caption text-subtle-foreground" data-subtree-blocked="true">
          <span className="min-w-0 break-words">
            {subtree.targetUnknown ? "상태를 모르는 에이전트가 있어 닫기 전에 상태를 확인해야 합니다." : "상태를 모르는 자식이 있어 모두 닫기 전에 상태를 확인해야 합니다."}
          </span>
          <Button size="sm" variant="secondary" onClick={() => actions.refreshStatus()} data-subtree-check-status="true">
            상태 확인
          </Button>
        </p>
      ) : null}
      <AlertDialogFooter ref={footer}>
        <AlertDialogCancel data-subtree-cancel="true" data-close-cancel="true">
          취소
        </AlertDialogCancel>
        <Button variant="secondary" disabled={subtree.targetUnknown} onClick={() => actions.confirmClose()} data-subtree-close-only="true">
          이것만 닫기
        </Button>
        <Button
          variant="destructive"
          disabled={closeAllBlocked}
          onClick={() => actions.closeSubtree(subtree.ids)}
          data-subtree-close-all="true"
          data-initial-focus={closeAllBlocked ? undefined : "true"}
        >
          모두 닫기
        </Button>
      </AlertDialogFooter>
    </>
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

/** A refused replacement close remains an explicit, retryable operator intent. */
export function AgentCloseNotice({ actions }: { actions: Actions }) {
  const pending = useShellStore((s) => s.rest?.recent_closed?.pending);
  const item = pending?.find((item) => item.phase === "failed" || item.phase === "refused");
  if (!item) return null;
  return <div role="status" data-agent-close-notice={item.key} className="flex items-center gap-md border-b border-border bg-card px-md py-xs text-caption text-subtle-foreground">
    <span className="min-w-0 flex-1">{item.message}</span>
    {item.retryable && <Button variant="link" size="sm" onClick={() => actions.retryAgentClose(item.key)}>Retry close</Button>}
    <Button variant="ghost" size="icon-sm" aria-label="Dismiss close" onClick={() => actions.dismissAgentClose(item.key)}><XIcon /></Button>
  </div>;
}
