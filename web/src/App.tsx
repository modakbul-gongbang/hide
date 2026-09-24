import { useEffect, useMemo, useRef } from "react";
import { createActions, type Actions } from "./actions";
import { allBuffers, claimLegacyBuffer, deleteBuffer, discardLegacyBuffers, flushBuffer, identity, moveBuffer, staleBuffers, sweepBuffers } from "./buffers";
import { pruneDrafts, settleDraft } from "./editor/draft";
import { ConnectionBadge } from "./badge";
import { EditorSurface } from "./Editor";
import { configureFileBytes } from "./fileBytes";
import { installKeyboard } from "./keyboard";
import { ConfirmClose, ConfirmTrash, CycleOverlay, FindBar, NoticeBar } from "./Overlays";
import { PaneCanvas } from "./PaneGrid";
import { Palette } from "./Palette";
import { installProbe, probeEnabled } from "./probe";
import { rememberCheckout, rememberTab } from "./recent";
import { RightPanel } from "./RightPanel";
import { ShortcutSheet } from "./ShortcutSheet";
import { Sidebar } from "./sidebar";
import { checkoutById, editorFor, focusedCheckout } from "./snapshot";
import { useShellStore } from "./store";
import { TabBar } from "./TabBar";
import { attachedPaneIds, feedChunks, liveTerminalIds, resetAllTerminals, retainTerminals, terminalFor, terminalSelectionText } from "./terminals";
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
    configureFileBytes(session.dispatch);
    const keyboard = installKeyboard(actions);
    if (probeEnabled()) {
      installProbe(
        () => terminalFor(useShellStore.getState().focusedPaneId),
        () => useShellStore.getState().focusedPaneId,
        session.drop,
        attachedPaneIds,
        liveTerminalIds,
        (paneId) => terminalFor(paneId),
        terminalSelectionText,
      );
    }
    // The MRU behind ⌥`/⌥Tab and the project row follows what the core
    // reports as focused, whichever side moved it.
    const unsubscribe = useShellStore.subscribe((state, previous) => {
      if (state.rest === previous.rest) return;
      // A terminal lives as long as the core streams its pane; released or
      // vanished panes lose theirs here, never on a tab switch (D-05).
      if (state.rest?.terminal?.panes !== previous.rest?.terminal?.panes) retainTerminals();
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

  // A save mark belongs to a tab that is still unsaved: once the core reports
  // the tab clean, the mark comes off whichever tab is showing (D-10).
  const editorTabs = useShellStore((s) => s.editor?.tabs);
  const identities = useRef(new Map<string, { root: string; path: string }>());
  useEffect(() => {
    if (!editorTabs) return;
    const rest = useShellStore.getState().rest;
    const open = new Set<string>();
    for (const tab of editorTabs) {
      open.add(tab.id);
      const root = checkoutById(rest, tab.checkout_id)?.path ?? "";
      const before = identities.current.get(tab.id);
      identities.current.set(tab.id, { root, path: tab.path });
      // A rename or a move retargets the stored buffer to the new identity,
      // showing tab or background tab alike (D-14).
      if (before && (before.root !== root || before.path !== tab.path)) {
        void moveBuffer(before.root, before.path, root, tab.path).then((outcome) => {
          if (outcome === "failed" && tab.dirty) useShellStore.getState().noteBufferWarning(tab.id, true);
        });
      }
    }
    for (const tabId of [...identities.current.keys()]) {
      if (!open.has(tabId)) identities.current.delete(tabId);
    }
    // A tab id names a path, so a buffer for a tab that is gone would be
    // applied to whatever opens at that path next (B5, D-14).
    pruneDrafts(open);
    for (const tab of editorTabs) if (!tab.dirty) settleDraft(tab.id);
    const state = useShellStore.getState();
    if (state.savingTabs.size === 0) return;
    const dirty = new Set(editorTabs.filter((tab) => tab.dirty).map((tab) => tab.id));
    for (const tabId of state.savingTabs) if (!dirty.has(tabId)) state.noteSaving(tabId, false);
  }, [editorTabs]);

  // A buffer whose document the core no longer holds is discarded with a
  // diagnostic; an open document's buffer is restored by its editor (B8).
  useEffect(() => {
    if (connection !== "live") return;
    const rest = useShellStore.getState().rest;
    const documents = (useShellStore.getState().editor?.tabs ?? [])
      .filter((tab) => tab.kind === "file")
      .map((tab) => ({ root: checkoutById(rest, tab.checkout_id)?.path ?? "", path: tab.path }));
    const open = new Set(documents.map(({ root, path }) => identity(root, path)));
    void (async () => {
      await Promise.all(documents.map(({ root, path }) => flushBuffer(root, path)));
      await Promise.all(documents.map(({ root, path }) => claimLegacyBuffer(root, path)));
      const legacyDiscarded = await discardLegacyBuffers(new Set(documents.map(({ path }) => path)));
      for (const path of legacyDiscarded) {
        useShellStore.getState().noteDiagnostic(`discarded the unsaved buffer for closed document ${path}`);
      }
      const buffers = await allBuffers();
      // A buffer the core no longer holds is discarded, and one that has gone
      // unclaimed for two weeks goes with it (D-14).
      const discarded = [...sweepBuffers(buffers, open), ...staleBuffers(buffers, Date.now())];
      for (const buffer of discarded) {
        useShellStore
          .getState()
          .noteDiagnostic(`discarded the unsaved buffer for closed document ${buffer.path}`);
        void deleteBuffer(buffer.root, buffer.path);
      }
    })();
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
          <Canvas actions={actions} />
        </main>
        <RightPanel actions={actions} />
      </div>
      <CycleOverlay />
      <ConfirmClose actions={actions} />
      <ConfirmTrash actions={actions} />
      <Palette actions={actions} />
      <ShortcutSheetGate actions={actions} />
    </div>
  );
}

function ShortcutSheetGate({ actions }: { actions: Actions }) {
  const open = useUiStore((s) => s.overlay === "shortcuts");
  return open ? <ShortcutSheet actions={actions} /> : null;
}

/**
 * The center surface. The core keeps the editor's tabs while a terminal tab
 * shows, so the editor's own `active_tab_id` is what says which one is drawn;
 * with none, the focused checkout's visible tab is the terminal canvas.
 */
function Canvas({ actions }: { actions: Actions }) {
  const editorShowing = useShellStore((s) => editorFor(s.editor) !== null);
  return editorShowing ? (
    <EditorSurface actions={actions} />
  ) : (
    <PaneCanvas dispatch={actions.dispatch} onClosePane={(paneId) => actions.closePane(paneId)} />
  );
}
