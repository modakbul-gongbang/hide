import type { TFunction } from "i18next";
import { CircleAlertIcon, EllipsisIcon, Maximize2Icon, MoonIcon, XIcon } from "lucide-react";
import { memo, useEffect, useRef } from "react";
import type { Actions } from "./actions";
import { refusalText, submitFiles } from "./attachments";
import { AgentMark } from "./AgentMark";
import { StatusMark } from "./components/status-mark";
import { Button } from "./components/ui/button";
import { Hint } from "./components/ui/tooltip";
import { ChildChipRow, ReturnToParent, usePaneMenu, type TerminalMenuContext } from "./PaneRelations";
import { chordLabel, commandLabel } from "./shortcutLabels";
import { keySystem } from "./host";
import { useInterfaceTranslation } from "./i18n/client";
import { modChord, TERMINAL_COPY, TERMINAL_PASTE } from "./shortcuts";
import { sleepCaption, wakingLine } from "./sleep";
import type { AgentSleep, PaneRow, TerminalPane } from "./snapshot";
import { useShellStore } from "./store";
import { attachTerminal, bracketedPaste, focusTerminal, requestView, setTextScale, terminalSelectionText } from "./terminals";

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
export function transportCaption(transport: TerminalPane | undefined, t: TFunction<"translation">, local = true, offline = false): { text: string; reconnects: boolean } | null {
  // With its host's connection down, a remote attach ends as `closing`; the
  // pane is not closing, its device is unreachable.
  if (offline) return { text: t("panes.transport.disconnected"), reconnects: false };
  // The core found the last wheel unmoved on a pane another client controls
  // (B3): said where the wheel went rather than dropped silently.
  if (transport?.scroll_held_elsewhere) return { text: t("panes.transport.scrollElsewhere"), reconnects: false };
  if (!transport || LIVE_STATES.has(transport.transport_state)) return null;
  if (!local) {
    const state = transport.transport_state;
    return { text: state === "closing" ? t("panes.transport.closing") : state === "ended" ? t("panes.transport.remoteEnded") : state === "unavailable" ? t("panes.transport.remoteUnavailable") : t("panes.transport.starting"), reconnects: false };
  }
  switch (transport.transport_state) {
    case "released":
      return { text: t("panes.transport.released"), reconnects: true };
    case "ended":
      return {
        text: transport.exit_code == null ? t("panes.transport.ended") : t("panes.transport.exited", { exitCode: transport.exit_code }),
        reconnects: true,
      };
    case "unavailable":
      return { text: t("panes.transport.unavailable"), reconnects: true };
    case "closing":
      return { text: t("panes.transport.closing"), reconnects: false };
    default:
      return { text: t("panes.transport.starting"), reconnects: false };
  }
}

/** The chords the registry binds here for the terminal menu's commands, read when it opens. */
function terminalMenuChords(): TerminalMenuContext["chords"] {
  return {
    copy: chordLabel(modChord(TERMINAL_COPY, keySystem())),
    paste: chordLabel(modChord(TERMINAL_PASTE, keySystem())),
    find: commandLabel("find_in_pane"),
    splitRight: commandLabel("split_right"),
    splitDown: commandLabel("split_down"),
    zoom: commandLabel("toggle_zoom"),
  };
}

/**
 * What a pane shows in place of its terminal while its agent sleeps (PRD
 * agent-sleep B11-B14): the shell under it is not what the operator was
 * talking to, so the terminal stays hidden until the agent is back.
 */
