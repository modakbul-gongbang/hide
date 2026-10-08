import { InterfaceLanguageBoundary, translate, useInterfaceTranslation } from "./i18n/client";
import { agentCapacityNotice } from "./agentLayout";
import { useEffect, useMemo, useRef } from "react";
import { createActions, type Actions } from "./actions";
import { identity, moveBuffer, tabBufferKey, type BufferKey } from "./buffers";
import { BrowserHost } from "./BrowserDisplay";
import { freshError, watchErrorNotices } from "./errorNotice";
import { DraftRecoveryLine, refreshRecoveryDrafts } from "./DraftRecovery";
import { pruneDrafts, settleDraft } from "./editor/draft";
import { ConnectionBadge } from "./badge";
import { configureFileBytes } from "./fileBytes";
import { installKeyboard, observeRecent, reconcileHeldCycle } from "./keyboard";
import { noteOperatorPointer, observeInputRequests } from "./keyTarget";
import { OverviewModal, OverviewPage } from "./Overview";
import { FactoryScreen } from "./factory/FactoryScreen";
import { useFactoryNotifications } from "./factory/notify";
import { AgentCloseNotice, ConfirmClose, ConfirmTrash, CycleOverlay, NoticeBar } from "./Overlays";
import { Palette } from "./Palette";
import { installProbe, probeEnabled } from "./probe";
import { configurePaneVisits, expectPane, expectSurface, focusSignature } from "./recent";
import { AgentOnboardingGate } from "./AgentOnboarding";
import { SettingsGate } from "./SettingsSheet";
import { StartPanelHost } from "./StartPanel";
import { FONT_SIZE_BASE, usableAccent, usableFontSize } from "./settings";
import { ShortcutSheet } from "./ShortcutSheet";
import { Sidebar } from "./sidebar";
import { focusedRemoteDevice, frontCheckout } from "./snapshot";
import { OPEN_ANSWER_TIMEOUT_MS, openingProgress, startupScreen } from "./navigation";
import { useShellStore } from "./store";
import { AddProjectDialog } from "./AddProjectDialog";
import { WorkspaceDialogs, WorkspaceNotices } from "./WorkspaceDialogs";
import { applyEditorTheme } from "./editor/theme";
import { applyGridHolds, applyTerminalTheme, attachedPaneIds, feedChunks, liveTerminalIds, resetAllTerminals, retainTerminals, terminalFor, terminalSelectionText } from "./terminals";
import { primaryValue, readTheme, resolveTheme } from "./theme";
import { TooltipProvider } from "./components/ui/tooltip";
import { useUsageWindowHint } from "./components/weekly-usage";
import { useUiStore } from "./ui";
import { WorkspaceScreen } from "./WorkspaceScreen";
import { connectShell, type DispatchFn } from "./ws";

const noop: DispatchFn = () => {};

