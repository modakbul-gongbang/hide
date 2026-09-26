import { useEffect, useMemo, useRef } from "react";
import { createActions, type Actions } from "./actions";
import { identity, moveBuffer, tabBufferKey, type BufferKey } from "./buffers";
import { BrowserHost } from "./BrowserDisplay";
import { DraftRecoveryLine, refreshRecoveryDrafts } from "./DraftRecovery";
import { pruneDrafts, settleDraft } from "./editor/draft";
import { ConnectionBadge } from "./badge";
import { configureFileBytes } from "./fileBytes";
import { installKeyboard } from "./keyboard";
import { MainScreen } from "./MainScreen";
import { ConfirmClose, ConfirmTrash, CycleOverlay, NoticeBar } from "./Overlays";
import { Palette } from "./Palette";
import { ProjectOverview } from "./ProjectOverview";
import { installProbe, probeEnabled } from "./probe";
import { rememberCheckout, rememberTab } from "./recent";
import { SettingsGate } from "./SettingsSheet";
import { FONT_SIZE_BASE, usableAccent, usableFontSize } from "./settings";
import { ShortcutSheet } from "./ShortcutSheet";
import { Sidebar } from "./sidebar";
import { focusedCheckout, focusedRemoteDevice, frontCheckout } from "./snapshot";
import { OPEN_ANSWER_TIMEOUT_MS, openingProgress, startupScreen } from "./navigation";
import { useShellStore } from "./store";
import { WorkspaceDialogs, WorkspaceNotices } from "./WorkspaceDialogs";
import { applyEditorTheme } from "./editor/theme";
import { applyTerminalTheme, attachedPaneIds, feedChunks, liveTerminalIds, resetAllTerminals, retainTerminals, terminalFor, terminalSelectionText } from "./terminals";
import { primaryValue, readTheme, resolveTheme } from "./theme";
import { TooltipProvider } from "./components/ui/tooltip";
import { useUsageWindowHint } from "./components/weekly-usage";
import { useUiStore } from "./ui";
import { viewRefusal } from "./viewLayout";
import { SessionsScreen } from "./SessionsScreen";
import { WorkspaceScreen } from "./WorkspaceScreen";
import { connectShell, type DispatchFn } from "./ws";

const noop: DispatchFn = () => {};

export function App() {
  const dispatchRef = useRef<DispatchFn>(noop);
  const actions: Actions = useMemo(() => createActions((event) => dispatchRef.current(event)), []);
  useUsageWindowHint(actions);

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
      const fresh = error && error.occurred_at !== previous.rest?.status?.last_error?.occurred_at ? error : null;
      if (fresh?.kind.startsWith("remote.control.")) {
        useUiStore.getState().setNotice({ text: fresh.message, refreshable: fresh.kind === "remote.control.close_status_unknown" });
      }
      // A View action or open the core refused changed nothing, and the
      // operator is told why in the core's words (S7 B19); the keyboard stays
      // where it is rather than waiting for a move that will not land.
      const refusal = viewRefusal(fresh);
      if (refusal) {
        useUiStore.getState().setNotice({ text: refusal, refreshable: false });
        useUiStore.getState().setViewFocusRequest(null);
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

  // Appearance is the core's: the stored accent drives the primary token and
  // the stored interface size scales the interface text tokens, so a reload
  // restores both from the snapshot (B4). A value outside what the sheet
  // offers leaves the token default rather than drawing a guess.
  const accentHex = useShellStore((s) => usableAccent(s.rest?.ui_state?.accent_hex));
  const fontSize = useShellStore((s) => usableFontSize(s.rest?.ui_state?.font_size));
  useEffect(() => {
    const root = document.documentElement.style;
    const primary = primaryValue(accentHex);
    if (primary) root.setProperty("--primary", primary);
    else root.removeProperty("--primary");
    if (fontSize) root.setProperty("--interface-scale", String(fontSize / FONT_SIZE_BASE));
    else root.removeProperty("--interface-scale");
  }, [accentHex, fontSize]);

  // The theme is the core's too (D-14, D-15): System follows the OS
  // appearance while it changes, and the terminals are re-colored in place.
  const storedTheme = useShellStore((s) => s.rest?.ui_state?.theme);
  useEffect(() => {
    const { choice, unknown } = readTheme(storedTheme);
    if (unknown) useShellStore.getState().noteDiagnostic("ui_state.theme is not one this page knows; Dark is shown");
    const query = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const theme = resolveTheme(choice, query.matches);
      const root = document.documentElement.classList;
      root.toggle("dark", theme === "dark");
      root.toggle("light", theme === "light");
      applyTerminalTheme();
      applyEditorTheme();
    };
    apply();
    if (choice !== "system") return;
    query.addEventListener("change", apply);
    return () => query.removeEventListener("change", apply);
  }, [storedTheme]);

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
    <TooltipProvider>
      <div className="relative flex h-full flex-col bg-background text-foreground">
        <ConnectionBadge />
        <NoticeBar actions={actions} />
        <DraftRecoveryLine actions={actions} />
        <WorkspaceNotices actions={actions} />
        <div className="flex min-h-0 flex-1">
          <Sidebar actions={actions} />
          <main className="flex min-h-0 min-w-0 flex-1 flex-col">
            <CenterScreen actions={actions} />
          </main>
        </div>
        <CycleOverlay />
        <ConfirmClose actions={actions} />
        <ConfirmTrash actions={actions} />
        <Palette actions={actions} />
        <ShortcutSheetGate actions={actions} />
        <SettingsGate actions={actions} />
        <WorkspaceDialogs actions={actions} />
        <BrowserHost actions={actions} />
      </div>
    </TooltipProvider>
  );
}

