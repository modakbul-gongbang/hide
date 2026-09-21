import { memo, useEffect, useRef } from "react";
import type { PaneRow, TerminalPane } from "./snapshot";
import { useShellStore } from "./store";
import { focusTerminal, mountTerminal, requestView, setTextScale } from "./terminals";
import type { DispatchFn } from "./ws";

/** Transport states with a live stream; anything else is drawn as a caption in the header. */
const LIVE_STATES = new Set(["connected", "controlling", "idle"]);

export function paneTitle(pane: PaneRow): string {
  return pane.identity_label ?? pane.terminal_title ?? pane.herdr_label ?? pane.id;
}

/** The caption a non-live transport state gets, and whether a click asks the core to reattach. */
export function transportCaption(transport: TerminalPane | undefined): { text: string; reconnects: boolean } | null {
  if (!transport || LIVE_STATES.has(transport.transport_state)) return null;
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
  dispatch,
  onClose,
}: {
  pane: PaneRow;
  transport: TerminalPane | undefined;
  focused: boolean;
  scale: number;
  dispatch: DispatchFn;
  onClose: (paneId: string) => void;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  const viewGeneration = useShellStore((s) => s.viewGeneration);
  const paneId = pane.id;

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    return mountTerminal(paneId, host, dispatch, useShellStore.getState().rest?.ui_state?.pane_text_scales?.[paneId] ?? 1);
  }, [paneId, dispatch]);

  // After every self-contained snapshot the core may have restarted, so the
  // pane asks for a full frame rather than trusting what it has drawn. The
  // mount already requested one, so the first generation is skipped.
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

  const caption = transportCaption(transport);
  return (
    <section
      className="flex h-full min-h-0 min-w-0 flex-col bg-background"
      data-pane-view={paneId}
      data-focused={focused ? "true" : "false"}
      data-transport={transport?.transport_state ?? ""}
    >
      <header
        className={`flex h-[var(--size-pane-header)] shrink-0 items-center gap-sm px-sm text-caption ${
          focused ? "bg-elevated text-primary" : "bg-panel text-secondary"
        }`}
      >
        <span className="min-w-0 flex-1 truncate">{paneTitle(pane)}</span>
        {caption ? (
          <span className="truncate text-muted">{caption.text}</span>
        ) : (
          <span className="truncate text-muted">{pane.status_label}</span>
        )}
        <button
          type="button"
          className="flex h-[var(--size-icon-button-toolbar)] w-[var(--size-icon-button-toolbar)] items-center justify-center rounded-xs text-secondary hover:bg-balloon hover:text-primary"
          aria-label={`Close pane ${paneTitle(pane)}`}
          title="Close pane"
          onClick={() => onClose(paneId)}
        >
          ×
        </button>
      </header>
      <div className="h-[var(--size-hairline)] shrink-0 bg-divider" />
      <div className="relative min-h-0 flex-1">
        <div ref={hostRef} className="absolute inset-0" />
        {caption?.reconnects ? (
          <button
            type="button"
            className="absolute inset-0 flex items-center justify-center bg-panel text-caption text-secondary"
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
