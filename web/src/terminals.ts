// One xterm.js instance per pane of the visible tab, keyed by pane id.
//
// Panes of other tabs have no instance (PRD S2 D-05): a chunk for a pane
// without one is dropped, not queued, because mounting requests a full frame
// (`terminal_viewport` with `new_view`) and nothing older is worth replaying.
// The core attaches the visible tab's panes on its own tick, so a mount is
// only ever a request for the view, never for the session.

import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { mapModifiedKey } from "./keys";
import { noteWriteComplete, probeEnabled } from "./probe";
import { useShellStore, type TerminalChunk } from "./store";
import type { DispatchFn } from "./ws";

type Instance = {
  term: Terminal;
  fit: FitAddon;
  dispatch: DispatchFn;
  scale: number;
};

const instances = new Map<string, Instance>();

function token(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

function tokenPx(name: string): number {
  const value = Number.parseFloat(token(name));
  if (!Number.isFinite(value)) throw new Error(`token ${name} is not a length`);
  return value;
}

function decodeChunk(chunk: TerminalChunk): Uint8Array | null {
  if (!chunk.bytes_base64) return null;
  const binary = atob(chunk.bytes_base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

function bytesBase64(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

export function feedChunks(chunks: TerminalChunk[]) {
  for (const chunk of chunks) {
    const instance = instances.get(chunk.pane_id);
    if (!instance) continue;
    const bytes = decodeChunk(chunk);
    if (!bytes) continue;
    if (probeEnabled()) {
      const term = instance.term;
      term.write(bytes, () => noteWriteComplete(term));
    } else {
      instance.term.write(bytes);
    }
  }
}

/** A self-contained snapshot may follow a core restart; every pane redraws from its next full frame. */
export function resetAllTerminals() {
  for (const instance of instances.values()) instance.term.reset();
}

export function terminalFor(paneId: string | null): Terminal | null {
  return paneId ? (instances.get(paneId)?.term ?? null) : null;
}

export function focusTerminal(paneId: string) {
  instances.get(paneId)?.term.focus();
}

function sendGrid(paneId: string, instance: Instance, newView: boolean) {
  instance.fit.fit();
  const { term } = instance;
  if (term.cols < 2 || term.rows < 2) return;
  instance.dispatch({
    schema_version: 2,
    kind: "terminal_viewport",
    payload: { pane_id: paneId, cols: term.cols, rows: term.rows, new_view: newView },
  });
  instance.dispatch({
    schema_version: 2,
    kind: "terminal_resize",
    payload: { pane_id: paneId, cols: term.cols, rows: term.rows },
  });
}

/** Asks the core for a full frame of the pane; used after mount and after every self-contained snapshot. */
export function requestView(paneId: string) {
  const instance = instances.get(paneId);
  if (instance) sendGrid(paneId, instance, true);
}

/** Applies the core's per-pane text scale; the new fit goes out as `terminal_resize` (PRD S2 B13). */
export function setTextScale(paneId: string, scale: number) {
  const instance = instances.get(paneId);
  if (!instance || instance.scale === scale) return;
  instance.scale = scale;
  instance.term.options.fontSize = Math.round(tokenPx("--text-terminal-base") * scale);
  sendGrid(paneId, instance, false);
}

/**
 * Creates the instance for `paneId` inside `host` and returns its disposer.
 * The host's size is watched, so a grid change from Herdr's new geometry
 * reaches the core as one `terminal_resize` per pane (PRD S2 B6).
 */
export function mountTerminal(
  paneId: string,
  host: HTMLElement,
  dispatch: DispatchFn,
  scale: number,
): () => void {
  if (instances.has(paneId)) throw new Error(`pane ${paneId} already has a terminal`);
  const term = new Terminal({
    fontFamily: token("--font-mono"),
    fontSize: Math.round(tokenPx("--text-terminal-base") * scale),
    theme: {
      background: token("--color-background"),
      foreground: token("--color-primary"),
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
  const instance: Instance = { term, fit, dispatch, scale };
  instances.set(paneId, instance);
  const send = (bytes: Uint8Array) =>
    dispatch({
      schema_version: 2,
      kind: "key",
      payload: { pane_id: paneId, bytes_base64: bytesBase64(bytes) },
    });
  term.attachCustomKeyEventHandler((event) => {
    if (event.type !== "keydown") return true;
    const mapped = mapModifiedKey(event);
    if (!mapped) return true;
    event.preventDefault();
    send(mapped);
    return false;
  });
  term.onData((data) => send(new TextEncoder().encode(data)));
  // Clicking into a pane is the operator moving keyboard focus; the core
  // owns the focus pane, so the click is an event and the header follows
  // the snapshot, not the click.
  const onFocus = () => {
    if (useShellStore.getState().focusedPaneId === paneId) return;
    dispatch({ schema_version: 2, kind: "focus_pane", payload: { pane_id: paneId, origin: "operator" } });
  };
  term.textarea?.addEventListener("focus", onFocus);
  const observer = new ResizeObserver(() => sendGrid(paneId, instance, false));
  observer.observe(host);
  sendGrid(paneId, instance, true);
  return () => {
    observer.disconnect();
    term.textarea?.removeEventListener("focus", onFocus);
    instances.delete(paneId);
    term.dispose();
  };
}
