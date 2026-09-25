// Every shell command in one place, so a shortcut, a button and a menu run
// the same code against the same snapshot. Each action is one core event
// (dispatch is fire-and-forget; a sequence would arrive as several frames).

import { closeWithSaveOutcome, deleteBuffer, flushBuffer, settledBuffer, storedDraftOnClose, tabBufferKey, type BufferKey } from "./buffers";
import { closeDecision, statusUnknownNotice } from "./close";
import { draftExported, unstoredDeviceDrafts } from "./settings";
import { latestDraft, noteClosing, noteSent } from "./editor/draft";
import { RELATION_ANSWER_TIMEOUT_MS, relationState } from "./lineage";
import type { OpenTarget } from "./navigation";
import { REGISTERED_CHECKOUT, remoteConnected, remoteContext, remoteControl, remoteRequestId, remoteTargetOfPane, remoteView, withDeviceForward, type RemoteAction, type RemoteView } from "./remote";
import { deviceOfCheckout, editorFor, editorTabFor, explorerContext, focusedCheckout, visibleTab, type AgentRow, type Checkout, type EditorTabSnapshot, type Tab } from "./snapshot";
import { useShellStore } from "./store";
import { SIDEBAR_MODES, useUiStore, type SidebarMode } from "./ui";
import type { DispatchFn } from "./ws";
import { drawnViews, viewAreaInUse } from "./viewFocus";
import { activeDisplay, adjacentInOrder, displaysOfDocument, findArea, locateDisplay, menuEdge, neighbourArea, resizeTarget, viewCommands, type Edge, type ViewCommandId, type ViewMenuId } from "./viewLayout";
import { workspaceViewOf, type ViewMode } from "./workspace";

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
      dispatch({
        schema_version: 2,
        kind: "remote_control",
        payload: { target_id: targetId, request_id: requestId, report_pane_focus_outcome: true, action: "focus_pane", pane_id: targetPaneId },
      });
      return;
    }
    dispatch({ schema_version: 2, kind: "focus_pane", payload: { pane_id: targetPaneId, origin: "operator", request_id: requestId } });
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
   * The front Workspace's layout and tools (S6 D-03, D-05): one
   * `workspace_view` event naming only what changes. The core keeps them per
   * Workspace, so another Workspace is never touched.
   */
  const setWorkspaceView = (patch: { mode?: ViewMode; explorer?: boolean; changes?: boolean; agent_share?: number; reveal?: string }) => {
    if (!workspaceViewOf(rest())) return diagnostic("workspace_view: no Workspace in front");
    dispatch({ schema_version: 2, kind: "workspace_view", payload: patch });
  };

  const showExplorerPanel = () => setWorkspaceView({ explorer: true });

  /** ⌘⇧B: hides the Workspace tools when any shows, else shows the Explorer. */
  const toggleRightPanel = () => {
    const view = workspaceViewOf(rest());
    if (!view) return diagnostic("toggle tools: no Workspace in front");
    if (view.explorer || view.changes) setWorkspaceView({ explorer: false, changes: false });
    else setWorkspaceView({ explorer: true });
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
    // A file asked for is what the operator works on next, so a window too
    // narrow for Agents and Views together shows the Views (S7 B13).
    ui().setWorkingRegion("views");
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

  /** The front Workspace's View areas, when the core publishes them (S7). */
  const layoutNow = () => workspaceViewOf(rest())?.layout ?? null;

  /**
   * One `view_layout` action on the front Workspace (S7 contract 4.1): one
   * operator action is one event, and the core applies it whole or refuses it.
   */
  const viewLayout = (payload: { action: string } & Record<string, unknown>) => {
    if (!layoutNow()) return diagnostic(`view_layout ${payload.action}: no View areas in front`);
    dispatch({ schema_version: 2, kind: "view_layout", payload });
  };

  /** Moves the keyboard to a display or an area once the core shows it active (S7 B20). */
  const followFocus = (request: { displayId: string } | { areaId: string }) => ui().setViewFocusRequest(request);

  const focusView = (displayId: string) => viewLayout({ action: "focus", display_id: displayId });

  const focusViewArea = (areaId: string) => {
    followFocus({ areaId });
    viewLayout({ action: "focus_area", area_id: areaId });
  };

  /** A display into `areaId` at `index`, its final position there; a reorder when it is already there. */
  const moveView = (displayId: string, areaId: string, index: number) => {
    followFocus({ displayId });
    viewLayout({ action: "move", display_id: displayId, area_id: areaId, index });
  };

  /** A new area at `edge` of `areaId`, taking half of it, with the display moved in (S7 B7, B9). */
  const splitView = (displayId: string, areaId: string, edge: Edge) => {
    followFocus({ displayId });
    // A repeated request id is a no-op in the core, so a split that arrives
    // twice splits once (S7 B18).
    viewLayout({ action: "split", display_id: displayId, area_id: areaId, edge, request_id: remoteRequestId() });
  };

  /** A split's ratio after a divider drag or a keyboard step; one event per landing (S7 B9). */
  const resizeViewSplit = (splitId: string, ratio: number) => viewLayout({ action: "resize", split_id: splitId, ratio });

  /** The active View area's active display, which a chord acts on. */
  const activeDisplayNow = () => {
    const layout = layoutNow();
    return layout ? activeDisplay(layout) : null;
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
   * What closing a document carries, and the watch that settles its stored
   * draft; null, with a diagnostic, when the close must not be sent.
   */
  const prepareDocumentClose = (tab: EditorTabSnapshot): { pending: { tab_id: string; path: string; contents_utf8: string } | null } | null => {
    const tabId = tab.id;
    const state = useShellStore.getState();
    // Only unsaved work rides a close: the newest keystroke decides, not the
    // last snapshot's dirty flag, and a clean tab closes in one step rather
    // than through a save the core may refuse (B4, D-10). A dirty tab whose
    // text this shell cannot reproduce stays open with a note instead.
    const document = state.documents[tabId] ?? null;
    // A read-only or preview-only document has no draft the core would accept,
    // so its close is one step however the tab got marked (D-10, D-14).
    const editable = document ? document.readonly_reason === null && document.contents_utf8 !== null : true;
    let contents = editable ? latestDraft(tabId) : null;
    if (contents === null && tab.dirty && editable) {
      if (document) contents = document.contents_utf8 ?? null;
    }
    if (contents === null && tab.dirty && editable) {
      diagnostic(`view_layout close: ${tab.path} has unsaved changes this shell cannot reproduce; open it and save first`);
      return null;
    }
    const pending =
      contents !== null
        ? { tab_id: tab.id, path: tab.path, contents_utf8: contents }
        : null;
    const draftKey = tabBufferKey(state.daemon?.host_id, state.rest, tab);
    if (pending) {
      // The draft goes when the close lands, not when it is asked for: the
      // core closes the tab only after its save landed clean, and a close it
      // refuses, or a tab that leaves for another reason, keeps the recovery
      // copy (D-14).
      const watch = {
        tabId,
        hostId: state.daemon?.host_id,
        device: deviceOfCheckout(state.rest, tab.checkout_id),
        errorAt: state.rest?.status?.last_error?.occurred_at ?? null,
      };
      noteClosing(tabId, true);
      const unsubscribe = useShellStore.subscribe((next) => {
        const outcome = closeWithSaveOutcome(watch, {
          connection: next.connection,
          hostId: next.daemon?.host_id,
          tabIds: (next.editor?.tabs ?? []).map((row) => row.id),
          deviceIds: (next.rest?.navigator?.devices ?? []).map((row) => row.id),
          error: next.rest?.status?.last_error ?? null,
        });
        if (outcome === "wait") return;
        unsubscribe();
        noteClosing(tabId, false);
        if (outcome === "landed" && draftKey) void deleteBuffer(draftKey);
      });
    } else if (draftKey) {
      // Nothing rides this close, but a stored draft may still hold work this
      // page never loaded; it goes only when it matches what the core holds.
      // It is read once this tab's queued write has landed, so an edit
      // typed just before the close is judged rather than left behind.
      const known = document ? { contents_utf8: document.contents_utf8, dirty: document.dirty } : null;
      void settledBuffer(draftKey).then((stored) => {
        const decision = storedDraftOnClose(stored, known);
        if (decision === "delete") void deleteBuffer(draftKey);
        else if (decision === "keep") diagnostic(`view_layout close: the stored draft of ${tab.path} is kept as a recovery item`);
      });
    }
    return { pending };
  };

  /**
   * Closes one display (S7 B5, contract 4.1). Only the document's last
   * display closes the document, so only that close carries its unsaved text
   * as `pending_save` and goes through the save-then-close protection; any
   * other display goes and the document stays with its draft and dirty state.
   */
  const closeView = (displayId: string) => {
    const layout = layoutNow();
    if (!layout) return diagnostic("view_layout close: no View areas in front");
    const located = locateDisplay(layout.root, displayId);
    if (!located) return diagnostic(`view_layout close: ${displayId} is not open`);
    const tab = editorTabFor(useShellStore.getState().editor, located.display.tab_id);
    const last = tab !== null && tab.kind === "file" && displaysOfDocument(layout.root, tab.id).length === 1;
    if (!tab || !last) return viewLayout({ action: "close", display_id: displayId });
    const close = prepareDocumentClose(tab);
    if (!close) return;
    viewLayout({ action: "close", display_id: displayId, ...(close.pending ? { pending_save: close.pending } : {}) });
  };

  /** A preview display becomes an ordinary one (B2): the named one, or the active one. */
  const keepViewOpen = (displayId?: string) => {
    const layout = layoutNow();
    if (!layout) return;
    const located = displayId ? locateDisplay(layout.root, displayId) : activeDisplay(layout);
    if (!located?.display.preview) return;
    viewLayout({ action: "keep_open", display_id: located.display.id });
  };

  /** Shows the Explorer with a file's row unfolded and selected; nothing is opened. */
  const revealInExplorer = (path: string) => {
    setWorkspaceView({ explorer: true, reveal: path });
    ui().setExplorerSelection(path);
  };

  /** One command of a display's tab menu or its area's overflow menu (B11). */
  const runViewMenu = (id: ViewMenuId, displayId: string) => {
    const layout = layoutNow();
    const located = layout ? locateDisplay(layout.root, displayId) : null;
    if (!layout || !located) return diagnostic(`view menu ${id}: ${displayId} is not open`);
    const edge = menuEdge(id);
    if (id === "keep_open") return keepViewOpen(displayId);
    if (id === "copy_path") return void navigator.clipboard?.writeText(located.display.path).catch(() => undefined);
    if (id === "reveal") return revealInExplorer(located.display.path);
    if (id === "close_view") return closeView(displayId);
    if (!edge) return;
    if (id.startsWith("split_")) return splitView(displayId, located.area.id, edge);
    const target = findArea(layout.root, neighbourArea(layout.root, located.area.id, edge) ?? "");
    if (!target) return diagnostic(`view menu ${id}: no view area lies ${edge} of ${located.area.id}`);
    moveView(displayId, target.id, target.displays.length);
  };

  /**
   * A View command from the palette on the active display or area (S7 B20).
   * One that cannot run now is not sent; the palette shows its reason.
   */
  const runViewCommand = (id: ViewCommandId) => {
    const layout = layoutNow();
    if (!layout) return diagnostic(`view command ${id}: no View areas in front`);
    const command = viewCommands(layout, drawnViews()).find((row) => row.id === id);
    if (!command || command.unavailable) return diagnostic(`view command ${id}: ${command?.unavailable ?? "unknown"}`);
    const active = activeDisplay(layout);
    switch (id) {
      case "split_right":
      case "split_down":
        if (active) splitView(active.display.id, active.area.id, id === "split_right" ? "right" : "down");
        return;
      case "move_next": {
        const next = active ? adjacentInOrder(layout.root, active.area.id, 1) : null;
        if (active && next) moveView(active.display.id, next.id, next.displays.length);
        return;
      }
      case "focus_next":
      case "focus_previous": {
        const next = adjacentInOrder(layout.root, layout.active_area, id === "focus_next" ? 1 : -1);
        if (next) focusViewArea(next.id);
        return;
      }
      case "close_view":
        if (active) closeView(active.display.id);
        return;
      case "grow":
      case "shrink": {
        const target = resizeTarget(layout, drawnViews()?.geometry ?? null, id === "grow");
        if ("ratio" in target) resizeViewSplit(target.splitId, target.ratio);
        return;
      }
    }
  };

  return {
    dispatch,
    revealAncestors,
    taskIdNow,

    openSettings() {
      ui().openOverlay("settings");
    },

    setAccent(hex: string) {
      updateUiState({ accent_hex: hex });
    },

    setFontSize(size: number) {
      updateUiState({ font_size: size });
    },

    /** The browser host's pane chords, replaced as a whole; the Swift host's map is untouched. */
    setBrowserShortcuts(bindings: Record<string, string>) {
      updateUiState({ browser_shortcut_bindings: bindings });
    },

    /** Whether this page is looking at the Agents tab; the daemon owns the core's flag. */
    observeAgents(observing: boolean) {
      dispatch({ schema_version: 2, kind: "ai_settings", payload: { observing } });
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

    /** An agent chosen on an Overview or in the Agents list: its Workspace and pane (B12). */
    openAgent(paneId: string) {
      beginOpening({ paneId });
      // An agent asked for is what a window too narrow for both regions shows (S7 B13).
      ui().setWorkingRegion("agents");
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

    focusTab(tabId: string) {
      if (remoteContext(rest())) {
        const host = remoteHost("Switching tab");
        if (host) sendRemote(host.targetId, { action: "focus_tab", tab_id: tabId });
        return;
      }
      const here = current();
      if (!here) return;
      dispatch({
        schema_version: 2,
        kind: "focus_tab",
        payload: { workspace_id: here.checkout.workspace_id, checkout_id: here.checkout.id, tab_id: tabId },
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
      if (remoteContext(rest())) {
        const host = remoteHost("Close tab");
        if (!host) return;
        const tab = host.view?.checkout.tabs.find((row) => row.id === (tabId ?? host.view?.tab?.id));
        if (!tab?.id) return diagnostic("close_tab: no visible remote tab");
        requestClose("tab", tab.id, tab.panes, host.targetId, host.agents);
        return;
      }
      // The close chord closes the active display when the View area holds
      // the keyboard or is all that shows, and the Agent tab otherwise.
      const display = tabId ? null : activeDisplayNow();
      if (display && viewAreaInUse(rest())) return closeView(display.display.id);
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

    toggleInactiveProjects(deviceId: string) {
      dispatch({ schema_version: 2, kind: "inactive_projects_toggle", payload: { device_id: deviceId } });
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

    setLayout(mode: ViewMode) {
      setWorkspaceView({ mode });
    },

    revealInExplorer,

    /**
     * Explorer and Changes open and close independently (B10). Showing a tool
     * the operator dismissed from over a narrow window shows it again without
     * an event: the core still holds it shown (S7 B12, B13).
     */
    setTool(tool: "explorer" | "changes", visible: boolean) {
      if (visible) ui().setToolsDismissed(false);
      const view = workspaceViewOf(rest());
      if (view && visible && (tool === "explorer" ? view.explorer : view.changes)) return;
      setWorkspaceView(tool === "explorer" ? { explorer: visible } : { changes: visible });
    },

    selectChange(path: string, committed: boolean, preview: boolean) {
      const here = explorerContext(rest()).checkout;
      const changes = useShellStore.getState().changes;
      const scope = rest()?.navigator?.changes_root_path;
      if (!here || !scope || changes?.root_path !== scope ||
          (scope !== here.path && !scope.startsWith(`${here.path}/`))) {
        return diagnostic("changes_select: checkout is unavailable");
      }
      const group = committed ? changes.committed : changes.entries;
      if (!group.some((entry) => entry.path === path)) return diagnostic("changes_select: row is no longer available");
      dispatch({ schema_version: 2, kind: "changes_select", payload: { path, committed, preview } });
    },

    openShortcuts() {
      ui().openOverlay(ui().overlay === "shortcuts" ? "none" : "shortcuts");
    },

    openFind() {
      // The core searches a pane's history through this machine's Herdr
      // only, so with an SSH device selected Find is refused with a notice
      // rather than run on this machine (PRD S5 B19).
      const context = remoteContext(rest());
      if (context) {
        ui().setNotice({ text: `Find in pane is not available for ${context.device.label} from the web shell; nothing was sent.`, refreshable: false });
        return;
      }
      ui().openOverlay("find");
    },

    /** The registration field lives in the sidebar, so a hidden sidebar is shown first. */
    openNewWorkspace() {
      setLeftSidebarVisible(true);
      ui().openOverlay("new_workspace");
    },

    /** ⌘K, ⌘P and the right-panel chords are claimed now and answered in S3. */
    notReady(label: string) {
      ui().setNotice({ text: `${label}: 준비 중 (S3)`, refreshable: false });
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

    /** ⌘⇧K, a double click or Keep open: a preview display becomes an ordinary one. */
    keepViewOpen,

    focusView,
    focusViewArea,
    moveView,
    splitView,
    resizeViewSplit,
    closeView,
    runViewMenu,
    runViewCommand,

    /** Re-reads an unavailable display's file (S7 B16). */
    retryView(displayId: string) {
      viewLayout({ action: "retry", display_id: displayId });
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
