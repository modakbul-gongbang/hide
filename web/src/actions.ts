// Every shell command in one place, so a shortcut, a button and a menu run
// the same code against the same snapshot. Each action is one core event
// (dispatch is fire-and-forget; a sequence would arrive as several frames).

import type { UiStateUsageHints } from "./generated/hided-ws";
import type { ThemeChoice } from "./theme";
import type { HostKind } from "./host";
import {
  closeWithSaveOutcome,
  deleteBuffer,
  documentCloseCarries,
  flushBuffer,
  settledBuffer,
  storedDraftOnClose,
  tabBufferKey,
  type BufferKey,
  type CloseWatch,
  type CloseWatchFrame,
} from "./buffers";
import { closeDecision, statusUnknownNotice } from "./close";
import { draftExported, unstoredDeviceDrafts, type SettingsTab } from "./settings";
import { latestDraft, noteClosing, noteSent } from "./editor/draft";
import { RELATION_ANSWER_TIMEOUT_MS, relationState } from "./lineage";
import type { OpenTarget } from "./navigation";
import { REGISTERED_CHECKOUT, remoteConnected, remoteContext, remoteControl, remoteRequestId, remoteTargetOfPane, remoteView, inPlace as inPlaceEvent, withDeviceForward, type RemoteAction, type RemoteView } from "./remote";
import {
  catalogWorkspaces,
  deviceOfCheckout,
  editorFor,
  editorTabFor,
  explorerContext,
  focusedCheckout,
  frontCheckout,
  visibleTab,
  type AgentRow,
  type Checkout,
  type EditorDocumentSnapshot,
  type EditorTabSnapshot,
  type Tab,
  type Workspace,
} from "./snapshot";
import { fileUrl } from "./browserViews";
import { useShellStore } from "./store";
import { SIDEBAR_MODES, useUiStore, type SidebarMode } from "./ui";
import type { DispatchFn } from "./ws";
import { drawnViews, viewAreaInUse } from "./viewFocus";
import {
  activeDisplay,
  adjacentInOrder,
  areasOf,
  displaysOfDocument,
  findArea,
  isMenuCommand,
  locateDisplay,
  menuEdge,
  neighbourArea,
  resizeTarget,
  viewCommands,
  viewLayoutPayload,
  workspaceKey,
  type Edge,
  type ViewCommandId,
  type ViewFrame,
  type ViewMenuId,
  type ViewWorkspace,
} from "./viewLayout";
import { workspaceViewOf, type PanelState } from "./workspace";

/** The `device_id` an event carries: none for this machine, which the core takes as the default. */
function deviceField(device: string): { device_id?: string } {
  return device === "local" ? {} : { device_id: device };
}

export type Actions = ReturnType<typeof createActions>;

