import type { StoredBuffer } from "./buffers";
import { create } from "zustand";
import type { ConnectionState } from "./connection";
import { remoteContext, remoteView } from "./remote";
import { share } from "./share";
import {
  type AgentRow,
  type ChangesSnapshot,
  type DocumentsSection,
  type EditorDocumentSnapshot,
  type EditorSnapshot,
  type PaneFind,
  type SnapshotRest,
} from "./snapshot";

export type { AgentRow, SnapshotRest } from "./snapshot";

function focusedPaneOf(rest: SnapshotRest): string | null {
  const remote = remoteContext(rest);
  if (remote) return remoteView(remote.session)?.focusedPaneId ?? null;
  return rest.terminal?.pane_id ?? rest.focused?.pane_id ?? null;
}

export type TerminalChunk = {
  pane_id: string;
  sequence: number;
  bytes_base64: string;
};

/** `inode` is the entry's own identity in a checkout listing, which a trash of the row confirms. */
export type DirectoryEntry = { name: string; path: string; is_directory: boolean; inode?: number };
export type DirectoryList = {
  /** The event that asked: `remote_file_list` for the registration input, `file_list` for the Explorer. */
  kind: string;
  root_path: string;
  entries: DirectoryEntry[];
  truncated: boolean;
};
/** A device folder its helper could not list now (`directory_unavailable`); the reason is the helper's. */
export type DirectoryUnavailable = { device_id: string; root_path: string; code: string; message: string };
export type PathRefusal = { kind: string; path: string; reason: string };
export type DirectoryChanged = { path: string };
export type FileIndexEntry = { path: string; relative_path: string };
export type FileIndexResult = {
  /** The device the root is on; `local` for this Hide host. */
  device_id: string;
  root_path: string;
  query: string;
  /** Named `files`, not `entries`: the directory_list frame owns `entries`. */
  files: FileIndexEntry[];
  truncated: boolean;
  indexing: boolean;
  /** Why a device's helper could not walk the root; the next query walks again. */
  unavailable: string | null;
};

export type Frame = {
  type: string;
  message?: string;
  payload?: {
    revision?: number;
    terminal_sequence?: number;
    rest?: SnapshotRest;
    editor?: EditorSnapshot | null;
    changes?: ChangesSnapshot | null;
    documents?: DocumentsSection | null;
    find?: PaneFind;
    chunks?: TerminalChunk[];
  } & Partial<DirectoryList> &
    Partial<DirectoryUnavailable> &
    Partial<PathRefusal> &
    Partial<DirectoryChanged> &
    Partial<FileIndexResult>;
};

/** What the daemon says about itself after a handshake (hided `daemon` frame). */
export type DaemonInfo = {
  version: string;
  /** This daemon host's lasting identity; a draft is filed under it (S5.5 B9). */
  host_id: string;
  /** The machine the daemon runs on, which owns every value it stores (S5.5 B35); null when the system gives none. */
  host_name: string | null;
  schema_version: number;
  pid: number;
  started_at_unix: string;
  state_dir: string;
  core_state_path: string;
  herdr_bin_path: string | null;
  herdr_socket_path: string | null;
  keep_alive: boolean;
  idle_secs: number;
};

