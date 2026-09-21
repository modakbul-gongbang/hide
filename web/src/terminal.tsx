import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useEffect, useRef } from "react";
import { mapModifiedKey } from "./keys";
import { useShellStore, type TerminalChunk } from "./store";
import type { DispatchFn } from "./ws";

const writers = new Map<string, (data: Uint8Array) => void>();
const pending = new Map<string, Uint8Array[]>();
let termRefForReset: Terminal | null = null;

function decodeChunk(chunk: TerminalChunk): Uint8Array | null {
  if (!chunk.bytes_base64) return null;
  const binary = atob(chunk.bytes_base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

export function resetTerminal() {
  termRefForReset?.reset();
  pending.clear();
}

export function feedChunks(chunks: TerminalChunk[]) {
  for (const chunk of chunks) {
    const bytes = decodeChunk(chunk);
    if (!bytes) continue;
    const write = writers.get(chunk.pane_id);
    if (write) {
      write(bytes);
      continue;
    }
    const queued = pending.get(chunk.pane_id) ?? [];
    queued.push(bytes);
    pending.set(chunk.pane_id, queued);
  }
}

function bytesBase64(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

export function TerminalPane({ dispatch }: { dispatch: DispatchFn }) {
  const hostRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const paneId = useShellStore((s) => s.focusedPaneId);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const term = new Terminal({
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, monospace",
      fontSize: 14,
      theme: {
        background: "var(--color-background)",
        foreground: "var(--color-primary)",
      },
      allowProposedApi: true,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);
    try {
      term.loadAddon(new WebglAddon());
    } catch {
      /* canvas renderer remains */
    }
    termRef.current = term;
    termRefForReset = term;
    const sendGrid = (newView: boolean) => {
      fit.fit();
      const id = useShellStore.getState().focusedPaneId;
      if (!id || term.cols < 2 || term.rows < 2) return;
      dispatch({
        schema_version: 2,
        kind: "terminal_viewport",
        payload: { pane_id: id, cols: term.cols, rows: term.rows, new_view: newView },
      });
      dispatch({
        schema_version: 2,
        kind: "terminal_resize",
        payload: { pane_id: id, cols: term.cols, rows: term.rows },
      });
    };
    const sendResize = () => sendGrid(false);
    const observer = new ResizeObserver(() => sendResize());
    observer.observe(host);
    sendResize();
    term.attachCustomKeyEventHandler((event) => {
      if (event.type !== "keydown") return true;
      const mapped = mapModifiedKey(event);
      if (!mapped) return true;
      event.preventDefault();
      const id = useShellStore.getState().focusedPaneId;
      if (id) {
        dispatch({
          schema_version: 2,
          kind: "key",
          payload: { pane_id: id, bytes_base64: bytesBase64(mapped) },
        });
      }
      return false;
    });
    term.onData((data) => {
      const id = useShellStore.getState().focusedPaneId;
      if (!id) return;
      const bytes = new TextEncoder().encode(data);
      dispatch({
        schema_version: 2,
        kind: "key",
        payload: { pane_id: id, bytes_base64: bytesBase64(bytes) },
      });
    });
    return () => {
      observer.disconnect();
      writers.clear();
      term.dispose();
      termRef.current = null;
    };
  }, [dispatch]);

  useEffect(() => {
    const term = termRef.current;
    if (!term || !paneId) return;
    term.reset();
    writers.clear();
    writers.set(paneId, (data) => term.write(data));
    const queued = pending.get(paneId) ?? [];
    pending.delete(paneId);
    for (const bytes of queued) term.write(bytes);
    dispatch({
      schema_version: 2,
      kind: "terminal_viewport",
      payload: { pane_id: paneId, cols: term.cols, rows: term.rows, new_view: true },
    });
    dispatch({
      schema_version: 2,
      kind: "terminal_resize",
      payload: { pane_id: paneId, cols: term.cols, rows: term.rows },
    });
  }, [paneId, dispatch]);

  return <div ref={hostRef} className="h-full min-w-0 flex-1 bg-background" data-terminal-pane={paneId ?? ""} />;
}
