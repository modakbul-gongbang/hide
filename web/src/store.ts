import { create } from "zustand";
import type { ConnectionState } from "./connection";
import { share } from "./share";
import { type AgentRow, type PaneFind, type SnapshotRest } from "./snapshot";

export type { AgentRow, SnapshotRest } from "./snapshot";

export type TerminalChunk = {
  pane_id: string;
  sequence: number;
  bytes_base64: string;
};

export type DirectoryEntry = { name: string; path: string };
export type DirectoryList = { root_path: string; entries: DirectoryEntry[]; truncated: boolean };
export type PathRefusal = { kind: string; path: string; reason: string };

export type Frame = {
  type: string;
  message?: string;
  payload?: {
    revision?: number;
    terminal_sequence?: number;
    rest?: SnapshotRest;
    find?: PaneFind;
    chunks?: TerminalChunk[];
  } & Partial<DirectoryList> &
    Partial<PathRefusal>;
};

type Store = {
  connection: ConnectionState;
  revision: number;
  terminalSequence: number;
  /** The core's rest section, structurally shared across frames (`share.ts`). */
  rest: SnapshotRest | null;
  agents: AgentRow[];
  /** The keyboard-focus pane the core reports (`terminal.pane_id`). */
  focusedPaneId: string | null;
  herdrState: string | null;
  find: PaneFind | null;
  /** The last directory listing hided answered; the registration input reads it. */
  directoryList: DirectoryList | null;
  /** The last path hided refused; cleared when the input changes. */
  pathRefusal: PathRefusal | null;
  /** The newest `DIAGNOSTIC_CAP` entries; older ones are counted in `diagnosticsDropped`. */
  diagnostics: string[];
  diagnosticsDropped: number;
  /** Counts self-contained snapshots; every terminal re-requests its view on each. */
  viewGeneration: number;
  refused: boolean;
  setConnection: (connection: ConnectionState, refused?: boolean) => void;
  noteDiagnostic: (message: string) => void;
  clearPathRefusal: () => void;
  applyFrame: (frame: Frame) => TerminalChunk[];
};

const KNOWN_GROUPS = new Set(["needs_you", "done", "working", "seen"]);
export const DIAGNOSTIC_CAP = 200;

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
  agents: [],
  focusedPaneId: null,
  herdrState: null,
  find: null,
  directoryList: null,
  pathRefusal: null,
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
  applyFrame: (frame) => {
    const payload = frame.payload ?? {};
    if (frame.type === "directory_list") {
      set({
        directoryList: {
          root_path: payload.root_path ?? "",
          entries: payload.entries ?? [],
          truncated: payload.truncated ?? false,
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