type Store = {
  connection: ConnectionState;
  /** The daemon this page is connected to; null until the first handshake. */
  daemon: DaemonInfo | null;
  revision: number;
  terminalSequence: number;
  /** The core's rest section, structurally shared across frames (`share.ts`). */
  rest: SnapshotRest | null;
  /**
   * The core's editor section, one of the three sections the core revisions on
   * its own. A delta carries it only when it changed, so a null in a delta
   * keeps what the store holds; a snapshot always carries every section.
   */
  editor: EditorSnapshot | null;
  /** The core's changes section, read the same way and for the same reason. */
  changes: ChangesSnapshot | null;
  /**
   * The documents the front Workspace's visible displays show, by editor tab
   * id (S7 contract 3.1). A snapshot replaces the map; a delta that carries
   * the section keeps the documents still visible, overwrites the ones that
   * changed and drops the rest; a delta without it keeps the map as it is.
   */
  documents: Documents;
  agents: AgentRow[];
  /**
   * The keyboard-focus pane of the context on screen: the core's
   * `terminal.pane_id` for this machine, the host's own focus for a selected
   * SSH device (`remote.ts`).
   */
  focusedPaneId: string | null;
  herdrState: string | null;
  find: PaneFind | null;
  /** The last registration listing hided answered; the registration input reads it. */
  directoryList: DirectoryList | null;
  /** The Explorer's listings, one per expanded folder, oldest evicted past `LISTING_CAP`. */
  listings: Record<string, DirectoryList>;
  /** The last path hided refused; cleared when the input changes. */
  pathRefusal: PathRefusal | null;
  /** The last device folder that could not be listed, until a listing for it arrives. */
  directoryUnavailable: DirectoryUnavailable | null;
  /** The ⌘P palette's last answer, keyed by the query it answered. */
  fileIndex: FileIndexResult | null;
  /** The last attachment the daemon refused, drawn as one line over its pane (B15). */
  attachmentRefusal: { pane_id: string; reason: string } | null;
  /** Tabs whose save is in flight; the strip shows only these (D-10). */
  savingTabs: Set<string>;
  /** Tabs whose buffer could not be stored; they say "kept in this tab only" (D-14). */
  bufferWarnings: Set<string>;
  /** The draft text last exported per tab, which no longer needs its tab to survive (B26, B44). */
  exportedDrafts: Map<string, string>;
  /** Stored drafts no open tab stands for, to open, export or discard (S5.5 B10-B12). */
  recoveryDrafts: StoredBuffer[];
  setRecoveryDrafts: (drafts: StoredBuffer[]) => void;
  /** Watch frames per folder, counted so the Explorer re-reads even a folder
   * whose listing was in flight when the change landed. */
  folderChanges: Record<string, number>;
  /** The newest `DIAGNOSTIC_CAP` entries; older ones are counted in `diagnosticsDropped`. */
  diagnostics: string[];
  diagnosticsDropped: number;
  /** Counts self-contained snapshots; every terminal re-requests its view on each. */
  viewGeneration: number;
  refused: boolean;
  setConnection: (connection: ConnectionState, refused?: boolean) => void;
  noteDiagnostic: (message: string) => void;
  clearPathRefusal: () => void;
  setAttachmentRefusal: (refusal: { pane_id: string; reason: string } | null) => void;
  noteSaving: (tabId: string, saving: boolean) => void;
  noteBufferWarning: (tabId: string, warned: boolean) => void;
  noteDraftExported: (tabId: string, contents: string) => void;
  /** Drops cached listings so the Explorer re-reads those folders. */
  invalidateListings: (paths: string[]) => void;
  applyFrame: (frame: Frame) => TerminalChunk[];
};

export type Documents = Readonly<Record<string, EditorDocumentSnapshot>>;

const NO_DOCUMENTS: Documents = {};

/**
 * One `documents` section applied to the map the store holds. An unchanged
 * document keeps its object, and a section that changes nothing returns the
 * same map, so a view of another document does not redraw for this one.
 */
function mergeDocuments(previous: Documents, section: DocumentsSection): Documents {
  const next: Record<string, EditorDocumentSnapshot> = {};
  for (const id of section.visible) {
    const kept = previous[id];
    if (kept) next[id] = kept;
  }
  for (const change of section.changed) next[change.tab_id] = change.document;
  const ids = Object.keys(next);
  const same = ids.length === Object.keys(previous).length && ids.every((id) => previous[id] === next[id]);
  return same ? previous : next;
}

const KNOWN_GROUPS = new Set(["needs_you", "done", "working", "seen"]);
export const DIAGNOSTIC_CAP = 200;

/** Expanded folders the store keeps a listing for; the PRD's watch cap is the same number. */
export const LISTING_CAP = 64;

/** Watch-frame counts the store keeps; far above the 64-folder watch set. */
export const FOLDER_CHANGE_CAP = 512;

function withListing(
  listings: Record<string, DirectoryList>,
  listing: DirectoryList,
): Record<string, DirectoryList> {
  const next: Record<string, DirectoryList> = { ...listings, [listing.root_path]: listing };
  const paths = Object.keys(next);
  const overflow = Math.max(0, paths.length - LISTING_CAP);
  for (const path of paths.slice(0, overflow)) delete next[path];
  return next;
}

function withDiagnostics(
  diagnostics: string[],
  dropped: number,
  added: string[],
): { diagnostics: string[]; diagnosticsDropped: number } {
  if (added.length === 0) return { diagnostics, diagnosticsDropped: dropped };
  const all = [...diagnostics, ...added];
  const overflow = Math.max(0, all.length - DIAGNOSTIC_CAP);
  return { diagnostics: all.slice(overflow), diagnosticsDropped: dropped + overflow };
}

