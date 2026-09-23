import { create } from "zustand";
import type { ConnectionState } from "./connection";
import { share } from "./share";
import {
  type AgentRow,
  type ChangesSnapshot,
  type EditorSnapshot,
  type PaneFind,
  type SnapshotRest,
} from "./snapshot";

export type { AgentRow, SnapshotRest } from "./snapshot";

export type TerminalChunk = {
  pane_id: string;
  sequence: number;
  bytes_base64: string;
};

export type DirectoryEntry = { name: string; path: string; is_directory: boolean };
export type DirectoryList = {
  /** The event that asked: `remote_file_list` for the registration input, `file_list` for the Explorer. */
  kind: string;
  root_path: string;
  entries: DirectoryEntry[];
  truncated: boolean;
};
export type PathRefusal = { kind: string; path: string; reason: string };
export type DirectoryChanged = { path: string };
export type FileIndexEntry = { path: string; relative_path: string };
export type OpenExternalResult = {
  path: string;
  ok: boolean;
  reason: string | null;
};
export type FileIndexResult = {
  root_path: string;
  query: string;
  /** Named `files`, not `entries`: the directory_list frame owns `entries`. */
  files: FileIndexEntry[];
  truncated: boolean;
  indexing: boolean;
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
    find?: PaneFind;
    chunks?: TerminalChunk[];
  } & Partial<DirectoryList> &
    Partial<PathRefusal> &
    Partial<DirectoryChanged> &
    Partial<FileIndexResult> &
    Partial<OpenExternalResult>;
};

type Store = {
  connection: ConnectionState;
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
  agents: AgentRow[];
  /** The keyboard-focus pane the core reports (`terminal.pane_id`). */
  focusedPaneId: string | null;
  herdrState: string | null;
  find: PaneFind | null;
  /** The last registration listing hided answered; the registration input reads it. */
  directoryList: DirectoryList | null;
  /** The Explorer's listings, one per expanded folder, oldest evicted past `LISTING_CAP`. */
  listings: Record<string, DirectoryList>;
  /** The last path hided refused; cleared when the input changes. */
  pathRefusal: PathRefusal | null;
  /** The ⌘P palette's last answer, keyed by the query it answered. */
  fileIndex: FileIndexResult | null;
  /** The last attachment the daemon refused, drawn as one line over its pane (B15). */
  attachmentRefusal: { pane_id: string; reason: string } | null;
  /** The last open-in-default-app answer, drawn under the document (D-12). */
  externalOpen: OpenExternalResult | null;
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
  /** Drops cached listings so the Explorer re-reads those folders. */
  invalidateListings: (paths: string[]) => void;
  applyFrame: (frame: Frame) => TerminalChunk[];
};

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
  revision: 0,
  terminalSequence: 0,
  rest: null,
  editor: null,
  changes: null,
  agents: [],
  focusedPaneId: null,
  herdrState: null,
  find: null,
  directoryList: null,
  listings: {},
  pathRefusal: null,
  fileIndex: null,
  attachmentRefusal: null,
  externalOpen: null,
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
  invalidateListings: (paths) => {
    const listings = get().listings;
    if (!paths.some((path) => path in listings)) return;
    const next = { ...listings };
    for (const path of paths) delete next[path];
    set({ listings: next });
  },
  applyFrame: (frame) => {
    const payload = frame.payload ?? {};
    // The core revisions `editor` and `changes` on their own, beside `rest`:
    // a snapshot carries every section, a delta only the ones that changed
    // since the client's revision. Both land before the routing below, which
    // answers a listing or a refusal with an early return.
    if (frame.type === "snapshot") {
      set({ editor: payload.editor ?? null, changes: payload.changes ?? null });
    } else if (payload.editor !== undefined || payload.changes !== undefined) {
      set({
        editor: payload.editor ?? get().editor,
        changes: payload.changes ?? get().changes,
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
        set({ listings: withListing(get().listings, listing) });
      } else if (listing.kind === "remote_file_list") {
        set({ directoryList: listing });
      } else {
        get().noteDiagnostic(`directory_list without a known kind=${listing.kind || "none"}`);
      }
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
      const path = payload.path;
      if (path) {
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
    if (frame.type === "open_external_result") {
      set({
        externalOpen: {
          path: payload.path ?? "",
          ok: payload.ok ?? false,
          reason: payload.reason ?? null,
        },
      });
      return [];
    }
    if (frame.type === "file_index_result") {
      set({
        fileIndex: {
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
      const incoming = frame.type === "snapshot" ? payload.rest ?? {} : { ...previous, ...payload.rest };
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
      set({
        rest,
        agents,
        ...withDiagnostics(get().diagnostics, get().diagnosticsDropped, diagnostics),
        viewGeneration: frame.type === "snapshot" ? get().viewGeneration + 1 : get().viewGeneration,
        ...cursors,
        focusedPaneId: rest.terminal?.pane_id ?? rest.focused?.pane_id ?? null,
        herdrState: rest.status?.herdr?.state ?? get().herdrState,
      });
    } else {
      set(cursors);
    }
    return chunks;
  },
}));
