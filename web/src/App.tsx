import { useEffect, useMemo, useRef } from "react";
import { createActions, type Actions } from "./actions";
import { identity, moveBuffer, tabBufferKey, type BufferKey } from "./buffers";
import { DraftRecoveryLine, refreshRecoveryDrafts } from "./DraftRecovery";
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
import { RemoteSurface } from "./RemoteSurface";
import { RightPanel } from "./RightPanel";
import { SettingsGate } from "./SettingsSheet";
import { FONT_SIZE_BASE, usableAccent, usableFontSize } from "./settings";
import { ShortcutSheet } from "./ShortcutSheet";
import { Sidebar } from "./sidebar";
import { editorFor, focusedCheckout, focusedRemoteDevice } from "./snapshot";
import { useShellStore } from "./store";
import { TabBar } from "./TabBar";
import { WorkspaceDialogs, WorkspaceNotices } from "./WorkspaceDialogs";
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
      // A command a remote host refused is one the operator can act on (a
      // lost connection, a close that needs confirming), so it is a notice
      // rather than only a diagnostic (design 13).
      // A notice about one device's command does not outlive the device
      // context it was about.
      if (state.rest?.navigator?.focused_device_id !== previous.rest?.navigator?.focused_device_id) {
        useUiStore.getState().setNotice(null);
      }
      const error = state.rest?.status?.last_error;
      if (error && error.occurred_at !== previous.rest?.status?.last_error?.occurred_at && error.kind.startsWith("remote.control.")) {
        useUiStore.getState().setNotice({ text: error.message, refreshable: error.kind === "remote.control.close_status_unknown" });
      }
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

  // Appearance is the core's: the stored accent replaces the accent token and
  // the stored interface size scales the interface text tokens, so a reload
  // restores both from the snapshot (B4). A value outside what the sheet
  // offers leaves the token default rather than drawing a guess.
  const accentHex = useShellStore((s) => usableAccent(s.rest?.ui_state?.accent_hex));
  const fontSize = useShellStore((s) => usableFontSize(s.rest?.ui_state?.font_size));
  useEffect(() => {
    const root = document.documentElement.style;
    if (accentHex) root.setProperty("--color-accent", accentHex);
    else root.removeProperty("--color-accent");
    if (fontSize) root.setProperty("--interface-scale", String(fontSize / FONT_SIZE_BASE));
    else root.removeProperty("--interface-scale");
  }, [accentHex, fontSize]);

  // A reconnect cancels an in-flight drag or cycle; the registration text
  // stays because its component is not remounted (PRD S2 B14).
  const connection = useShellStore((s) => s.connection);
  useEffect(() => {
    if (connection !== "live") useUiStore.getState().setCycle(null);
  }, [connection]);

  // A save mark belongs to a tab that is still unsaved: once the core reports
  // the tab clean, the mark comes off whichever tab is showing (D-10).
  const editorTabs = useShellStore((s) => s.editor?.tabs);
  const identities = useRef(new Map<string, BufferKey>());
  const host = useShellStore((s) => s.daemon?.host_id ?? null);
  useEffect(() => {
    if (!editorTabs) return;
    const rest = useShellStore.getState().rest;
    const open = new Set<string>();
    for (const tab of editorTabs) {
      if (tab.kind !== "file") continue;
      open.add(tab.id);
      const key = tabBufferKey(host, rest, tab);
      if (!key) continue;
      const before = identities.current.get(tab.id);
      identities.current.set(tab.id, key);
      // A rename, a move, or a device checkout confirmed at its repository
      // root retargets the stored draft to the new identity, showing tab or
      // background tab alike (D-14).
      if (before && identity(before) !== identity(key)) {
        void moveBuffer(before, key).then((outcome) => {
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
  }, [editorTabs, host]);

  // Drafts no open tab stands for are recovery items: a daemon restart that
  // lost its tabs, another device's or host's documents, or drafts from
  // before drafts named their device. None is discarded on its own (B10-B12).
  const openDocuments = useShellStore((s) => (s.editor?.tabs ?? []).filter((tab) => tab.kind === "file").map((tab) => tab.id).join("\n"));
  useEffect(() => {
    if (connection !== "live") return;
    void refreshRecoveryDrafts();
  }, [connection, openDocuments, host]);

  return (
    <div className="relative flex h-full flex-col bg-background text-primary">
      <ConnectionBadge />
      <NoticeBar actions={actions} />
      <DraftRecoveryLine actions={actions} />
      <WorkspaceNotices actions={actions} />
      <div className="flex min-h-0 flex-1">
        <Sidebar actions={actions} />
        <main className="flex min-h-0 min-w-0 flex-1 flex-col">
          <MainSurface actions={actions} />
        </main>
        <RightPanel actions={actions} />
      </div>
      <CycleOverlay />
      <ConfirmClose actions={actions} />
      <ConfirmTrash actions={actions} />
      <Palette actions={actions} />
      <ShortcutSheetGate actions={actions} />
      <SettingsGate actions={actions} />
      <WorkspaceDialogs actions={actions} />
    </div>
  );
}

function ShortcutSheetGate({ actions }: { actions: Actions }) {
  const open = useUiStore((s) => s.overlay === "shortcuts");
  return open ? <ShortcutSheet actions={actions} /> : null;
}

/** This machine's tabs, or the selected SSH device's own tabs and panes in their place (B19). */
function MainSurface({ actions }: { actions: Actions }) {
  const remote = useShellStore((s) => focusedRemoteDevice(s.rest) !== null);
  if (remote) return <RemoteSurface actions={actions} />;
  return (
    <>
      <TabBar actions={actions} />
      <FindBar actions={actions} />
      <Canvas actions={actions} />
    </>
  );
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
