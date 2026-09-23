import { configureAttachments, receiveAttachmentRefusal } from "./attachments";
import { connectionAfterHealthFails, nextBackoff } from "./connection";
import { clearPending, receiveBytes, receiveBytesError, receiveBytesRefusal } from "./fileBytes";
import { noteArrival, probeEnabled } from "./probe";
import { useShellStore, type TerminalChunk } from "./store";

export type DispatchFn = (event: {
  schema_version: number;
  kind: string;
  payload: Record<string, unknown>;
}) => unknown;

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

export function connectShell(handlers: Handlers): { dispatch: DispatchFn; sendBinary: (bytes: Uint8Array) => void; close: () => void; drop: () => void } {
  let socket: WebSocket | null = null;
  let closed = false;
  let backoff = 500;
  let healthFails = 0;
  let revision = 0;
  let terminalSequence = 0;
  let reconnectTimer: number | undefined;

  // Decided once: the probe is a measurement seam, not a per-frame branch.
  const probing = probeEnabled();

  const dispatch: DispatchFn = (event) => {
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      // A key typed while reconnecting is lost, not queued; the badge shows
      // the state and the log keeps the fact.
      useShellStore.getState().noteDiagnostic(`dispatch dropped: ${event.kind} while socket not open`);
      return false;
    }
    socket.send(JSON.stringify(event));
    return true;
  };

  /** Attachment bytes ride the same socket as binary frames (PRD B14). */
  const sendBinary = (bytes: Uint8Array) => {
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      useShellStore.getState().noteDiagnostic("attachment bytes dropped while socket not open");
      return;
    }
    socket.send(bytes);
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
    // File bytes arrive as binary frames on this same socket; the header and
    // payload are split by the file-bytes reader, not by the snapshot store.
    ws.binaryType = "arraybuffer";
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
      if (probing) noteArrival();
      if (event.data instanceof ArrayBuffer) {
        receiveBytes(event.data);
        return;
      }
      const frame = JSON.parse(String(event.data)) as {
        type: string;
        payload?: { revision?: number; terminal_sequence?: number };
      };
      if (frame.type === "file_bytes_error") {
        receiveBytesError(frame.payload as unknown as { request_id: string; reason: string });
        return;
      }
      if (frame.type === "attachment_refused") {
        receiveAttachmentRefusal(frame.payload as unknown as { request_id: string; reason: string });
        return;
      }
      // A refused file_bytes path is answered to the viewer that asked, not
      // the Explorer's refusal line.
      if (frame.type === "path_refused" && (frame.payload as { kind?: string } | undefined)?.kind === "file_bytes") {
        const refusal = frame.payload as unknown as { path: string; reason: string };
        receiveBytesRefusal(refusal.path, refusal.reason);
        return;
      }
      const chunks = useShellStore.getState().applyFrame(frame);
      revision = useShellStore.getState().revision;
      terminalSequence = useShellStore.getState().terminalSequence;
      handlers.onChunks(chunks, frame.type === "snapshot");
      useShellStore.getState().setConnection("live");
      backoff = 500;
      healthFails = 0;
    });
    ws.addEventListener("close", (event) => {
      // Only the socket that died fails its own reads; a stale socket's close
      // must not reject a read in flight on its successor.
      if (socket === ws) clearPending("socket_closed");
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
  configureAttachments({ dispatch, sendBinary });
  return {
    dispatch,
    sendBinary,
    /** Closes the socket as a server drop would, so the reconnect path can be exercised (probe seam). */
    drop: () => socket?.close(),
    close: () => {
      closed = true;
      if (reconnectTimer) window.clearTimeout(reconnectTimer);
      socket?.close();
    },
  };
}