export const useShellStore = create<Store>((set, get) => ({
  connection: "connecting",
  daemon: null,
  revision: 0,
  terminalSequence: 0,
  rest: null,
  editor: null,
  changes: null,
  documents: NO_DOCUMENTS,
  agents: [],
  focusedPaneId: null,
  herdrState: null,
  find: null,
  directoryList: null,
  listings: {},
  pathRefusal: null,
  directoryUnavailable: null,
  fileIndex: null,
  attachmentRefusal: null,
  savingTabs: new Set<string>(),
  bufferWarnings: new Set<string>(),
  exportedDrafts: new Map<string, string>(),
  recoveryDrafts: [],
  setRecoveryDrafts: (recoveryDrafts) => set({ recoveryDrafts }),
  folderChanges: {},
  diagnostics: [],
  diagnosticsDropped: 0,
  viewGeneration: 0,
  refused: false,
  setConnection: (connection, refused = false) => set({ connection, refused }),
  noteDiagnostic: (message) =>
    set(withDiagnostics(get().diagnostics, get().diagnosticsDropped, [message])),
  clearPathRefusal: () => {
    if (get().pathRefusal) set({ pathRefusal: null });
  },
  setAttachmentRefusal: (attachmentRefusal) => set({ attachmentRefusal }),
  noteDraftExported: (tabId, contents) => {
    const next = new Map(get().exportedDrafts);
    next.set(tabId, contents);
    set({ exportedDrafts: next });
  },
  noteBufferWarning: (tabId, warned) => {
    const current = get().bufferWarnings;
    if (warned === current.has(tabId)) return;
    const next = new Set(current);
    if (warned) next.add(tabId);
    else next.delete(tabId);
    // A draft stored here again no longer needs the text it was exported as.
    if (!warned && get().exportedDrafts.has(tabId)) {
      const exportedDrafts = new Map(get().exportedDrafts);
      exportedDrafts.delete(tabId);
      set({ bufferWarnings: next, exportedDrafts });
      return;
    }
    set({ bufferWarnings: next });
  },
  noteSaving: (tabId, saving) => {
    const current = get().savingTabs;
    if (saving === current.has(tabId)) return;
    const next = new Set(current);
    if (saving) next.add(tabId);
    else next.delete(tabId);
    set({ savingTabs: next });
  },
  invalidateListings: (paths) => {
    const listings = get().listings;
    if (!paths.some((path) => path in listings)) return;
    const next = { ...listings };
    for (const path of paths) delete next[path];
    set({ listings: next });
  },
  applyFrame: (frame) => {
    const payload = frame.payload ?? {};
    // The core revisions `editor`, `changes` and `documents` on their own,
    // beside `rest`: a snapshot carries every section, a delta only the ones
    // that changed since the client's revision. They land before the routing
    // below, which answers a listing or a refusal with an early return.
    if (frame.type === "snapshot") {
      set({
        editor: payload.editor ?? null,
        changes: payload.changes ?? null,
        documents: payload.documents ? mergeDocuments(NO_DOCUMENTS, payload.documents) : NO_DOCUMENTS,
      });
    } else if (payload.editor !== undefined || payload.changes !== undefined || payload.documents !== undefined) {
      set({
        editor: payload.editor ?? get().editor,
        changes: payload.changes ?? get().changes,
        documents: payload.documents ? mergeDocuments(get().documents, payload.documents) : get().documents,
      });
    }
    if (frame.type === "directory_list") {
      const listing: DirectoryList = {
        kind: payload.kind ?? "",
        root_path: payload.root_path ?? "",
        entries: payload.entries ?? [],
        truncated: payload.truncated ?? false,
      };
      if (listing.kind === "file_list") {
        // A listing belongs to the device that answered it; one for a device
        // no longer selected would show its rows under this device's paths.
        if ((payload.device_id ?? "local") !== (get().rest?.navigator?.focused_device_id ?? "local")) return [];
        const unavailable = get().directoryUnavailable;
        set({
          listings: withListing(get().listings, listing),
          directoryUnavailable: unavailable?.root_path === listing.root_path ? null : unavailable,
        });
      } else if (listing.kind === "remote_file_list") {
        set({ directoryList: listing });
      } else {
        get().noteDiagnostic(`directory_list without a known kind=${listing.kind || "none"}`);
      }
      return [];
    }
    if (frame.type === "directory_unavailable") {
      set({
        directoryUnavailable: {
          device_id: payload.device_id ?? "",
          root_path: payload.root_path ?? "",
          code: payload.code ?? "",
          message: payload.message ?? "",
        },
      });
      return [];
    }
    if (frame.type === "path_refused") {
      set({
        pathRefusal: { kind: payload.kind ?? "", path: payload.path ?? "", reason: payload.reason ?? "" },
      });
      return [];
    }
    if (frame.type === "directory_changed") {
      // A watched folder moved; its cached listing is dropped and the count
      // lets the Explorer re-read even a listing that was still in flight.
      // Listings are the shown device's, so another device's frame names a
      // path that is not the one listed here (S5.5 B2).
      const path = payload.path;
      const shownDevice = get().rest?.navigator?.focused_device_id ?? "local";
      if (path && (payload.device_id ?? "local") === shownDevice) {
        get().invalidateListings([path]);
        // A bounded map ordered by recency: the count is re-inserted so a
        // folder that keeps changing is the last one evicted.
        const next = { ...get().folderChanges };
        delete next[path];
        next[path] = (get().folderChanges[path] ?? 0) + 1;
        const paths = Object.keys(next);
        const overflow = paths.length - FOLDER_CHANGE_CAP;
        for (const stale of overflow > 0 ? paths.slice(0, overflow) : []) delete next[stale];
        set({ folderChanges: next });
      }
      return [];
    }
    if (frame.type === "open_external_result") return [];
    if (frame.type === "daemon") {
      set({ daemon: frame.payload as unknown as DaemonInfo });
      return [];
    }
    if (frame.type === "file_index_result") {
      set({
        fileIndex: {
          device_id: payload.device_id ?? "local",
          unavailable: payload.unavailable ?? null,
          root_path: payload.root_path ?? "",
          query: payload.query ?? "",
          files: payload.files ?? [],
          truncated: payload.truncated ?? false,
          indexing: payload.indexing ?? false,
        },
      });
      return [];
    }
    if (frame.type === "error") {
      get().noteDiagnostic(`hided error: ${frame.message ?? "unknown"}`);
      return [];
    }
    const chunks = payload.chunks ?? [];
    const cursors = {
      revision: payload.revision ?? get().revision,
      terminalSequence: payload.terminal_sequence ?? get().terminalSequence,
      find: payload.find ? share(get().find, payload.find) : get().find,
    };
    if (frame.type === "snapshot" || payload.rest) {
      const previous = get().rest;
      // The core leaves `workspace_view` out of the frame when no Workspace
      // is in front, so a delta that carries rest without it clears it.
      const incoming =
        frame.type === "snapshot"
          ? payload.rest ?? {}
          : { ...previous, ...payload.rest, workspace_view: payload.rest?.workspace_view };
      const rest = share(previous, incoming);
      const diagnostics: string[] = [];
      const agents =
        rest.navigator?.agents === previous?.navigator?.agents
          ? get().agents
          : (rest.navigator?.agents ?? []).map((agent) => {
              if (agent.group && !KNOWN_GROUPS.has(agent.group)) {
                diagnostics.push(`unknown enum group=${agent.group}`);
                return { ...agent, group: "unknown", unknown: true, symbol: "?", status_label: "unknown" };
              }
              return agent;
            });
      const lastError = rest.status?.last_error;
      if (lastError && lastError.occurred_at !== previous?.status?.last_error?.occurred_at) {
        diagnostics.push(`${lastError.kind}: ${lastError.message}`);
      }
      // Listings are one device's folders; switching devices starts empty.
      const deviceChanged = (rest.navigator?.focused_device_id ?? "local") !== (previous?.navigator?.focused_device_id ?? "local");
      set({
        rest,
        agents,
        ...(deviceChanged ? { listings: {}, directoryUnavailable: null } : {}),
        ...withDiagnostics(get().diagnostics, get().diagnosticsDropped, diagnostics),
        viewGeneration: frame.type === "snapshot" ? get().viewGeneration + 1 : get().viewGeneration,
        ...cursors,
        focusedPaneId: focusedPaneOf(rest),
        herdrState: rest.status?.herdr?.state ?? get().herdrState,
      });
    } else {
      set(cursors);
    }
    return chunks;
  },
}));