function ShortcutSheetGate({ actions }: { actions: Actions }) {
  const open = useUiStore((s) => s.overlay === "shortcuts");
  return open ? <ShortcutSheet actions={actions} /> : null;
}

/**
 * Main, a Project's Overview, or the front Workspace (PRD S6 D-02, D-11). The
 * page starts on the Workspace the core kept in front when it is the one used
 * last and still in the catalog, and on Main on a first run or when it is
 * gone; while the device in front is still being reached the choice waits,
 * so a Workspace that is about to appear does not first flash Main. A
 * Workspace that goes away while it is shown gives way to Main.
 */
function CenterScreen({ actions }: { actions: Actions }) {
  const screen = useUiStore((s) => s.screen);
  const front = useShellStore((s) => frontCheckout(s.rest)?.id ?? null);
  const hasView = useShellStore((s) => Boolean(s.rest?.workspace_view));
  const first = useShellStore((s) => startupScreen(s.rest, front !== null));
  const loaded = useShellStore((s) => s.rest !== null);
  const remoteFront = useShellStore((s) => focusedRemoteDevice(s.rest) !== null);
  const opening = useUiStore((s) => s.opening);
  const progress = useShellStore((s) => (opening && !opening.failure ? openingProgress(s.rest, opening) : null));
  // An open from Main, an Overview or the Agents list shows its Workspace
  // once the core has moved there; a refusal or no answer stays put and says
  // why on Main or the Overview. On a Workspace the core's error notice
  // already says it, so nothing is kept for later (B2, B21).
  useEffect(() => {
    if (!opening || opening.failure || progress === null) return;
    const store = useUiStore.getState();
    if (progress === "landed") {
      store.setOpening(null);
      store.setScreen({ kind: "workspace" });
      return;
    }
    store.setOpening(store.screen?.kind === "workspace" ? null : { ...opening, failure: progress });
  }, [opening, progress]);
  useEffect(() => {
    if (!opening || opening.failure) return undefined;
    const timer = window.setTimeout(() => {
      const store = useUiStore.getState();
      if (store.opening !== opening) return;
      const failure = "It did not come forward in time; nothing changed.";
      if (store.screen?.kind !== "workspace") return store.setOpening({ ...opening, failure });
      store.setOpening(null);
      store.setNotice({ text: `Not opened: ${failure}`, refreshable: false });
    }, OPEN_ANSWER_TIMEOUT_MS);
    return () => window.clearTimeout(timer);
  }, [opening]);
  useEffect(() => {
    if (screen !== null || !loaded) return undefined;
    if (first) {
      useUiStore.getState().setScreen({ kind: first });
      return undefined;
    }
    // A Herdr or a device that never answers must not hold the page on a
    // blank screen; a device gets longer, since it is reached over SSH.
    const timer = window.setTimeout(() => {
      if (useUiStore.getState().screen === null) useUiStore.getState().setScreen({ kind: "main" });
    }, remoteFront ? 15_000 : 3000);
    return () => window.clearTimeout(timer);
  }, [screen, loaded, first, remoteFront]);
  if (screen === null) {
    return (
      <div className="flex flex-1 items-center justify-center text-caption text-muted-foreground" data-center-screen="starting">
        Opening your last Workspace…
      </div>
    );
  }
  if (screen.kind === "overview") return <ProjectOverview projectId={screen.projectId} actions={actions} />;
  if (screen.kind === "sessions") return <SessionsScreen projectId={screen.projectId} actions={actions} />;
  if (screen.kind === "workspace" && front && hasView) return <WorkspaceScreen actions={actions} />;
  return <MainScreen actions={actions} />;
}
