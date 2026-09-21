import { connectionAfterHealthFails, nextBackoff } from "./connection";
import { noteArrival } from "./probe";
import { useShellStore, type TerminalChunk } from "./store";

export type DispatchFn = (event: {
  schema_version: number;
  kind: string;
  payload: Record<string, unknown>;
}) => void;

type Handlers = {
  onChunks: (chunks: TerminalChunk[], full: boolean) => void;
};

function tokenFromLocation(): string | null {
  const hash = new URLSearchParams(window.location.hash.replace(/^#/, ""));
  return hash.get("token");
}

async function healthOk(): Promise<boolean> {
  try {
    const response = await fetch("/health", { cache: "no-store" });
    return response.ok;
  } catch {
    return false;
  }
}

export function connectShell(handlers: Handlers): { dispatch: DispatchFn; close: () => void } {
  let socket: WebSocket | null = null;
  let closed = false;
  let backoff = 500;
  let healthFails = 0;
  let revision = 0;
  let terminalSequence = 0;
  let reconnectTimer: number | undefined;

  const dispatch: DispatchFn = (event) => {
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    socket.send(JSON.stringify(event));
  };

  const open = () => {
    if (closed) return;
    const token = tokenFromLocation();
    if (!token) {
      useShellStore.getState().setConnection("gone");
      return;
    }
    useShellStore.getState().setConnection(socket ? "reconnecting" : "connecting");
    const ws = new WebSocket(`${location.protocol === "https:" ? "wss" : "ws"}://${location.host}/ws`);
    socket = ws;
    ws.addEventListener("open", () => {
      ws.send(
        JSON.stringify({
          token,
          schema_version: 2,
          have_revision: revision,
          have_terminal_sequence: terminalSequence,
        }),
      );
    });
    ws.addEventListener("message", (event) => {
      noteArrival();
      const frame = JSON.parse(String(event.data)) as {
        type: string;
        payload?: { revision?: number; terminal_sequence?: number };
      };
      const chunks = useShellStore.getState().applyFrame(frame);
      revision = useShellStore.getState().revision;
      terminalSequence = useShellStore.getState().terminalSequence;
      handlers.onChunks(chunks, frame.type === "snapshot");
      useShellStore.getState().setConnection("live");
      backoff = 500;
      healthFails = 0;
    });
    ws.addEventListener("close", (event) => {
      if (closed) return;
      if (event.code >= 4001 && event.code <= 4004) {
        useShellStore.getState().setConnection("gone", true);
        return;
      }
      void scheduleReconnect();
    });
    ws.addEventListener("error", () => {
      ws.close();
    });
  };

  const scheduleReconnect = async () => {
    useShellStore.getState().setConnection("reconnecting");
    const ok = await healthOk();
    healthFails = ok ? 0 : healthFails + 1;
    const next = connectionAfterHealthFails(healthFails);
    if (next === "gone") {
      useShellStore.getState().setConnection("gone");
      return;
    }
    backoff = nextBackoff(backoff);
    reconnectTimer = window.setTimeout(open, backoff);
  };

  open();
  return {
    dispatch,
    close: () => {
      closed = true;
      if (reconnectTimer) window.clearTimeout(reconnectTimer);
      socket?.close();
    },
  };
}
