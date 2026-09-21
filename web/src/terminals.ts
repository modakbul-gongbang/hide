// One xterm.js instance per attached pane, keyed by pane id.
//
// An instance outlives the tab it is drawn in (PRD S2 D-05, amendment 1):
// the core keeps the panes of the last `ATTACHED_TAB_LIMIT` tabs attached and
// streams their output, so their instances stay alive, fed, and parked out of
// view while another tab is shown. Coming back to a tab shows the last frame
// at once; nothing is re-requested unless the size changed. An instance is
// disposed only when the core reports the pane released or no longer lists
// it (`retainTerminals`), never on a tab switch.
//
// The wheel is Herdr's too (PRD S2 B19): the pane has no local scrollback,
// so a wheel batch becomes one `terminal_scroll` per animation frame and the
// core answers with the viewport it now shows.

import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { mapModifiedKey } from "./keys";
import { noteWriteComplete, probeEnabled } from "./probe";
import { wheelModifiers, wheelRows } from "./wheel";
import { useShellStore, type TerminalChunk } from "./store";
import type { DispatchFn } from "./ws";

type Instance = {
  term: Terminal;
  fit: FitAddon;
  dispatch: DispatchFn;
  scale: number;
  /** The element the terminal was opened in; it moves between a host and the parking lot. */
  element: HTMLDivElement;
  /** The pane host it is shown in, or null while parked. */
  host: HTMLElement | null;
  observer: ResizeObserver | null;
  /** A self-contained snapshot arrived while parked; the next show asks for a full frame. */
  stale: boolean;
  /** Wheel rows accumulated toward the next flush, and the pending flush. */
  wheel: { rows: number; remainder: number; column: number; row: number; modifiers: number; frame: number | null };
  disposeHandlers: () => void;
};

const instances = new Map<string, Instance>();

let parking: HTMLDivElement | null = null;

/** A hidden lot in the document, so a parked terminal keeps its canvas and layout state. */
function parkingLot(): HTMLDivElement {
  if (!parking) {
    parking = document.createElement("div");
    parking.hidden = true;
    parking.dataset.terminalParking = "true";
    document.body.append(parking);
  }
  return parking;
}

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
  for (const instance of instances.values()) {
    instance.term.reset();
    if (!instance.host) instance.stale = true;
  }
}

export function terminalFor(paneId: string | null): Terminal | null {
  return paneId ? (instances.get(paneId)?.term ?? null) : null;
}

export function focusTerminal(paneId: string) {
  instances.get(paneId)?.term.focus();
}

/** Pane ids that currently hold an instance, shown or parked (measurement and e2e seam). */
export function liveTerminalIds(): string[] {
  return [...instances.keys()];
}