export function createActions(dispatch: DispatchFn) {
  const rest = () => useShellStore.getState().rest;
  const diagnostic = (message: string) => useShellStore.getState().noteDiagnostic(message);
  const ui = () => useUiStore.getState();

  /** Remembers what was asked to come forward; `CenterScreen` shows it once it is in front. */
  const beginOpening = (target: OpenTarget) => {
    ui().setOpening({ target, errorBefore: rest()?.status?.last_error?.occurred_at ?? null, failure: null });
  };

  const current = (): { checkout: Checkout; tab: Tab | null } | null => {
    const checkout = focusedCheckout(rest());
    return checkout ? { checkout, tab: visibleTab(checkout) } : null;
  };

  /**
   * The checkout an Explorer change is made in: the one in front on the
   * selected device, named with that device so the core refuses the change
   * once another device's tree is on screen (S5.5 B16, B34).
   */
  const explorerTarget = (): { root: string; device_id?: string } | null => {
    const context = explorerContext(rest());
    if (!context.checkout) return null;
    return { root: context.checkout.path, ...deviceField(context.device) };
  };

  const setLeftSidebarVisible = (visible: boolean) => {
    const state = rest()?.ui_state;
    if (!state || state.left_sidebar_visible === visible) return;
    updateUiState({ left_sidebar_visible: visible });
  };

  /**
   * The core's ui state with one patch applied. The event replaces the whole
   * state rather than merging it, so every field the snapshot carries rides
   * along; a field the web omits would fall back to the core's default and
   * erase a value another surface owns. The two registration lists are the
   * exception: their own events (`register_device`, `workspace_pin_set`, …)
   * own them and the core keeps them when they are absent, so an echo of an
   * older snapshot can never undo a registration that landed after it.
   */
  const updateUiState = (patch: Record<string, unknown>) => {
    const state = rest()?.ui_state;
    if (!state) return;
    const { workspace_registrations: _workspaces, device_registrations: _devices, ...owned } = state;
    void _workspaces;
    void _devices;
    dispatch({ schema_version: 2, kind: "ui_state_update", payload: { ...owned, ...patch } });
  };

  /**
   * The selected host, when a pane or tab command can go to it now. A device
   * that is not connected gets a notice and nothing is sent; the core would
   * refuse it too (`remote.control.not_connected`), but only in its log.
   */
  const remoteHost = (command: string): { targetId: string; view: RemoteView | null; agents: AgentRow[] } | null => {
    const context = remoteContext(rest());
    if (!context) return null;
    if (!remoteConnected(context)) {
      ui().setNotice({ text: `${context.device.label} is not connected. ${command} was not sent.`, refreshable: false });
      return null;
    }
    return { targetId: context.device.id, view: remoteView(context.session), agents: context.session?.agents ?? [] };
  };

  const sendRemote = (targetId: string, request: RemoteAction) => dispatch(remoteControl(targetId, request));

  /**
   * A child chip, a Return or a relationship Open (S6 B15, B16): one
   * focus_pane carrying a request id, so the core's receipt says whether this
   * click landed. A second click on the same target while the first is in
   * flight is dropped. A pane on another device is asked for there, and the
   * device's answer comes back as the same receipt.
   */
  const followRelation = (sourcePaneId: string, targetPaneId: string, label: string) => {
    const current = ui().relation;
    if (current?.targetPaneId === targetPaneId && relationState(current, rest()?.status?.pane_focus_request)?.phase === "pending") return;
    const requestId = remoteRequestId();
    ui().setRelation({ requestId, sourcePaneId, targetPaneId, label });
    setTimeout(() => {
      const asked = ui().relation;
      if (asked?.requestId !== requestId || relationState(asked, rest()?.status?.pane_focus_request)?.phase !== "pending") return;
      ui().setRelation({ ...asked, timedOut: true });
    }, RELATION_ANSWER_TIMEOUT_MS);
    ui().setScreen({ kind: "workspace" });
    const targetId = remoteTargetOfPane(rest(), targetPaneId);
    if (targetId) {
      // Chips, Returns and Opens sit in the Agent area on screen, beside the side panel (issue 170).
      dispatch({
        schema_version: 2,
        kind: "remote_control",
        payload: { target_id: targetId, request_id: requestId, report_pane_focus_outcome: true, in_place: true, action: "focus_pane", pane_id: targetPaneId },
      });
      return;
    }
    dispatch({ schema_version: 2, kind: "focus_pane", payload: { pane_id: targetPaneId, origin: "operator", request_id: requestId, in_place: true } });
  };

  /** The id of the core's task slot now, so a request can tell its own answer from an older one. */
  const taskIdNow = () => rest()?.task_operation?.id ?? 0;

  /**
   * One close through the Swift flow. `targetId` names the SSH device the
   * pane or tab lives on, and the close goes there as `remote_control`; a
   * local close is the core's own `close_pane`/`close_tab`.
   */
  const requestClose = (kind: "pane" | "tab", id: string, panes: Tab["panes"], targetId: string | null, agents: AgentRow[]) => {
    const decision = closeDecision(kind, panes, agents);
    if (decision.action === "status_unknown") {
      ui().setNotice({ text: statusUnknownNotice(decision.label), refreshable: true });
      return;
    }
    if (decision.action === "confirm") {
      ui().setPendingClose({ kind, id, targetId, title: decision.title, consequence: decision.consequence, affected: decision.affected });
      return;
    }
    sendClose(kind, id, targetId, false);
  };

  const sendClose = (kind: "pane" | "tab", id: string, targetId: string | null, confirmed: boolean) => {
    if (targetId) {
      sendRemote(targetId, kind === "pane" ? { action: "close_pane", pane_id: id, confirmed } : { action: "close_tab", tab_id: id, confirmed });
      return;
    }
    dispatch({
      schema_version: 2,
      kind: kind === "pane" ? "close_pane" : "close_tab",
      payload: kind === "pane" ? { pane_id: id, confirmed } : { tab_id: id, confirmed },
    });
  };

  /**
   * A project or checkout row. On a selected SSH device a project can hold
   * several Herdr workspaces, and the checkout names the one to focus.
   */
  const focusCheckout = (workspaceId: string, checkoutId: string) => {
    const context = remoteContext(rest());
    if (context) {
      const host = remoteHost("Switching workspace");
      if (!host) return;
      const project = context.session?.workspaces.find((row) => row.id === workspaceId);
      const checkout = project?.checkouts.find((row) => row.id === checkoutId);
      if (!checkout) {
        return diagnostic(`focus_workspace: ${checkoutId} is not on ${context.device.label}`);
      }
      // A registered project Herdr has no workspace in yet is opened by
      // creating one at its folder there (find-or-create, as on this machine).
      if (checkoutId.endsWith(REGISTERED_CHECKOUT)) {
        sendRemote(host.targetId, { action: "create_tab", workspace_id: workspaceId, checkout_id: checkoutId, cwd: checkout.path, label: checkout.next_tab_label });
        return;
      }
      sendRemote(host.targetId, { action: "focus_workspace", workspace_id: workspaceId, checkout_id: checkoutId });
      return;
    }
    dispatch({ schema_version: 2, kind: "focus_checkout", payload: { workspace_id: workspaceId, checkout_id: checkoutId } });
  };

  /**
   * Keyboard focus to one pane. The pane's own id says which host it is on,
   * so a remote pane is focused there and a local one here, whatever the
   * context was when the click started.
   */
  const focusPane = (paneId: string) => {
    const targetId = remoteTargetOfPane(rest(), paneId);
    if (targetId) {
      sendRemote(targetId, { action: "focus_pane", pane_id: paneId });
      return;
    }
    if (remoteContext(rest())) return diagnostic(`focus_pane: ${paneId} is not on the selected device`);
    dispatch({ schema_version: 2, kind: "focus_pane", payload: { pane_id: paneId, origin: "operator" } });
  };

  /** Switches the left sidebar's mode; the Explorer is a right panel now. */
  const showSidebarMode = (mode: SidebarMode) => {
    ui().setSidebarMode(mode);
  };

  /**
   * The front Workspace's side panel and tools (S6 D-05, issue 170): one
   * `workspace_view` event naming only what changes. The core keeps them per
   * Workspace, so another Workspace is never touched.
   */
  const setWorkspaceView = (patch: { panel?: PanelState; pinned?: boolean; explorer?: boolean; changes?: boolean; views_over_share?: number; reveal?: string }) => {
    if (!workspaceViewOf(rest())) return diagnostic("workspace_view: no Workspace in front");
    dispatch({ schema_version: 2, kind: "workspace_view", payload: patch });
  };

  /**
   * Shows one tool (B10). The operator asked for it, so a narrow panel's
   * overlay opens (S7 B12), and a closed panel opens with it (issue 170, the
   * core's rule); nothing is sent when the open panel already shows it, since
   * the overlay closing never stored it hidden.
   */
  const showTool = (tool: "explorer" | "changes") => {
    const view = workspaceViewOf(rest());
    if (!view) return diagnostic("workspace_view: no Workspace in front");
    ui().openTools();
    if (view.panel === "closed" || !(tool === "explorer" ? view.explorer : view.changes)) setWorkspaceView(tool === "explorer" ? { explorer: true } : { changes: true });
  };

  const showExplorerPanel = () => showTool("explorer");

  /** ⌘⇧B: closes the side panel when it shows, expanded or not, else opens it at its width (issue 170). */
  const toggleRightPanel = () => {
    const view = workspaceViewOf(rest());
    if (!view) return diagnostic("toggle side panel: no Workspace in front");
    setWorkspaceView({ panel: view.panel === "closed" ? "open" : "closed" });
  };

  /**
   * The folder chain to an opened file under the focused checkout, so the
   * Explorer shows and highlights the row without the core's `reveal_path`,
   * which would open a pinned tab (B3).
   */
  const revealAncestors = (path: string) => {
    // The Explorer tool is the Workspace's to show or hide (B10); opening a
    // file only unfolds its folders, so the row is highlighted whenever the
    // tool is open.
    const state = rest()?.ui_state;
    const context = explorerContext(rest());
    const root = context.checkout?.path;
    if (!state || !root || !path.startsWith(`${root}/`)) return;
    const parts = path.slice(root.length + 1).split("/");
    parts.pop();
    const expanded = new Set(context.expanded);
    for (let depth = 1; depth <= parts.length; depth += 1) {
      const ancestor = `${root}/${parts.slice(0, depth).join("/")}`;
      expanded.add(ancestor);
    }
    updateUiState(expandedPatch(context.device, [...expanded]));
  };

  /** The ui_state field that holds one device's expanded folders. */
  const expandedPatch = (device: string, paths: string[]) =>
    device === "local"
      ? { expanded_paths: paths }
      : { device_expanded_paths: { ...(rest()?.ui_state?.device_expanded_paths ?? {}), [device]: paths } };

  /** The checkout the Explorer and the palette act on, on the selected device. */
  const explorerHere = () => {
    const context = explorerContext(rest());
    return context.checkout ? { checkout: context.checkout, device: context.device } : null;
  };

  /**
   * `file_open` for a path in the checkout in front, naming its device.
   * `beside` asks for a pinned display in the area next to the active one,
   * or a new area to its right (S7 B4); the core places it.
   */
  const openInFront = (path: string, preview: boolean, what: string, beside = false) => {
    const here = explorerHere();
    if (!here) return diagnostic(`${what}: no focused checkout`);
    revealAncestors(path);
    dispatch({
      schema_version: 2,
      kind: "file_open",
      payload: {
        path,
        workspace_id: here.checkout.workspace_id,
        checkout_id: here.checkout.id,
        preview: beside ? false : preview,
        ...(beside ? { beside: true } : {}),
        ...deviceField(here.device),
      },
    });
  };

  /**
   * The View areas the operator acted on (contract 4.1): the frame the page
   * has drawn, else the front Workspace's as the store holds it, for a chord
   * while the Views are hidden. Every View action names this frame's
   * Workspace and reads its ids from this frame's layout.
   */
  const frameNow = (): ViewFrame | null => {
    const drawn = drawnViews();
    if (drawn) return { workspace: drawn.workspace, layout: drawn.layout };
    const view = workspaceViewOf(rest());
    return view?.layout ? { workspace: { device_id: view.device_id, path: view.path }, layout: view.layout } : null;
  };

  /** The Workspace in front, whose View areas an open lands in. */
  const frontViewWorkspace = (): ViewWorkspace | null => {
    const view = workspaceViewOf(rest());
    return view ? { device_id: view.device_id, path: view.path } : null;
  };

  /** The frame an action acts on, or null with a diagnostic naming the action. */
  const frameFor = (action: string): ViewFrame | null => {
    const frame = frameNow();
    if (!frame) diagnostic(`view_layout ${action}: no View areas in front`);
    return frame;
  };

  /**
   * One `view_layout` action (S7 contract 4.1): one operator action is one
   * event, naming the Workspace of the frame it was taken on, and the core
   * applies it whole or refuses it.
   */
  const viewLayout = (frame: ViewFrame, action: { action: string } & Record<string, unknown>) => {
    dispatch({ schema_version: 2, kind: "view_layout", payload: viewLayoutPayload(frame.workspace, action) });
  };

  /**
   * Moves the keyboard to a display or an area once the core shows it there
   * (S7 B20). A display is asked for from where it stands in this frame, so
   * a move or split of the active view waits for the frame that lands it.
   */
  const followFocus = (frame: ViewFrame, target: { displayId: string } | { areaId: string }) => {
    const workspace = workspaceKey(frame.workspace);
    if ("areaId" in target) return ui().setViewFocusRequest({ workspace, areaId: target.areaId });
    const located = locateDisplay(frame.layout.root, target.displayId);
    ui().setViewFocusRequest({ workspace, displayId: target.displayId, from: located ? { areaId: located.area.id, index: located.index } : null });
  };

  const focusView = (displayId: string) => {
    const frame = frameFor("focus");
    if (frame) viewLayout(frame, { action: "focus", display_id: displayId });
  };

  const focusViewArea = (areaId: string) => {
    const frame = frameFor("focus_area");
    if (!frame) return;
    followFocus(frame, { areaId });
    viewLayout(frame, { action: "focus_area", area_id: areaId });
  };

  /** A display into `areaId` at `index`, its final position there; a reorder when it is already there. */
  const moveView = (displayId: string, areaId: string, index: number) => {
    const frame = frameFor("move");
    if (!frame) return;
    followFocus(frame, { displayId });
    viewLayout(frame, { action: "move", display_id: displayId, area_id: areaId, index });
  };

  /** A new area at `edge` of `areaId`, taking half of it, with the display moved in (S7 B7, B9). */
  const splitView = (displayId: string, areaId: string, edge: Edge) => {
    const frame = frameFor("split");
    if (!frame) return;
    followFocus(frame, { displayId });
    // A repeated request id is a no-op in the core, so a split that arrives
    // twice splits once (S7 B18).
    viewLayout(frame, { action: "split", display_id: displayId, area_id: areaId, edge, request_id: remoteRequestId() });
  };

  /** A split's ratio after a divider drag or a keyboard step; one event per landing (S7 B9). */
  const resizeViewSplit = (splitId: string, ratio: number) => {
    const frame = frameFor("resize");
    if (frame) viewLayout(frame, { action: "resize", split_id: splitId, ratio });
  };

  /** The active View area's active display, which a chord acts on. */
  const activeDisplayNow = () => {
    const frame = frameNow();
    return frame ? activeDisplay(frame.layout) : null;
  };

  /**
   * The contents a save or a close would send for one document, or null: the
   * newest keystroke, else the document a display shows (the `documents`
   * section carries only the documents on screen).
   */
  const draftFor = (tabId: string): string | null =>
    latestDraft(tabId) ?? useShellStore.getState().documents[tabId]?.contents_utf8 ?? null;

  /** Saves the document of the active display, or the named document when one is given. */
  const saveFile = (tabId?: string) => {
    const state = useShellStore.getState();
    const tab = editorTabFor(state.editor, tabId ?? state.editor?.active_tab_id ?? null);
    if (!tab || tab.kind !== "file") {
      diagnostic("file_save: no showing document");
      return false;
    }
    // A read-only or preview-only document has nothing the core would accept:
    // a save of it is refused, and the refusal would mark it dirty.
    const document = state.documents[tab.id] ?? null;
    if (document && (document.readonly_reason !== null || document.contents_utf8 === null)) {
      diagnostic(`file_save: ${tab.path} is not editable here`);
      return false;
    }
    const contents = draftFor(tab.id);
    if (contents === null) {
      diagnostic(`file_save: no contents for ${tab.path}`);
      return false;
    }
    // A device file's path means nothing to this machine's checkout roots,
    // so the save names the device and the daemon hands it to the core,
    // which writes only to the place the tab was opened from.
    const device = deviceOfCheckout(state.rest, tab.checkout_id);
    const sent = dispatch({
      schema_version: 2,
      kind: "file_save",
      payload: {
        tab_id: tab.id,
        path: tab.path,
        // The core checks the save against the revision it read when it
        // opened the file; the shell sends only the draft.
        contents_utf8: contents,
        ...deviceField(device),
      },
    });
    if (sent === false) return false;
    noteSent(tab.id, contents);
    return true;
  };

  /**
   * `changes_select` for a History row of the checkout in front (S4, S7
   * contract 4.2): the core places a diff by the rules a file open follows,
   * so a closed side panel opens to show it (issue 170).
   */
  const selectChangeIn = (path: string, committed: boolean, preview: boolean, beside: boolean) => {
    const here = explorerContext(rest()).checkout;
    const changes = useShellStore.getState().changes;
    const scope = rest()?.navigator?.changes_root_path;
    if (!here || !scope || changes?.root_path !== scope || (scope !== here.path && !scope.startsWith(`${here.path}/`))) {
      return diagnostic("changes_select: checkout is unavailable");
    }
    const group = committed ? changes.committed : changes.entries;
    if (!group.some((entry) => entry.path === path)) return diagnostic("changes_select: row is no longer available");
    dispatch({ schema_version: 2, kind: "changes_select", payload: { path, committed, preview, ...(beside ? { beside: true } : {}) } });
  };

  /** What one frame says about a watched close (`closeWithSaveOutcome`). */
  const closeWatchFrame = (state: ReturnType<typeof useShellStore.getState>, tabId: string): CloseWatchFrame => {
    const document = state.documents[tabId];
    const save = document?.save?.state;
    const view = workspaceViewOf(state.rest);
    const layout = view?.layout;
    return {
      connection: state.connection,
      hostId: state.daemon?.host_id,
      tabIds: (state.editor?.tabs ?? []).map((row) => row.id),
      deviceIds: (state.rest?.navigator?.devices ?? []).map((row) => row.id),
      workspace: view ? workspaceKey(view) : null,
      displayIds: layout ? areasOf(layout.root).flatMap((area) => area.displays.map((display) => display.id)) : [],
      error: state.rest?.status?.last_error ?? null,
      draft: latestDraft(tabId),
      save: !document ? "hidden" : save === "saving" || save === "waiting" || save === "checking" ? "in_flight" : "settled",
    };
  };

  /**
   * Settles the stored draft of a document whose last display this frame
   * closes (D-14). A close that carries a save is watched until it lands,
   * or until nothing more can come of it, and meanwhile the document is not
   * autosaved or saved on leaving, since the close carries that save. A
   * close with nothing to carry deletes a stored draft only when it matches
   * what the core holds, once this tab's queued write has landed.
   */
  const watchDocumentClose = (
    tab: EditorTabSnapshot,
    target: { workspace: ViewWorkspace; displayId: string },
    pending: { tab_id: string; path: string; contents_utf8: string } | null,
    document: EditorDocumentSnapshot | null,
  ) => {
    const state = useShellStore.getState();
    const draftKey = tabBufferKey(state.daemon?.host_id, state.rest, tab);
    if (!pending) {
      if (!draftKey) return;
      const known = document ? { contents_utf8: document.contents_utf8, dirty: document.dirty } : null;
      void settledBuffer(draftKey).then((stored) => {
        const decision = storedDraftOnClose(stored, known);
        if (decision === "delete") void deleteBuffer(draftKey);
        else if (decision === "keep") diagnostic(`view_layout close: the stored draft of ${tab.path} is kept as a recovery item`);
      });
      return;
    }
    // The draft goes when the close lands, not when it is asked for: a
    // close the core refuses, one that turns out not to be the last view, or
    // a tab that leaves for another reason keeps the recovery copy.
    let watch: CloseWatch = {
      tabId: tab.id,
      workspace: workspaceKey(target.workspace),
      displayId: target.displayId,
      hostId: state.daemon?.host_id,
      device: deviceOfCheckout(state.rest, tab.checkout_id),
      errorAt: state.rest?.status?.last_error?.occurred_at ?? null,
      contents: pending.contents_utf8,
      sawSave: false,
    };
    noteClosing(tab.id, true);
    const unsubscribe = useShellStore.subscribe((next, previous) => {
      // Only a frame that can change the answer is read; terminal output is not one.
      if (
        next.editor === previous.editor &&
        next.documents === previous.documents &&
        next.rest === previous.rest &&
        next.connection === previous.connection &&
        next.daemon === previous.daemon
      ) {
        return;
      }
      const { outcome, sawSave } = closeWithSaveOutcome(watch, closeWatchFrame(next, tab.id));
      watch = { ...watch, sawSave };
      if (outcome === "wait") return;
      unsubscribe();
      noteClosing(tab.id, false);
      if (outcome === "landed" && draftKey) void deleteBuffer(draftKey);
      else if (outcome === "keep") diagnostic(`view_layout close: ${tab.path} did not close; its draft is kept`);
    });
  };

  /**
   * Closes one display (S7 B5, contract 4.1). The document's unsaved text
   * rides every close as `pending_save`, since the core and not this frame
   * knows whether the display is the document's last; on any other display
   * the core drops it and the document keeps its draft and dirty state. When
   * this frame shows the document's last display, the close is watched, and
   * unsaved work this page cannot reproduce is not sent at all: a notice
   * offers Don't save, the only close that drops unsaved text.
   */
  const closeView = (displayId: string) => {
    const frame = frameFor("close");
    if (!frame) return;
    const located = locateDisplay(frame.layout.root, displayId);
    if (!located) return diagnostic(`view_layout close: ${displayId} is not open`);
    const state = useShellStore.getState();
    const tab = editorTabFor(state.editor, located.display.tab_id);
    if (!tab || tab.kind !== "file") return viewLayout(frame, { action: "close", display_id: displayId });
    const last = displaysOfDocument(frame.layout.root, tab.id).length === 1;
    const document = state.documents[tab.id] ?? null;
    const carries = documentCloseCarries({ draft: latestDraft(tab.id), document, dirty: tab.dirty });
    if (carries.kind === "unloaded" && last) {
      ui().setNotice({
        text: `${tab.label} has unsaved changes this page has not loaded. Show its view to save them, or close it without saving.`,
        refreshable: false,
        dontSave: { workspace: frame.workspace, displayId },
      });
      return;
    }
    const pending = carries.kind === "save" ? { tab_id: tab.id, path: tab.path, contents_utf8: carries.contents } : null;
    if (last) watchDocumentClose(tab, { workspace: frame.workspace, displayId }, pending, document);
    viewLayout(frame, { action: "close", display_id: displayId, ...(pending ? { pending_save: pending } : {}) });
  };

  /**
   * The operator's explicit Don't save (contract 4.1): the view closes, and
   * with it its document without the unsaved text, on the Workspace the
   * close was asked on. A copy stored in this browser stays a recovery item.
   */
  const closeViewWithoutSaving = (target: { workspace: ViewWorkspace; displayId: string }) => {
    ui().setNotice(null);
    dispatch({ schema_version: 2, kind: "view_layout", payload: viewLayoutPayload(target.workspace, { action: "close", display_id: target.displayId, discard: true }) });
  };

  /** A preview display becomes an ordinary one (B2): the named one, or the active one. */
  const keepViewOpen = (displayId?: string) => {
    const frame = frameNow();
    if (!frame) return;
    const located = displayId ? locateDisplay(frame.layout.root, displayId) : activeDisplay(frame.layout);
    if (!located?.display.preview) return;
    viewLayout(frame, { action: "keep_open", display_id: located.display.id });
  };

  /** Shows the Explorer with a file's row unfolded and selected; nothing is opened. */
  const revealInExplorer = (path: string) => {
    ui().openTools();
    setWorkspaceView({ explorer: true, reveal: path });
    ui().setExplorerSelection(path);
  };

  /** One command of a display's tab menu, its area's overflow menu or the palette (B11, B20). */
  const runViewMenu = (id: ViewMenuId, displayId: string) => {
    const frame = frameFor(id);
    const located = frame ? locateDisplay(frame.layout.root, displayId) : null;
    if (!frame || !located) return diagnostic(`view menu ${id}: ${displayId} is not open`);
    const edge = menuEdge(id);
    if (id === "keep_open") return keepViewOpen(displayId);
    if (id === "copy_path") {
      const text = located.display.kind === "browser" ? (located.display.url ?? "") : located.display.path;
      return void navigator.clipboard?.writeText(text).catch(() => undefined);
    }
    if (id === "reveal") return revealInExplorer(located.display.path);
    if (id === "close_view") return closeView(displayId);
    if (!edge) return;
    if (id.startsWith("split_")) return splitView(displayId, located.area.id, edge);
    const target = findArea(frame.layout.root, neighbourArea(frame.layout.root, located.area.id, edge) ?? "");
    if (!target) return diagnostic(`view menu ${id}: no view area lies ${edge} of ${located.area.id}`);
    moveView(displayId, target.id, target.displays.length);
  };

  /**
   * A View command from the palette (S7 B20): a menu command on the active
   * view, or a focus or resize of the active area. One that cannot run now
   * is not sent; the palette shows its reason.
   */
  const runViewCommand = (id: ViewCommandId) => {
    const frame = frameFor(id);
    if (!frame) return;
    const drawn = drawnViews();
    const command = viewCommands(frame.layout, drawn).find((row) => row.id === id);
    if (!command || command.unavailable) return diagnostic(`view command ${id}: ${command?.unavailable ?? "unknown"}`);
    if (isMenuCommand(id)) {
      const active = activeDisplay(frame.layout);
      if (active) runViewMenu(id, active.display.id);
      return;
    }
    switch (id) {
      case "focus_next":
      case "focus_previous": {
        const next = adjacentInOrder(frame.layout.root, frame.layout.active_area, id === "focus_next" ? 1 : -1);
        if (next) focusViewArea(next.id);
        return;
      }
      case "grow":
      case "shrink": {
        const target = resizeTarget(frame.layout, drawn?.geometry ?? null, id === "grow");
        if ("ratio" in target) resizeViewSplit(target.splitId, target.ratio);
        return;
      }
    }
  };

  return {
    dispatch,
    revealAncestors,
    taskIdNow,

    openSettings(tab: SettingsTab = "general") {
      ui().openSettings(tab);
    },

    setAccent(hex: string) {
      updateUiState({ accent_hex: hex });
    },

    /** One typed event; the core owns the choice and stores it (D-15). */
    setTheme(theme: ThemeChoice) {
      dispatch({ schema_version: 2, kind: "theme_set", payload: { theme } });
    },

    setFontSize(size: number) {
      updateUiState({ font_size: size });
    },

    /** `host`'s pane chords, replaced as a whole; the other host's set is untouched. */
    setPaneShortcuts(host: HostKind, bindings: Record<string, string>) {
      updateUiState(host === "electron" ? { shortcut_bindings: bindings } : { browser_shortcut_bindings: bindings });
    },

    /** Whether this page is looking at the Agents tab; the daemon owns the core's flag. */
    observeAgents(observing: boolean) {
      dispatch({ schema_version: 2, kind: "ai_settings", payload: { observing } });
    },

    /**
     * Whether the window is on screen and whether the Weekly Usage popover is
     * open, the two hints the core's usage timer reads. They ride the UI-state
     * event, which replaces the whole state, so they go with its echo.
     */
    observeUsage(hints: UiStateUsageHints) {
      updateUiState(hints);
    },

    chooseAi(provider: string, model?: string) {
      dispatch({ schema_version: 2, kind: "ai_settings", payload: model === undefined ? { provider } : { provider, model } });
    },

    installHook(runtimeId: string) {
      dispatch({ schema_version: 2, kind: "install_agent_hooks", payload: { runtime_id: runtimeId } });
    },

    registerDevice(id: string, label: string, alias: string, options: { hostConsent: boolean; herdrSocketPath: string | null }) {
      dispatch({
        schema_version: 2,
        kind: "register_device",
        payload: { id, label, ssh_alias: alias, host_consent: options.hostConsent, herdr_socket_path: options.herdrSocketPath },
      });
    },

    /** Gives or withdraws the one consent for Hide's helper on a device (PRD S5.5 B50-B52). */
    setDeviceHostConsent(deviceId: string, allow: boolean) {
      dispatch({ schema_version: 2, kind: "device_host_consent", payload: { device_id: deviceId, allow } });
    },

    retryDeviceHost(deviceId: string) {
      dispatch({ schema_version: 2, kind: "device_host_retry", payload: { device_id: deviceId } });
    },

    testDevice(deviceId: string) {
      dispatch({ schema_version: 2, kind: "test_device", payload: { device_id: deviceId } });
    },

    retryDevice(deviceId: string) {
      dispatch({ schema_version: 2, kind: "retry_connect", payload: { target_id: deviceId } });
    },

    /**
     * The core closes the device's file tabs without saving, so every draft of
     * them this browser is still writing lands first and stays as a recovery
     * item (B26). A draft that could not be stored then (B44) would be lost
     * with its tab, so nothing is sent and its path is returned instead.
     */
    async removeDevice(deviceId: string): Promise<string[]> {
      const state = useShellStore.getState();
      const keys = (state.editor?.tabs ?? [])
        .filter((tab) => tab.checkout_id.startsWith(`remote:${deviceId}:`))
        .map((tab) => tabBufferKey(state.daemon?.host_id, state.rest, tab))
        .filter((key): key is BufferKey => key !== null);
      await Promise.all(keys.map((key) => flushBuffer(key)));
      const flushed = useShellStore.getState();
      const unstored = unstoredDeviceDrafts(deviceId, flushed.editor?.tabs ?? [], flushed.bufferWarnings, draftExported(flushed.exportedDrafts, latestDraft));
      if (unstored.length > 0) {
        diagnostic(`remove_device: ${deviceId} kept because ${unstored.length} draft(s) could not be stored`);
        return unstored;
      }
      dispatch({ schema_version: 2, kind: "remove_device", payload: { device_id: deviceId } });
      return [];
    },

    focusDevice(deviceId: string) {
      dispatch({ schema_version: 2, kind: "focus_device", payload: { device_id: deviceId } });
    },

    /**
     * A Workspace chosen on Main or an Overview, on any device (S6 B2, B21).
     * A Workspace on another device than the one in front is one event that
     * also brings its device forward, so a refusal moves neither. The screen
     * follows once the core has moved there (`opening`).
     */
    openWorkspace(deviceId: string, workspaceId: string, checkoutId: string) {
      const path =
        (deviceId === "local" ? rest()?.navigator?.workspaces : rest()?.status?.remote?.find((row) => row.target_id === deviceId)?.session?.workspaces)
          ?.flatMap((row) => row.checkouts)
          .find((row) => row.id === checkoutId)?.path ?? null;
      beginOpening({ checkoutId, deviceId, path });
      const front = rest()?.navigator?.focused_device_id ?? "local";
      if (front === deviceId) return focusCheckout(workspaceId, checkoutId);
      if (deviceId === "local") {
        dispatch({ schema_version: 2, kind: "focus_checkout", payload: { workspace_id: workspaceId, checkout_id: checkoutId, focus_device: true } });
        return;
      }
      // A registered device project Herdr has no workspace in yet is opened
      // by creating one at its folder there, as its sidebar row does.
      if (checkoutId.endsWith(REGISTERED_CHECKOUT)) {
        const checkout = rest()?.status?.remote?.find((row) => row.target_id === deviceId)?.session?.workspaces.flatMap((row) => row.checkouts).find((row) => row.id === checkoutId);
        if (!checkout) return diagnostic(`open workspace: ${checkoutId} is not on ${deviceId}`);
        return dispatch(withDeviceForward(remoteControl(deviceId, { action: "create_tab", workspace_id: workspaceId, checkout_id: checkoutId, cwd: checkout.path, label: checkout.next_tab_label })));
      }
      dispatch(withDeviceForward(remoteControl(deviceId, { action: "focus_workspace", workspace_id: workspaceId, checkout_id: checkoutId })));
    },

    followRelation,

    /** Asks for the failed relationship focus again, as a new request. */
    retryRelation() {
      const relation = ui().relation;
      if (relation) followRelation(relation.sourcePaneId, relation.targetPaneId, relation.label);
    },

    dismissRelation() {
      ui().setRelation(null);
    },

    dismissOpening() {
      ui().setOpening(null);
    },

    /**
     * Names a Project for its Sessions screen and reads its history, or reads
     * it again, with the open session after it: the screen's every Retry
     * (PRD S8 D-03, B4, B5). The Project is named whatever is focused.
     */
    refreshProjectSessions(workspaceId: string, deviceId: string) {
      dispatch({ schema_version: 2, kind: "sessions_refresh", payload: { workspace_id: workspaceId, ...deviceField(deviceId) } });
    },

    /** Opens one session read-only beside the named Project's history; never in a Workspace (B3). */
    openProjectSession(workspaceId: string, sessionId: string) {
      dispatch({ schema_version: 2, kind: "archive_open", payload: { kind: "session", id: sessionId, workspace_id: workspaceId } });
    },

    /** An agent chosen on an Overview or in the Agents list: its Workspace and pane (B12). */
    openAgent(paneId: string) {
      beginOpening({ paneId });
      const target = remoteTargetOfPane(rest(), paneId) ?? "local";
      const forward = (rest()?.navigator?.focused_device_id ?? "local") !== target;
      if (target !== "local") {
        const event = remoteControl(target, { action: "focus_pane", pane_id: paneId });
        return dispatch(forward ? withDeviceForward(event) : event);
      }
      dispatch({ schema_version: 2, kind: "focus_pane", payload: { pane_id: paneId, origin: "operator", focus_device: forward } });
    },

    setPinned(workspaceId: string, pinned: boolean) {
      dispatch({ schema_version: 2, kind: "workspace_pin_set", payload: { workspace_id: workspaceId, pinned } });
    },

    createWorktree(request: { deviceId: string; repositoryRoot: string; branch: string; baseBranch: string | null; agentKind: string | null; purpose: string | null }) {
      dispatch({
        schema_version: 2,
        kind: "create_worktree",
        payload: {
          device_id: request.deviceId,
          repository_root: request.repositoryRoot,
          branch: request.branch,
          base_branch: request.baseBranch,
          agent_kind: request.agentKind,
          purpose: request.purpose,
        },
      });
    },

    setPurpose(checkoutId: string, text: string) {
      dispatch({ schema_version: 2, kind: "set_checkout_purpose", payload: { checkout_id: checkoutId, text } });
    },

    removeWorktree(deviceId: string, checkoutPath: string, deleteBranch: boolean) {
      dispatch({ schema_version: 2, kind: "remove_worktree", payload: { device_id: deviceId, checkout_path: checkoutPath, delete_branch: deleteBranch } });
    },

    retryTaskAgent(id: number) {
      dispatch({ schema_version: 2, kind: "task_agent_retry", payload: { id } });
    },

    focusPane,

    createTab() {
      if (remoteContext(rest())) {
        const host = remoteHost("New tab");
        if (!host) return;
        const checkout = host.view?.checkout;
        if (!checkout) return diagnostic("create_tab: the remote device has no workspace open");
        sendRemote(host.targetId, { action: "create_tab", workspace_id: checkout.workspace_id, checkout_id: checkout.id, cwd: checkout.path, label: checkout.next_tab_label });
        return;
      }
      const here = current();
      if (!here) return diagnostic("create_tab: no focused checkout");
      dispatch({
        schema_version: 2,
        kind: "create_tab",
        payload: { workspace_id: here.checkout.workspace_id, checkout_id: here.checkout.id, label: here.checkout.next_tab_label },
      });
    },

    /** `inPlace`: chosen in the Agent area's own tab strip, beside the side panel, which stays up (issue 170). */
    focusTab(tabId: string, inPlace = false) {
      if (remoteContext(rest())) {
        const host = remoteHost("Switching tab");
        if (!host) return;
        const event = remoteControl(host.targetId, { action: "focus_tab", tab_id: tabId });
        dispatch(inPlace ? inPlaceEvent(event) : event);
        return;
      }
      const here = current();
      if (!here) return;
      dispatch({
        schema_version: 2,
        kind: "focus_tab",
        payload: { workspace_id: here.checkout.workspace_id, checkout_id: here.checkout.id, tab_id: tabId, ...(inPlace ? { in_place: true } : {}) },
      });
    },

    reorderTab(stripId: string, toIndex: number) {
      // A device's strip is arranged by the core as this machine's is; a
      // Herdr tab moves on the device's own Herdr (`reorder_tab`).
      // A file slot moves with the device offline too; the core refuses a
      // Herdr move while the device is not connected.
      const context = remoteContext(rest());
      if (context) {
        const checkout = remoteView(context.session)?.checkout;
        if (!checkout) return diagnostic("reorder_tab: no device checkout in front");
        dispatch({
          schema_version: 2,
          kind: "reorder_tab",
          payload: { workspace_id: checkout.workspace_id, checkout_id: checkout.id, tab_id: stripId, to_index: toIndex },
        });
        return;
      }
      const here = current();
      if (!here) return;
      dispatch({
        schema_version: 2,
        kind: "reorder_tab",
        payload: { workspace_id: here.checkout.workspace_id, checkout_id: here.checkout.id, tab_id: stripId, to_index: toIndex },
      });
    },

    closeTab(tabId?: string) {
      // The close chord closes the active display when the View area holds
      // the keyboard or is all that shows, on this machine or a device, and
      // the Agent tab otherwise.
      const display = tabId ? null : activeDisplayNow();
      if (display && viewAreaInUse(rest())) return closeView(display.display.id);
      if (remoteContext(rest())) {
        const host = remoteHost("Close tab");
        if (!host) return;
        const tab = host.view?.checkout.tabs.find((row) => row.id === (tabId ?? host.view?.tab?.id));
        if (!tab?.id) return diagnostic("close_tab: no visible remote tab");
        requestClose("tab", tab.id, tab.panes, host.targetId, host.agents);
        return;
      }
      const here = current();
      const id = tabId ?? here?.tab?.id;
      if (!here || !id) return diagnostic("close_tab: no visible tab");
      const tab = here.checkout.tabs.find((row) => row.id === id);
      requestClose("tab", id, tab?.panes ?? [], null, useShellStore.getState().agents);
    },

    closePane(paneId?: string) {
      const id = paneId ?? useShellStore.getState().focusedPaneId;
      if (remoteContext(rest())) {
        const host = remoteHost("Close pane");
        if (!host) return;
        const pane = host.view?.tab?.panes.find((row) => row.id === id);
        if (!id || !pane) return diagnostic("close_pane: no focused remote pane");
        requestClose("pane", id, [pane], host.targetId, host.agents);
        return;
      }
      const here = current();
      if (!here?.tab || !id) return diagnostic("close_pane: no focused pane");
      const pane = here.tab.panes.find((row) => row.id === id);
      requestClose("pane", id, pane ? [pane] : [], null, useShellStore.getState().agents);
    },

    /** The operator chose "Stop work and close" on the confirmation. */
    confirmClose() {
      const pending = ui().pendingClose;
      if (!pending) return;
      ui().setPendingClose(null);
      // The confirmation names the host it was asked about; switching devices
      // while it was open does not move the close to another one.
      sendClose(pending.kind, pending.id, pending.targetId, true);
    },

    keepOpen() {
      ui().setPendingClose(null);
    },

    refreshStatus() {
      ui().setNotice(null);
      dispatch({ schema_version: 2, kind: "refresh_status", payload: {} });
    },

    /** A close whose outcome the core could not read; asks it to check (`check_close_status`). */
    checkCloseStatus(key: string) {
      dispatch({ schema_version: 2, kind: "check_close_status", payload: { key } });
    },

    reopenClosed() {
      // The core reopens the newest close of the device in front, and
      // `recent_closed` already speaks for that device.
      const recent = rest()?.recent_closed;
      if (!recent?.can_reopen) {
        diagnostic(`reopen_closed: nothing to reopen${recent?.reopen_blocked_reason ? ` (${recent.reopen_blocked_reason})` : ""}`);
        return;
      }
      dispatch({ schema_version: 2, kind: "reopen_closed", payload: {} });
    },

    split(direction: "right" | "down") {
      if (remoteContext(rest())) {
        const host = remoteHost("Split");
        if (!host) return;
        const pane = host.view?.tab?.panes.find((row) => row.id === host.view?.focusedPaneId);
        if (!pane) return diagnostic("split_pane: no focused remote pane");
        sendRemote(host.targetId, { action: "split_pane", pane_id: pane.id, direction, cwd: pane.cwd || null });
        return;
      }
      const here = current();
      const paneId = useShellStore.getState().focusedPaneId;
      const pane = here?.tab?.panes.find((row) => row.id === paneId);
      if (!here?.tab?.id || !pane) return diagnostic("create_pane: no focused pane");
      dispatch({
        schema_version: 2,
        kind: "create_pane",
        payload: { tab_id: here.tab.id, cwd: pane.cwd, command: null, direction },
      });
    },

    toggleZoom() {
      if (remoteContext(rest())) {
        const host = remoteHost("Zoom pane");
        if (!host) return;
        const paneId = host.view?.focusedPaneId;
        if (!paneId) return diagnostic("toggle_pane_zoom: no focused remote pane");
        sendRemote(host.targetId, { action: "toggle_pane_zoom", pane_id: paneId });
        return;
      }
      const paneId = useShellStore.getState().focusedPaneId;
      if (!paneId) return diagnostic("toggle_zoom: no focused pane");
      dispatch({ schema_version: 2, kind: "toggle_zoom", payload: { pane_id: paneId } });
    },

    /** ⌘= / ⌘- / ⌘0 scale whichever surface is showing: the document when an
     * editor tab owns the canvas, else the focused terminal pane. */
    textScale(direction: "in" | "out" | "reset") {
      // A pane's text size is this page's drawing, stored in the core's ui
      // state by pane id; a remote pane is sized the same way and nothing is
      // sent to its host.
      if (editorFor(useShellStore.getState().editor) && viewAreaInUse(rest())) {
        dispatch({ schema_version: 2, kind: "editor_text_scale", payload: { direction } });
        return;
      }
      const paneId = useShellStore.getState().focusedPaneId;
      if (!paneId) return diagnostic("pane_text_scale: no focused pane");
      dispatch({ schema_version: 2, kind: "pane_text_scale", payload: { pane_id: paneId, direction } });
    },

    focusCheckout,

    toggleInactiveCheckouts(projectPath: string) {
      dispatch({ schema_version: 2, kind: "inactive_checkouts_toggle", payload: { project_path: projectPath } });
    },

    /** Folds or unfolds an agent's descendants in the Agents list; the core keeps the choice. */
    toggleAgentTree(paneId: string) {
      dispatch({ schema_version: 2, kind: "agent_tree_toggle", payload: { pane_id: paneId } });
    },

    toggleInactiveProjects(deviceId: string) {
      dispatch({ schema_version: 2, kind: "inactive_projects_toggle", payload: { device_id: deviceId } });
    },

    /** Folds or unfolds a project's checkouts in the Projects list; the core keeps the choice and says it back as `expanded`. */
    toggleProjectCheckouts(workspace: Workspace) {
      const collapsed = new Set(rest()?.ui_state?.collapsed_workspace_ids ?? []);
      if (workspace.expanded === false) collapsed.delete(workspace.id);
      else collapsed.add(workspace.id);
      updateUiState({ collapsed_workspace_ids: [...collapsed].sort() });
    },

    /** Opens or closes the agent rows under a checkout in the Projects list; they start closed and the core keeps the choice. */
    toggleCheckoutAgents(checkoutId: string) {
      const expanded = new Set(rest()?.ui_state?.expanded_checkout_ids ?? []);
      if (!expanded.delete(checkoutId)) expanded.add(checkoutId);
      updateUiState({ expanded_checkout_ids: [...expanded].sort() });
    },

    toggleLeftSidebar() {
      const state = rest()?.ui_state;
      if (state) setLeftSidebarVisible(!state.left_sidebar_visible);
    },

    toggleSidebarView() {
      const order: SidebarMode[] = [...SIDEBAR_MODES];
      const index = order.indexOf(ui().sidebarMode);
      showSidebarMode(order[(index + 1) % order.length] ?? "agents");
    },

    showSidebarMode,

    showExplorerPanel,
    toggleRightPanel,

    setWorkspaceView,

    /** The side panel closed, open or expanded; none of them closes a view (issue 170). */
    setPanel(panel: PanelState) {
      setWorkspaceView({ panel });
    },

    /** Docks the side panel beside the agents, or floats it over them again: one terminal resize either way. */
    setPanelPinned(pinned: boolean) {
      setWorkspaceView({ pinned });
    },

    revealInExplorer,

    /**
     * Explorer and Changes open and close independently (B10). Showing a tool
     * a narrow panel's overlay kept out of sight opens the overlay without an
     * event when the open panel already stores it shown (S7 B12, B13).
     */
    setTool(tool: "explorer" | "changes", visible: boolean) {
      if (visible) return showTool(tool);
      setWorkspaceView(tool === "explorer" ? { explorer: false } : { changes: false });
    },

    /** A History row's diff in the active View area's preview (a single click) or pinned (a double click). */
    selectChange(path: string, committed: boolean, preview: boolean) {
      selectChangeIn(path, committed, preview, false);
    },

    /** "Open to the side" on a History row (S7 B4): its diff pinned beside the active View area. */
    openChangeBeside(path: string, committed: boolean) {
      selectChangeIn(path, committed, false, true);
    },

    openShortcuts() {
      ui().openOverlay(ui().overlay === "shortcuts" ? "none" : "shortcuts");
    },

    /** The focused pane's find bar; the core searches a device pane through that device's Herdr. */
    openFind() {
      if (ui().overlay === "find") {
        document.querySelector<HTMLInputElement>("[data-find-bar] input")?.focus();
        return;
      }
      ui().openOverlay("find");
    },

    /** The registration field lives in the sidebar, so a hidden sidebar is shown first. */
    openNewWorkspace() {
      setLeftSidebarVisible(true);
      ui().openOverlay("new_workspace");
    },

    /**
     * ⌘⇧H: the Overview of the Project the front checkout belongs to, on
     * whichever device (web-project-overview B1). With nothing in front there
     * is no Project to name, so nothing moves and the diagnostic says why.
     */
    openProjectOverview() {
      const front = frontCheckout(rest());
      const project = front ? catalogWorkspaces(rest()).find((row) => row.checkouts.some((checkout) => checkout.id === front.id)) : null;
      if (!project) return diagnostic("project overview: no checkout is in front");
      ui().setScreen({ kind: "overview", projectId: project.id });
    },

    /** Back from an Overview to the pane grid in front, or to Main when no Workspace is in front (B1). */
    leaveProjectOverview() {
      ui().setScreen(frontCheckout(rest()) && rest()?.workspace_view ? { kind: "workspace" } : { kind: "main" });
    },

    /** ⌘P: the file palette over hided's index of the focused checkout. */
    openFilePalette() {
      ui().openOverlay(ui().overlay === "file_palette" ? "none" : "file_palette");
    },

    /** ⌘K: the palette that searches the snapshot's agents, projects and checkouts. */
    openSearch() {
      ui().openOverlay(ui().overlay === "search" ? "none" : "search");
    },

    /** Asks for the index of `root` on `device`; a device's is walked by its helper. */
    requestFileIndex(root: string, query: string, device: string) {
      dispatch({ schema_version: 2, kind: "file_index", payload: { root, query, ...deviceField(device) } });
    },

    /** A palette pick opens in the checkout's preview tab (B12) and closes the palette. */
    openIndexEntry(path: string) {
      ui().closeOverlay();
      // The core's selected_path may already be this file from an earlier
      // open, so the row is highlighted from the pick itself (B3).
      ui().setExplorerSelection(path);
      openInFront(path, true, "file_open");
    },

    /** A pick from "Open file to the side": a pinned display beside the active area (S7 B4). */
    openIndexEntryBeside(path: string) {
      ui().closeOverlay();
      ui().setExplorerSelection(path);
      openInFront(path, false, "file_open", true);
    },

    /** "Open file to the side" from the palette: ⌘P's list, whose pick opens beside. */
    openFilePaletteBeside() {
      ui().openOverlay("file_palette_beside");
    },

    /** Registers a folder on `deviceId`; a device's own helper judges it against that device's home. */
    createWorkspace(path: string, label: string, deviceId = "local") {
      dispatch({ schema_version: 2, kind: "create_workspace", payload: { ...deviceField(deviceId), path, label, initialize_git: false } });
    },

    /** Removes a registration after its panes close (D-10); the folder is never touched. */
    removeWorkspace(workspaceId: string) {
      dispatch({ schema_version: 2, kind: "remove_workspace", payload: { workspace_id: workspaceId } });
    },

    listDirectory(path: string) {
      dispatch({ schema_version: 2, kind: "remote_file_list", payload: { target_id: "local", root_path: path } });
    },

    /** One checkout folder's children, answered by hided as a `directory_list`. */
    listChildren(root: string, path: string) {
      const device = explorerContext(rest()).device;
      dispatch({ schema_version: 2, kind: "file_list", payload: { root, path, ...deviceField(device) } });
    },

    /** The core owns which folders the tree has expanded, per device; this replaces the selected device's set. */
    setExpandedPaths(paths: string[]) {
      updateUiState(expandedPatch(explorerContext(rest()).device, paths));
    },

    /** A single click opens the active area's preview slot; a double click pins it. */
    openFile(path: string, preview: boolean) {
      openInFront(path, preview, "file_open");
    },

    /** "Open to the side" (S7 B4): a second, pinned display in the next area. */
    openFileBeside(path: string) {
      openInFront(path, false, "file_open", true);
    },

    /**
     * A browser display of `url` in a Workspace (issue 155): the front one,
     * or the one a page that asked for a new window belongs to. An address
     * the Workspace already shows is focused and loaded again.
     */
    openBrowser(url: string, workspace?: ViewWorkspace) {
      const target = workspace ?? frontViewWorkspace();
      if (!target) return diagnostic("browser_open: no Workspace in front");
      dispatch({ schema_version: 2, kind: "browser_open", payload: { url, workspace: { device_id: target.device_id, path: target.path } } });
    },

    /** Explorer "Open in Browser" on an HTML file of this machine's checkout. */
    openInBrowser(path: string) {
      const target = frontViewWorkspace();
      if (!target) return diagnostic("browser_open: no Workspace in front");
      dispatch({ schema_version: 2, kind: "browser_open", payload: { url: fileUrl(path), workspace: { device_id: target.device_id, path: target.path } } });
    },

    /** The address field: the display loads what was typed. */
    navigateBrowser(displayId: string, url: string) {
      const frame = frameFor("navigate");
      if (frame) viewLayout(frame, { action: "navigate", display_id: displayId, url });
    },

    /** What a page says (its address and title), recorded so the tab and a relaunch show it. */
    reportBrowserState(workspace: ViewWorkspace, displayId: string, url: string, title: string) {
      dispatch({
        schema_version: 2,
        kind: "browser_state",
        payload: { workspace: { device_id: workspace.device_id, path: workspace.path }, display_id: displayId, url, title },
      });
    },

    /** ⌘⇧K, a double click or Keep open: a preview display becomes an ordinary one. */
    keepViewOpen,

    focusView,
    focusViewArea,
    moveView,
    splitView,
    resizeViewSplit,
    closeView,
    closeViewWithoutSaving,
    runViewMenu,
    runViewCommand,

    /** Re-reads an unavailable display's file (S7 B16). */
    retryView(displayId: string) {
      const frame = frameFor("retry");
      if (frame) viewLayout(frame, { action: "retry", display_id: displayId });
    },

    /** A new file or folder in `parent`; the core opens a created file (B9). */
    createEntry(parent: string, name: string, isDirectory: boolean) {
      const here = explorerTarget();
      if (!here) return diagnostic("explorer create: no focused checkout");
      dispatch({
        schema_version: 2,
        kind: isDirectory ? "dir_create" : "file_create",
        payload: { ...here, parent, name },
      });
    },

    renameEntry(path: string, name: string) {
      const here = explorerTarget();
      if (!here) return diagnostic("path_rename: no focused checkout");
      dispatch({ schema_version: 2, kind: "path_rename", payload: { ...here, path, name } });
    },

    /** A drag that landed: one `path_move` into the folder it was dropped on. */
    moveEntry(path: string, destination: string) {
      const here = explorerTarget();
      if (!here) return diagnostic("path_move: no focused checkout");
      dispatch({ schema_version: 2, kind: "path_move", payload: { ...here, path, destination } });
    },

    /** Opens the trash confirmation; nothing is dispatched until it is confirmed. */
    requestTrash(path: string, name: string, isDirectory: boolean, selectAfter: string, inode: number | null) {
      const target = explorerTarget();
      if (!target) return diagnostic("path_trash: no focused checkout");
      ui().setPendingTrash({ path, name, isDirectory, selectAfter, inode, target });
    },

    confirmTrash() {
      const pending = ui().pendingTrash;
      ui().setPendingTrash(null);
      if (!pending) return;
      dispatch({
        schema_version: 2,
        kind: "path_trash",
        payload: {
          ...pending.target,
          path: pending.path,
          select_after: pending.selectAfter,
          inode: pending.inode,
        },
      });
    },

    cancelTrash() {
      ui().setPendingTrash(null);
    },

    /** One keystroke's contents for one document; the core keeps the draft and the dirty flag. */
    updateDraft(tabId: string, contents: string) {
      dispatch({ schema_version: 2, kind: "file_draft", payload: { tab_id: tabId, contents_utf8: contents } });
    },

    /**
     * A save of the showing document, or of the named tab: the expected
     * modification time is the one the core read when it opened the file, so a
     * disk change since then makes the save a conflict rather than a silent
     * overwrite (B5).
     */
    saveFile,

    /** A document's Markdown mode and wrap choice; the core persists both. */
    setFileView(tabId: string, live: boolean, wrap: boolean) {
      dispatch({ schema_version: 2, kind: "file_view", payload: { tab_id: tabId, markdown_live: live, wrap } });
    },

    /** "reload" reads the disk contents; "keep_editing" accepts the disk
     * timestamp so the next save overwrites. */
    resolveConflict(tabId: string, action: "reload" | "keep_editing") {
      dispatch({ schema_version: 2, kind: "file_conflict", payload: { tab_id: tabId, action } });
    },

    /** ⌘F for one display, by default the active one: only its editor opens its find panel. */
    requestEditorFind(displayId?: string) {
      ui().requestEditorFind(displayId ?? activeDisplayNow()?.display.id ?? null);
    },

    /** `step` 0 searches and keeps the current match; +1 and -1 move (core `PaneFindPayload`). */
    find(paneId: string, term: string, step: -1 | 0 | 1) {
      dispatch({
        schema_version: 2,
        kind: "pane_find",
        payload: { pane_id: paneId, term, case_sensitive: false, whole_word: false, regex: false, step },
      });
    },
  };
}