export function App() {
  const dispatchRef = useRef<DispatchFn>(noop);
  const actions: Actions = useMemo(() => createActions((event) => dispatchRef.current(event)), []);
  useUsageWindowHint(actions);
  useFactoryNotifications(actions);

  useEffect(() => {
    const session = connectShell({
      onChunks: (chunks, full) => {
        if (full) resetAllTerminals();
        feedChunks(chunks);
      },
    });
    dispatchRef.current = session.dispatch;
    configureFileBytes(session.dispatch);
    configurePaneVisits(actions.paneVisit);
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
    // Recent Panels and Recent Projects follow what the operator uses: the
    // core's focus, whichever side moved it, and where the keyboard is, since
    // moving between a checkout's terminal and its View area changes no core
    // state. The operator's next press or click ends a commit's wait.
    // Focus that lands outside both areas (the sidebar, a palette) is not a
    // move between surfaces.
    const observeFocus = (event: FocusEvent) => {
      if (event.target instanceof Element && event.target.closest("[data-agent-area], [data-view-area]")) observeRecent(useShellStore.getState().rest, true);
    };
    // A chord the shell answered (the next cycle) and a bare modifier are
    // not the operator acting on the surface a commit is bringing forward.
    const endCommit = (event: Event) => {
      if (event.defaultPrevented) return;
      if (event instanceof KeyboardEvent && ["Control", "Alt", "Shift", "Meta"].includes(event.key)) return;
      expectSurface(null);
      expectPane(null);
    };
    window.addEventListener("focusin", observeFocus);
    window.addEventListener("pointerdown", endCommit, true);
    // Pointing anywhere is the operator choosing where they are, so keys stop
    // following a pending new tab or split (`keyTarget.ts`).
    window.addEventListener("pointerdown", noteOperatorPointer, true);
    window.addEventListener("keydown", endCommit, true);
    // All projects and an Overview are visits of their own, and leaving one
    // for the Workspace makes the surface there the one in use; neither
    // changes core state, so the page's screen is watched here.
    const unsubscribeScreen = useUiStore.subscribe((state, previous) => {
      if (state.screen !== previous.screen) observeRecent(useShellStore.getState().rest, true);
    });
    // A failure the operator can act on is a notice rather than only a diagnostic (design 13); it is raised
    // before the rest of this subscriber runs, as it always was.
    const unsubscribeNotices = watchErrorNotices();
    const unsubscribe = useShellStore.subscribe((state, previous) => {
      if (state.rest === previous.rest) return;
      // A terminal lives as long as the core streams its pane; released or
      // vanished panes lose theirs here, never on a tab switch (D-05).
      if (state.rest?.terminal?.panes !== previous.rest?.terminal?.panes) {
        retainTerminals();
        applyGridHolds(state.rest?.terminal?.panes);
      }
      if (state.rest?.terminal?.input_requests !== previous.rest?.terminal?.input_requests) observeInputRequests(state.rest?.terminal?.input_requests);
      const fresh = freshError(state, previous);
      const waiting = state.rest?.workspace_view?.agent_layout?.waiting ?? 0;
      const previouslyWaiting = previous.rest?.workspace_view?.agent_layout?.waiting ?? 0;
      if (waiting !== previouslyWaiting || state.rest?.workspace_view?.path !== previous.rest?.workspace_view?.path) {
        if (waiting > 0) useUiStore.getState().setNotice({ text: agentCapacityNotice(waiting), refreshable: false });
        else if (previouslyWaiting > 0 && useUiStore.getState().notice?.text === agentCapacityNotice(previouslyWaiting)) useUiStore.getState().setNotice(null);
      }
      // A refused commit brings nothing forward, so the next surface in use is a visit again.
      if (fresh) {
        expectSurface(null);
        expectPane(null);
      }
      if (state.rest?.navigator === previous.rest?.navigator && state.rest?.workspace_view === previous.rest?.workspace_view && state.rest?.focused === previous.rest?.focused) return;
      observeRecent(state.rest, focusSignature(state.rest) !== focusSignature(previous.rest));
      const cycle = useUiStore.getState().cycle;
      if (cycle) useUiStore.getState().setCycle(reconcileHeldCycle(cycle, state.rest));
    });
    return () => {
      window.removeEventListener("focusin", observeFocus);
      window.removeEventListener("pointerdown", endCommit, true);
      window.removeEventListener("pointerdown", noteOperatorPointer, true);
      window.removeEventListener("keydown", endCommit, true);
      unsubscribeScreen();
      unsubscribeNotices();
      unsubscribe();
      keyboard();
      configurePaneVisits(() => {});
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
      <InterfaceLanguageBoundary />
      <div className="relative flex h-full flex-col bg-background text-foreground">
        <ConnectionBadge />
        <NoticeBar actions={actions} />
        <AgentCloseNotice actions={actions} />
        <DraftRecoveryLine actions={actions} />
        <WorkspaceNotices actions={actions} />
        <div className="flex min-h-0 flex-1">
          <Sidebar actions={actions} />
          <main className="flex min-h-0 min-w-0 flex-1 flex-col">
            <CenterScreen actions={actions} />
          </main>
        </div>
        <OverviewModal actions={actions} />
        <CycleOverlay />
        <ConfirmClose actions={actions} />
        <ConfirmTrash actions={actions} />
        <Palette actions={actions} />
        <StartPanelHost actions={actions} />
        <ShortcutSheetGate actions={actions} />
        <SettingsGate actions={actions} />
        <AgentOnboardingGate actions={actions} />
        <WorkspaceDialogs actions={actions} />
        <AddProjectDialog actions={actions} />
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
 * All projects, a Project's Overview, or the front Workspace (PRD S6 D-02, D-11). The
 * page starts on the Workspace the core kept in front when it is the one used
 * last and still in the catalog, and on All projects on a first run or when it is
 * gone; while the device in front is still being reached the choice waits,
 * so a Workspace that is about to appear does not first flash All projects. A
 * Workspace that goes away while it is shown gives way to All projects.
 */
function CenterScreen({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const screen = useUiStore((s) => s.screen);
  const front = useShellStore((s) => frontCheckout(s.rest)?.id ?? null);
  const hasView = useShellStore((s) => Boolean(s.rest?.workspace_view));
  const first = useShellStore((s) => startupScreen(s.rest, front !== null));
  const loaded = useShellStore((s) => s.rest !== null);
  const remoteFront = useShellStore((s) => focusedRemoteDevice(s.rest) !== null);
  const opening = useUiStore((s) => s.opening);
  const progress = useShellStore((s) => (opening && !opening.failure ? openingProgress(s.rest, opening) : null));
  // An open from All projects, an Overview or the Agents list shows its Workspace
  // once the core has moved there; a refusal or no answer stays put and says
  // why on All projects or the Overview. On a Workspace the core's error notice
  // already says it, so nothing is kept for later (B2, B21).
  useEffect(() => {
    if (!opening || opening.failure || progress === null) return;
    const store = useUiStore.getState();
    if (progress === "cancelled") {
      store.setOpening(null);
      return;
    }
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
      const failure = translate("shell.openTimeout");
      if (store.screen?.kind !== "workspace") return store.setOpening({ ...opening, failure });
      store.setOpening(null);
      store.setNotice({ text: translate("shell.notOpened", { reason: failure }), refreshable: false });
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
        {t("shell.openingLastWorkspace")}
      </div>
    );
  }
  if (screen.kind === "factory") return <FactoryScreen actions={actions} />;
  if (screen.kind === "workspace" && front && hasView) return <WorkspaceScreen actions={actions} />;
  return <OverviewPage actions={actions} />;
}
