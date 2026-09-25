import { EllipsisIcon, XIcon } from "lucide-react";
import { memo, useEffect, useRef } from "react";
import type { Actions } from "./actions";
import { refusalText, submitFiles } from "./attachments";
import { Button } from "./components/ui/button";
import { Hint } from "./components/ui/tooltip";
import { ChildChipRow, ReturnToParent, usePaneMenu } from "./PaneRelations";
import type { PaneRow, TerminalPane } from "./snapshot";
import { useShellStore } from "./store";
import { attachTerminal, bracketedPaste, focusTerminal, requestView, setTextScale } from "./terminals";

/**
 * Transport states with a live stream; anything else is drawn as a caption in
 * the header. An observed pane (another client holds Herdr's control) streams
 * too, and its wheel moves Herdr's viewport.
 */
const LIVE_STATES = new Set(["connected", "controlling", "observing", "idle"]);

export function paneTitle(pane: PaneRow): string {
  // A remote pane's id is scoped to its device (`remote:<device>:pane:w1:p2`);
  // the device is already on screen, so the header names the host's own id.
  return pane.identity_label ?? pane.terminal_title ?? pane.herdr_label ?? pane.id.replace(/^remote:.+?:pane:/, "");
}

/**
 * The caption a non-live transport state gets, and whether a click asks the
 * core to reattach. `reconnect_pane` finds its pane among this machine's, so a
 * remote pane's caption only reports; the core reattaches it on the host's
 * next session update.
 */
export function transportCaption(transport: TerminalPane | undefined, local = true, offline = false): { text: string; reconnects: boolean } | null {
  // With its host's connection down, a remote attach ends as `closing`; the
  // pane is not closing, its device is unreachable.
  if (offline) return { text: "disconnected", reconnects: false };
  // The core found the last wheel unmoved on a pane another client controls
  // (B3): said where the wheel went rather than dropped silently.
  if (transport?.scroll_held_elsewhere) return { text: "scroll held by another client", reconnects: false };
  if (!transport || LIVE_STATES.has(transport.transport_state)) return null;
  if (!local) {
    const state = transport.transport_state;
    return { text: state === "closing" ? "closing…" : state === "ended" || state === "unavailable" ? state : "starting…", reconnects: false };
  }
  switch (transport.transport_state) {
    case "released":
      return { text: "released · click to attach", reconnects: true };
    case "ended":
      return {
        text: transport.exit_code == null ? "ended · click to restart" : `exited ${transport.exit_code} · click to restart`,
        reconnects: true,
      };
    case "unavailable":
      return { text: "unavailable · click to retry", reconnects: true };
    case "closing":
      return { text: "closing…", reconnects: false };
    default:
      return { text: "starting…", reconnects: false };
  }
}

