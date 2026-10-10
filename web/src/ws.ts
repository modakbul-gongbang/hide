import { configureAttachments, receiveAttachmentRefusal } from "./attachments";
import { connectionAfterHealthFails, nextBackoff } from "./connection";
import type { MoveView } from "./coreMove";
import { clearPending, receiveBytes, receiveBytesError, receiveBytesRefusal } from "./fileBytes";
import { isOperatorFocus, numbered } from "./operatorFocus";
import { noteArrival, probeEnabled } from "./probe";
import { useShellStore, type TerminalChunk } from "./store";
import { hostKind } from "./host";

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

/**
 * A move that finished names the node this window became, or none after a
 * move back (PRD core-host-node-move, Window handoff): the page writes it
 * into its own address, so a reload or the host's next attach opens the
 * screen of the machine it is on.
 */
export function addressAfterMove(hash: string, view: MoveView): string | null {
  if (view.state !== "done") return null;
  const params = new URLSearchParams(hash.replace(/^#/, ""));
  const before = params.get("node");
  const node = view.direction === "back" ? null : view.node;
  if (before === node) return null;
  if (node) params.set("node", node);
  else params.delete("node");
  return `#${params.toString()}`;
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
  // The core that numbered `revision`, and the one the last daemon frame
  // named: a revision counts only on the core that gave it, so the second
  // becomes the first only when a snapshot or delta from it is applied.
  let revisionCore: string | null = null;
  let announcedCore: string | null = null;
  let terminalSequence: number | null = null;
  let terminalEpoch: string | null = null;
  let operatorFocusSequence = 0;
  let reconnectTimer: number | undefined;

  // Decided once: the probe is a measurement seam, not a per-frame branch.
  const probing = probeEnabled();

  const dispatch: DispatchFn = (event) => {
    // A move holds the window until the machine that takes the core draws
    // it; its screen refuses events, so none is sent (W1).
    if (useShellStore.getState().connection === "moving") {
      useShellStore.getState().noteDiagnostic(`dispatch dropped: ${event.kind} while the core moves`);
      return false;
    }
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      // A key typed while reconnecting is lost, not queued; the badge shows
      // the state and the log keeps the fact.
      useShellStore.getState().noteDiagnostic(`dispatch dropped: ${event.kind} while socket not open`);
      return false;
    }
    // Each operator focus carries this page's next number, so the snapshot
    // can say whether it includes the last one sent (`operatorFocus.ts`).
    // The number counts only once the frame has left.
    if (isOperatorFocus(event)) {
      const sequence = operatorFocusSequence + 1;
      socket.send(JSON.stringify(numbered(event, sequence)));
      operatorFocusSequence = sequence;
      useShellStore.getState().noteOperatorFocusSent(sequence);
      return true;
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
    // Whether this socket has drawn the core's state: a move's screen sends
    // only the move, and a node's held screen only where its link stands.
    let drawn = false;
    ws.addEventListener("open", () => {
      useShellStore.getState().releaseOperatorFocus();
      ws.send(
        JSON.stringify({
          token,
          schema_version: 2,
          client_kind: hostKind() === "electron" ? "desktop" : "web",
          have_revision: revision,
          have_core: revisionCore ?? undefined,
          // A client with no cursor names none and has its kept terminals
          // drawn again; a cursor resumes only on the hub its epoch names.
          have_terminal_sequence: terminalSequence ?? undefined,
          have_terminal_epoch: terminalEpoch ?? undefined,
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
        payload?: { revision?: number; terminal_sequence?: number; terminal_epoch?: string };
      };
      // The daemon describes itself before the first snapshot. It is not
      // state: the connection turns live only with the snapshot, because
      // everything keyed to "live" (buffer reconciliation, terminal views)
      // reads the snapshot that has not arrived yet.
      if (frame.type === "daemon") {
        announcedCore = (frame.payload as { core_instance?: string } | undefined)?.core_instance ?? null;
      }
      if (frame.type === "daemon" || frame.type === "mobile" || frame.type === "core_link") {
        useShellStore.getState().applyFrame(frame);
        return;
      }
      if (frame.type === "core_move") {
        useShellStore.getState().applyFrame(frame);
        const view = frame.payload as unknown as MoveView;
        const address = addressAfterMove(window.location.hash, view);
        if (address !== null) history.replaceState(history.state, "", address);
        if (!drawn) useShellStore.getState().setConnection("moving");
        return;
      }
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
      if (frame.type === "snapshot" || frame.type === "delta") revisionCore = announcedCore;
      terminalSequence = useShellStore.getState().terminalSequence;
      terminalEpoch = useShellStore.getState().terminalEpoch;
      handlers.onChunks(chunks, frame.type === "snapshot");
      if (frame.type === "snapshot" || frame.type === "delta") drawn = true;
      // A refusal before anything was drawn (a move's screen refusing an
      // event) leaves the window where it is.
      if (frame.type === "error" && !drawn) return;
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