function sendGrid(paneId: string, instance: Instance, newView: boolean) {
  if (!instance.host) return;
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

/** Asks the core for a full frame of the pane; used after every self-contained snapshot. */
export function requestView(paneId: string) {
  const instance = instances.get(paneId);
  if (!instance) return;
  instance.stale = false;
  sendGrid(paneId, instance, true);
}

/** Applies the core's per-pane text scale; the new fit goes out as `terminal_resize` (PRD S2 B13). */
export function setTextScale(paneId: string, scale: number) {
  const instance = instances.get(paneId);
  if (!instance || instance.scale === scale) return;
  instance.scale = scale;
  instance.term.options.fontSize = Math.round(tokenPx("--text-terminal-base") * scale);
  sendGrid(paneId, instance, false);
}

function flushWheel(paneId: string, instance: Instance) {
  const wheel = instance.wheel;
  wheel.frame = null;
  const rows = wheel.rows;
  wheel.rows = 0;
  if (rows === 0) return;
  instance.dispatch({
    schema_version: 2,
    kind: "terminal_scroll",
    payload: {
      pane_id: paneId,
      // Wheel deltas grow downward; Herdr's `up` shows older lines.
      direction: rows > 0 ? "down" : "up",
      lines: Math.abs(rows),
      column: wheel.column,
      row: wheel.row,
      modifiers: wheel.modifiers,
    },
  });
}

function onWheel(paneId: string, instance: Instance, event: WheelEvent) {
  // ⌥ + wheel stays with the browser, as in the Swift shell.
  if (event.altKey) return;
  event.preventDefault();
  const host = instance.host;
  if (!host) return;
  const { term } = instance;
  const rect = host.getBoundingClientRect();
  const rowHeight = term.rows > 0 ? rect.height / term.rows : 0;
  const colWidth = term.cols > 0 ? rect.width / term.cols : 0;
  const wheel = instance.wheel;
  const moved = wheelRows(event.deltaY, event.deltaMode, rowHeight, wheel.remainder);
  wheel.remainder = moved.remainder;
  wheel.rows += moved.rows;
  wheel.column = colWidth > 0 ? Math.max(0, Math.min(term.cols - 1, Math.floor((event.clientX - rect.left) / colWidth))) : 0;
  wheel.row = rowHeight > 0 ? Math.max(0, Math.min(term.rows - 1, Math.floor((event.clientY - rect.top) / rowHeight))) : 0;
  wheel.modifiers = wheelModifiers(event);
  if (wheel.frame === null) wheel.frame = requestAnimationFrame(() => flushWheel(paneId, instance));
}

function createInstance(paneId: string, dispatch: DispatchFn, scale: number): Instance {
  const element = document.createElement("div");
  element.className = "absolute inset-0";
  element.dataset.terminal = paneId;
  const term = new Terminal({
    fontFamily: token("--font-mono"),
    fontSize: Math.round(tokenPx("--text-terminal-base") * scale),
    theme: {
      background: token("--color-background"),
      foreground: token("--color-primary"),
    },
    // Herdr owns the history; the pane shows the viewport the core sends.
    scrollback: 0,
    allowProposedApi: true,
  });
  const fit = new FitAddon();
  term.loadAddon(fit);
  const instance: Instance = {
    term,
    fit,
    dispatch,
    scale,
    element,
    host: null,
    observer: null,
    stale: false,
    wheel: { rows: 0, remainder: 0, column: 0, row: 0, modifiers: 0, frame: null },
    disposeHandlers: () => {},
  };
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
  const wheel = (event: WheelEvent) => onWheel(paneId, instance, event);
  element.addEventListener("wheel", wheel, { capture: true, passive: false });
  instance.disposeHandlers = () => {
    element.removeEventListener("wheel", wheel, { capture: true });
    if (instance.wheel.frame !== null) cancelAnimationFrame(instance.wheel.frame);
  };
  return instance;
}

/**
 * Shows the pane's terminal inside `host`, creating it on first sight, and
 * returns the function that parks it again. The host's size is watched, so a
 * grid change from Herdr's new geometry reaches the core as one
 * `terminal_resize` per pane (PRD S2 B6).
 */
export function attachTerminal(
  paneId: string,
  host: HTMLElement,
  dispatch: DispatchFn,
  scale: number,
): () => void {
  let instance = instances.get(paneId);
  const fresh = !instance;
  if (!instance) {
    instance = createInstance(paneId, dispatch, scale);
    instances.set(paneId, instance);
  }
  if (instance.host) throw new Error(`pane ${paneId} is already shown`);
  const shown = instance;
  host.append(shown.element);
  shown.host = host;
  shown.dispatch = dispatch;
  if (fresh) {
    shown.term.open(shown.element);
    try {
      shown.term.loadAddon(new WebglAddon());
    } catch {
      /* canvas renderer remains */
    }
    // Clicking into a pane is the operator moving keyboard focus; the core
    // owns the focus pane, so the click is an event and the header follows
    // the snapshot, not the click.
    shown.term.textarea?.addEventListener("focus", () => {
      if (useShellStore.getState().focusedPaneId === paneId) return;
      shown.dispatch({ schema_version: 2, kind: "focus_pane", payload: { pane_id: paneId, origin: "operator" } });
    });
  }
  const observer = new ResizeObserver(() => sendGrid(paneId, shown, false));
  observer.observe(host);
  shown.observer = observer;
  const needsFrame = fresh || shown.stale;
  shown.stale = false;
  sendGrid(paneId, shown, needsFrame);
  if (!fresh) shown.term.refresh(0, shown.term.rows - 1);
  return () => {
    observer.disconnect();
    shown.observer = null;
    shown.host = null;
    parkingLot().append(shown.element);
  };
}

/**
 * Disposes every parked instance whose pane the core no longer streams:
 * released beyond the attach window, or gone with its tab. A shown pane is
 * left to its view, which parks it first.
 */
export function retainTerminals(attached: ReadonlySet<string>) {
  for (const [paneId, instance] of instances) {
    if (attached.has(paneId) || instance.host) continue;
    instance.disposeHandlers();
    instance.term.dispose();
    instance.element.remove();
    instances.delete(paneId);
  }
}