export const PaneView = memo(function PaneView({
  pane,
  transport,
  focused,
  scale,
  actions,
  local = true,
  offline = false,
}: {
  pane: PaneRow;
  transport: TerminalPane | undefined;
  focused: boolean;
  scale: number;
  actions: Actions;
  /** False for a pane on a selected SSH device. */
  local?: boolean;
  /** True while that device's connection is down. */
  offline?: boolean;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  const viewGeneration = useShellStore((s) => s.viewGeneration);
  const refusal = useShellStore((s) => s.attachmentRefusal);
  const setRefusal = useShellStore((s) => s.setAttachmentRefusal);
  const paneId = pane.id;
  const dispatch = actions.dispatch;
  const title = paneTitle(pane);
  const paneMenu = usePaneMenu(pane, title, actions);

  // A dropped file or a pasted image stages through hided and reaches the
  // terminal as the core's own `terminal_attachment` (B14).
  const drop = (event: React.DragEvent) => {
    const files = Array.from(event.dataTransfer.files);
    if (files.length === 0) return;
    event.preventDefault();
    void submitFiles(paneId, files, false, bracketedPaste(paneId));
  };

  // The terminal is shown here and parked on unmount, not disposed: the
  // instance belongs to the pane for as long as the core streams it (D-05).
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    return attachTerminal(paneId, host, dispatch, useShellStore.getState().rest?.ui_state?.pane_text_scales?.[paneId] ?? 1);
  }, [paneId, dispatch]);

  // After every self-contained snapshot the core may have restarted, so the
  // pane asks for a full frame rather than trusting what it has drawn. The
  // attach already requested what it needed, so the first generation is skipped.
  const mountedGeneration = useRef(viewGeneration);
  useEffect(() => {
    if (viewGeneration === mountedGeneration.current) return;
    mountedGeneration.current = viewGeneration;
    requestView(paneId);
  }, [paneId, viewGeneration]);

  useEffect(() => {
    setTextScale(paneId, scale);
  }, [paneId, scale]);

  useEffect(() => {
    if (focused) focusTerminal(paneId);
  }, [paneId, focused]);

  // ⌘V of an image is the shell's; text paste stays xterm's own. The listener
  // runs in the capture phase, before xterm's textarea sees the event.
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return undefined;
    const onPaste = (event: ClipboardEvent) => {
      const items = Array.from(event.clipboardData?.items ?? []).filter((item) => item.type.startsWith("image/"));
      if (items.length === 0) return;
      const files = items.map((item) => item.getAsFile()).filter((file): file is File => file !== null);
      if (files.length === 0) return;
      event.preventDefault();
      event.stopPropagation();
      void submitFiles(paneId, files, true, bracketedPaste(paneId));
    };
    host.addEventListener("paste", onPaste, true);
    return () => host.removeEventListener("paste", onPaste, true);
  }, [paneId]);

  const caption = transportCaption(transport, local, offline);
  return (
    <section
      className="flex h-full min-h-0 min-w-0 flex-col bg-background"
      data-pane-view={paneId}
      data-focused={focused ? "true" : "false"}
      data-transport={transport?.transport_state ?? ""}
      onDragOver={(event) => {
        if (event.dataTransfer.types.includes("Files")) event.preventDefault();
      }}
      onDrop={drop}
    >
      <header
        className={`relative flex h-[var(--size-pane-header)] shrink-0 items-center gap-sm px-sm text-caption ${
          focused ? "bg-secondary text-foreground" : "bg-card text-subtle-foreground"
        }`}
        onContextMenu={(event) => {
          event.preventDefault();
          paneMenu.openAt(event.clientX, event.clientY);
        }}
      >
        <ReturnToParent pane={pane} actions={actions} />
        <Hint label={title} reveals>
        <span className="min-w-0 flex-1 truncate">
          {title}
        </span>
        </Hint>
        {caption ? (
          <span className="truncate text-muted-foreground">{caption.text}</span>
        ) : (
          <span className="truncate text-muted-foreground">{pane.status_label}</span>
        )}
        <Hint label={`Pane actions for ${title}`}>
          <Button
            variant="ghost"
            size="icon-sm"
            className="text-subtle-foreground hover:bg-popover hover:text-foreground"
            aria-haspopup="menu"
            data-pane-menu={paneId}
            onClick={(event) => {
              const button = event.currentTarget.getBoundingClientRect();
              paneMenu.openAt(button.left, button.bottom);
            }}
            onKeyDown={(event) => {
              if (event.key === "ContextMenu" || (event.key === "F10" && event.shiftKey)) {
                event.preventDefault();
                event.currentTarget.click();
              }
            }}
          >
            <EllipsisIcon />
          </Button>
        </Hint>
        <Hint label={`Close pane ${title}`}>
          <Button variant="ghost" size="icon-sm" className="text-subtle-foreground hover:bg-popover hover:text-foreground" onClick={() => actions.closePane(paneId)}>
            <XIcon />
          </Button>
        </Hint>
        {paneMenu.menu}
      </header>
      <ChildChipRow pane={pane} actions={actions} />
      <div className="h-[var(--size-hairline)] shrink-0 bg-border" />
      <div className="relative min-h-0 flex-1">
        <div ref={hostRef} className="absolute inset-0" data-terminal-host={paneId} />
        {refusal?.pane_id === paneId ? (
          <div className="absolute inset-x-0 top-0 flex items-center gap-sm bg-card px-sm py-xxs text-caption text-destructive" data-pane-attachment-refusal="true">
            <span className="min-w-0 flex-1 truncate">{refusalText(refusal.reason)}</span>
            <Hint label="Dismiss">
              <Button variant="ghost" size="icon-sm" className="text-muted-foreground" onClick={() => setRefusal(null)}>
                <XIcon />
              </Button>
            </Hint>
          </div>
        ) : null}
        {caption?.reconnects ? (
          <button
            type="button"
            className="absolute inset-0 flex items-center justify-center bg-card text-caption text-subtle-foreground"
            onClick={() =>
              dispatch({ schema_version: 2, kind: "reconnect_pane", payload: { pane_id: paneId } })
            }
          >
            {caption.text}
          </button>
        ) : null}
      </div>
    </section>
  );
});
