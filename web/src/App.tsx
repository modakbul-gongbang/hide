import { useEffect, useMemo, useRef } from "react";
import { createActions, type Actions } from "./actions";
import { ConnectionBadge } from "./badge";
import { installKeyboard } from "./keyboard";
import { ConfirmClose, CycleOverlay, FindBar, NoticeBar } from "./Overlays";
import { PaneCanvas } from "./PaneGrid";
import { installProbe, probeEnabled } from "./probe";
import { rememberCheckout, rememberTab } from "./recent";
import { ShortcutSheet } from "./ShortcutSheet";
import { Sidebar } from "./sidebar";
import { focusedCheckout } from "./snapshot";
import { useShellStore } from "./store";
import { TabBar } from "./TabBar";
import { feedChunks, resetAllTerminals, terminalFor } from "./terminals";
import { useUiStore } from "./ui";
import { connectShell, type DispatchFn } from "./ws";

const noop: DispatchFn = () => {};

export function App() {
  const dispatchRef = useRef<DispatchFn>(noop);
  const actions: Actions = useMemo(() => createActions((event) => dispatchRef.current(event)), []);

  useEffect(() => {
    const session = connectShell({
      onChunks: (chunks, full) => {
        if (full) resetAllTerminals();
        feedChunks(chunks);
      },
    });
    dispatchRef.current = session.dispatch;
    const keyboard = installKeyboard(actions);
    if (probeEnabled()) {
      installProbe(
        () => terminalFor(useShellStore.getState().focusedPaneId),
        () => useShellStore.getState().focusedPaneId,
        session.drop,
        () =>
          (useShellStore.getState().rest?.terminal?.panes ?? [])
            .filter((pane) => pane.transport_state !== "released")
            .map((pane) => pane.pane_id),
      );
    }
    // The MRU behind ⌥`/⌥Tab and the project row follows what the core
    // reports as focused, whichever side moved it.
    const unsubscribe = useShellStore.subscribe((state, previous) => {
      if (state.rest === previous.rest) return;
      const checkout = focusedCheckout(state.rest);
      if (!checkout) return;
      const before = focusedCheckout(previous.rest);
      if (checkout.id !== before?.id) rememberCheckout(checkout.id);
      if (checkout.active_tab_id && (checkout.id !== before?.id || checkout.active_tab_id !== before?.active_tab_id)) {
        rememberTab(checkout.id, checkout.active_tab_id);
      }
    });
    return () => {
      unsubscribe();
      keyboard();
      session.close();
    };
  }, [actions]);

  // A reconnect cancels an in-flight drag or cycle; the registration text
  // stays because its component is not remounted (PRD S2 B14).
  const connection = useShellStore((s) => s.connection);
  useEffect(() => {
    if (connection !== "live") useUiStore.getState().setCycle(null);
  }, [connection]);

  return (
    <div className="relative flex h-full flex-col bg-background text-primary">
      <ConnectionBadge />
      <NoticeBar actions={actions} />
      <div className="flex min-h-0 flex-1">
        <Sidebar actions={actions} />
        <main className="flex min-h-0 min-w-0 flex-1 flex-col">
          <TabBar actions={actions} />
          <FindBar actions={actions} />
          <PaneCanvas dispatch={actions.dispatch} onClosePane={(paneId) => actions.closePane(paneId)} />
        </main>
      </div>
      <CycleOverlay />
      <ConfirmClose actions={actions} />
      <ShortcutSheetGate actions={actions} />
    </div>
  );
}

function ShortcutSheetGate({ actions }: { actions: Actions }) {
  const open = useUiStore((s) => s.overlay === "shortcuts");
  return open ? <ShortcutSheet actions={actions} /> : null;
}
