import { create } from "zustand";
import type { ConnectionState } from "./connection";

export type AgentRow = {
  id: string;
  pane_id: string;
  identity_label: string;
  agent_kind: string;
  symbol: string;
  group: string;
  status_label: string;
  detail?: string | null;
  elapsed: string;
  emphasized: boolean;
  unread: boolean;
  unknown?: boolean;
};

export type TerminalChunk = {
  pane_id: string;
  sequence: number;
  bytes_base64: string;
};

export type SnapshotRest = {
  navigator?: { agents?: AgentRow[] };
  focused?: { pane_id?: string | null };
  status?: { herdr?: { state?: string; message?: string | null } };
  terminal?: { pane_id?: string | null };
};

type Store = {
  connection: ConnectionState;
  revision: number;
  terminalSequence: number;
  rest: SnapshotRest | null;
  agents: AgentRow[];
  focusedPaneId: string | null;
  herdrState: string | null;
  diagnostics: string[];
  setConnection: (connection: ConnectionState) => void;
  applyFrame: (frame: {
    type: string;
    payload?: {
      revision?: number;
      terminal_sequence?: number;
      rest?: SnapshotRest;
      chunks?: TerminalChunk[];
    };
  }) => TerminalChunk[];
};

const KNOWN_GROUPS = new Set(["needs_you", "done", "working", "seen"]);

export const useShellStore = create<Store>((set, get) => ({
  connection: "connecting",
  revision: 0,
  terminalSequence: 0,
  rest: null,
  agents: [],
  focusedPaneId: null,
  herdrState: null,
  diagnostics: [],
  setConnection: (connection) => set({ connection }),
  applyFrame: (frame) => {
    const payload = frame.payload ?? {};
    const chunks = payload.chunks ?? [];
    if (frame.type === "snapshot" || payload.rest) {
      const rest = frame.type === "snapshot" ? payload.rest ?? {} : { ...get().rest, ...payload.rest };
      const rawAgents = rest.navigator?.agents ?? [];
      const diagnostics = [...get().diagnostics];
      const agents = rawAgents.map((agent) => {
        if (agent.group && !KNOWN_GROUPS.has(agent.group)) {
          diagnostics.push(`unknown enum group=${agent.group}`);
          return { ...agent, group: "unknown", unknown: true, symbol: "?", status_label: "unknown" };
        }
        return agent;
      });
      set({
        rest,
        agents,
        diagnostics,
        revision: payload.revision ?? get().revision,
        terminalSequence: payload.terminal_sequence ?? get().terminalSequence,
        focusedPaneId: rest.focused?.pane_id ?? rest.terminal?.pane_id ?? get().focusedPaneId,
        herdrState: rest.status?.herdr?.state ?? get().herdrState,
      });
    } else {
      set({
        revision: payload.revision ?? get().revision,
        terminalSequence: payload.terminal_sequence ?? get().terminalSequence,
      });
    }
    return chunks;
  },
}));