function SleepBody({ paneId, sleep, actions }: { paneId: string; sleep: AgentSleep; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const now = Date.now();
  return (
    <div
      className="absolute inset-0 flex flex-col items-center justify-center gap-sm bg-card px-lg text-center"
      data-pane-sleep={sleep.state}
    >
      {sleep.state === "sleeping" ? (
        <>
          <MoonIcon className="size-(--size-icon-lg) text-muted-foreground" aria-hidden="true" />
          <p className="text-subhead text-foreground">{t("panes.sleep.sleeping")}</p>
          {sleep.progress ? <p className="max-w-full truncate text-caption text-muted-foreground">{sleep.progress}</p> : null}
          <Button size="sm" className="mt-xs" data-agent-wake={paneId} onClick={() => actions.wakeAgent(paneId)}>
            {t("panes.sleep.wake")}
          </Button>
        </>
      ) : sleep.state === "waking" ? (
        <>
          <MoonIcon className="size-(--size-icon-lg) text-muted-foreground" aria-hidden="true" />
          <p className="text-subhead text-foreground" role="status">
            {t("panes.sleep.waking")}
          </p>
          <p className="text-caption text-muted-foreground">{wakingLine(sleep, now, t)}</p>
        </>
      ) : (
        <>
          <CircleAlertIcon className="size-(--size-icon-lg) text-destructive" aria-hidden="true" />
          <p className="text-subhead text-foreground">{t("panes.sleep.resumeFailed")}</p>
          {sleep.reason ? (
            <p className="max-w-full text-caption text-muted-foreground" role="alert">
              {sleep.reason}
            </p>
          ) : null}
          <div className="mt-xs flex items-center gap-sm">
            <Button size="sm" variant="secondary" data-agent-wake-retry={paneId} onClick={() => actions.wakeAgent(paneId)}>
              {t("common.retry")}
            </Button>
            <Button size="sm" variant="outline" data-agent-wake-fresh={paneId} onClick={() => actions.wakeAgent(paneId, true)}>
              {t("panes.sleep.newSession")}
            </Button>
          </div>
        </>
      )}
    </div>
  );
}

export const PaneView = memo(function PaneView({
  pane,
  transport,
  focused,
  scale,
  actions,
  agentKind,
  markSymbol,
  markTone,
  paneCount,
  zoomed,
  local = true,
  offline = false,
}: {
  pane: PaneRow;
  transport: TerminalPane | undefined;
  focused: boolean;
  scale: number;
  actions: Actions;
  /** The pane's agent, as the sidebar row and the tab draw it; null for a plain shell. */
  agentKind: string | null;
  markSymbol: string | null;
  markTone: string;
  /** Panes in the tab, a zoomed pane's hidden siblings included. */
  paneCount: number;
  /** The tab is zoomed, so this pane is the only one drawn. */
  zoomed: boolean;
  /** False for a pane on a selected SSH device. */
  local?: boolean;
  /** True while that device's connection is down. */
  offline?: boolean;
}) {
  const { t } = useInterfaceTranslation();
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
    return attachTerminal(paneId, host, dispatch, actions, useShellStore.getState().rest?.ui_state?.pane_text_scales?.[paneId] ?? 1);
  }, [paneId, dispatch, actions]);

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
    if (!focused) return;
    // A palette that holds the keyboard keeps it: a focus that lands late (the
    // core's answer to a choice made in ⌘K) must not take the keys the
    // operator is typing into a palette opened since. The palette hands the
    // keyboard back to the focused pane when it closes (`restoreFocus`).
    if (document.activeElement?.closest("[data-palette]")) return;
    focusTerminal(paneId);
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

  const openTerminalMenu = (event: React.MouseEvent) => {
    event.preventDefault();
    paneMenu.openTerminalAt(event.clientX, event.clientY, {
      selection: terminalSelectionText(paneId) !== null,
      zoomed,
      paneCount,
      chords: terminalMenuChords(),
    });
  };
  const zoomChord = zoomed ? terminalMenuChords().zoom : "";
  const hidden = paneCount - 1;

  const caption = transportCaption(transport, t, local, offline);
  const sleep = local ? pane.sleep : undefined;
  const sleepWords = sleep ? sleepCaption(sleep, Date.now(), t) : null;
  return (
    <section
      className="group/pane relative flex h-full min-h-0 min-w-0 flex-col bg-background"
      data-pane-view={paneId}
      data-focused={focused ? "true" : "false"}
      data-menu-open={paneMenu.open ? "true" : "false"}
      data-transport={transport?.transport_state ?? ""}
      onDragOver={(event) => {
        if (event.dataTransfer.types.includes("Files")) event.preventDefault();
      }}
      onDrop={drop}
    >
      <header
        className={`relative flex h-[var(--size-pane-header)] shrink-0 items-center gap-sm px-sm text-caption ${
          focused ? "bg-secondary text-foreground" : "bg-card text-muted-foreground"
        }`}
        onContextMenu={(event) => {
          event.preventDefault();
          paneMenu.openAt(event.clientX, event.clientY);
        }}
      >
        <ReturnToParent pane={pane} actions={actions} />
        {markSymbol ? <StatusMark symbol={markSymbol} className={markTone} data-pane-status-mark={markSymbol} /> : null}
        <AgentMark kind={agentKind} />
        <Hint label={title} reveals>
        <span className="min-w-0 flex-1 truncate">
          {title}
        </span>
        </Hint>
        {zoomed ? (
          <Hint label={zoomChord ? t("panes.unzoomChord", { chord: zoomChord }) : t("panes.unzoom")}>
            <Button
              variant="ghost"
              size="sm"
              className="shrink-0 gap-xxs px-xs text-caption text-foreground hover:bg-popover"
              aria-label={hidden > 0 ? t("panes.unzoomHidden", { count: hidden }) : t("panes.unzoom")}
              data-pane-zoom={hidden}
              onClick={() => {
                actions.toggleZoom(paneId);
                // The chip goes with the zoom; a remote pane stays mounted,
                // so the keyboard returns to its terminal rather than the page.
                focusTerminal(paneId);
              }}
            >
              <Maximize2Icon aria-hidden="true" />
              {hidden > 0 ? <span>+{hidden}</span> : null}
            </Button>
          </Hint>
        ) : null}
        {sleepWords ? (
          <span className="flex min-w-0 items-center gap-xxs truncate text-muted-foreground" data-pane-sleep-caption={sleep?.state}>
            {sleepWords.moon ? <MoonIcon className="size-(--size-status-mark) shrink-0" aria-hidden="true" /> : null}
            <span className="truncate">{sleepWords.text}</span>
          </span>
        ) : caption ? (
          <span className="truncate text-muted-foreground">{caption.text}</span>
        ) : (
          <span className="truncate text-muted-foreground">{pane.status_label}</span>
        )}
        <Hint label={t("panes.actions", { name: title })}>
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
        <Hint label={t("panes.close", { name: title })}>
          <Button variant="ghost" size="icon-sm" className="text-subtle-foreground hover:bg-popover hover:text-foreground" onClick={() => actions.closePane(paneId)}>
            <XIcon />
          </Button>
        </Hint>
        {paneMenu.menu}
      </header>
      <ChildChipRow pane={pane} actions={actions} />
      <div className="h-[var(--size-hairline)] shrink-0 bg-border" />
      <div className="relative min-h-0 flex-1">
        <div ref={hostRef} className="absolute inset-0" data-terminal-host={paneId} onContextMenu={openTerminalMenu} />
        {refusal?.pane_id === paneId ? (
          <div className="absolute inset-x-0 top-0 flex items-center gap-sm bg-card px-sm py-xxs text-caption text-destructive" data-pane-attachment-refusal="true">
            <span className="min-w-0 flex-1 truncate">{refusalText(refusal.reason, t)}</span>
            <Hint label={t("workspace.dismiss")}>
              <Button variant="ghost" size="icon-sm" className="text-muted-foreground" onClick={() => setRefusal(null)}>
                <XIcon />
              </Button>
            </Hint>
          </div>
        ) : null}
        {sleep ? <SleepBody paneId={paneId} sleep={sleep} actions={actions} /> : null}
        {caption?.reconnects && !sleep ? (
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
      {/* Which terminal takes the keys, among several (docs/UI_BEHAVIOR.md):
          drawn while its pane holds the keyboard or its menu is open. */}
      {focused && paneCount > 1 && !zoomed ? (
        <div
          aria-hidden="true"
          className="pointer-events-none absolute inset-0 z-10 hidden border border-subtle-foreground group-focus-within/pane:block group-data-[menu-open=true]/pane:block"
          data-pane-focus-outline="true"
        />
      ) : null}
    </section>
  );
});
