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
//
// A pointer gesture follows the Swift `TerminalPointerRoutingState` (PRD S2
// B20): a drag selects locally, and a single primary click that never
// dragged is replayed on release as one `terminal_click` with the pressed
// cell; the core decides whether the program gets a mouse report. ⌥ + press
// and a multi-click stay local. A copy of the selection is assembled here
// (`selection.ts`), not by xterm.

import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { mapModifiedKey } from "./keys";
import { noteWriteComplete, probeEnabled } from "./probe";
import { selectionToText, type CellRow } from "./selection";
import { pointerModifiers, wheelRows } from "./wheel";
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
  /** The cell of a primary press that may still become a click, or null once it dragged or was not one. */
  press: { column: number; row: number } | null;
  disposeHandlers: () => void;
};

const instances = new Map<string, Instance>();

/** The most lines one `terminal_scroll` carries (the core reads a u16). */
const SCROLL_LINES_MAX = 65535;

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

/**
 * True while `focusTerminal` moves DOM focus to follow the snapshot, so the
 * textarea's focus listener does not report the core's own move back to it
 * as an operator action; that echo, met by Herdr's later confirmation of the
 * previous move, kept two panes trading focus on a slow runner.
 */
let followingSnapshot = false;

/** Moves keyboard focus to the pane the snapshot names; not an operator action. */
/** Whether the pane's terminal asked for bracketed paste, which the core
 * wraps the attachment token in. */
export function bracketedPaste(paneId: string): boolean {
  return terminalFor(paneId)?.modes.bracketedPasteMode ?? false;
}

export function focusTerminal(paneId: string) {
  const instance = instances.get(paneId);
  if (!instance) return;
  followingSnapshot = true;
  try {
    instance.term.focus();
  } finally {
    followingSnapshot = false;
  }
}

/**
 * Gives keyboard focus back after a sheet or dialog closes. A terminal gets it
 * through `focusTerminal` on the pane the core says is focused, never by
 * re-focusing the textarea that held it: that focus would be reported as the
 * operator moving to that pane, and undo a move the dialog itself caused (a
 * created worktree's pane). Any other element simply takes focus back.
 */
export function restoreFocus(previous: HTMLElement | null) {
  if (previous?.closest(".xterm")) {
    const paneId = useShellStore.getState().focusedPaneId;
    if (paneId) focusTerminal(paneId);
    return;
  }
  if (previous?.isConnected) previous.focus();
}

/** Pane ids that currently hold an instance, shown or parked (measurement and e2e seam). */
export function liveTerminalIds(): string[] {
  return [...instances.keys()];
}

/** Pane ids the core streams right now: listed in the terminal section and not released. */
export function attachedPaneIds(): string[] {
  return (useShellStore.getState().rest?.terminal?.panes ?? [])
    .filter((pane) => pane.transport_state !== "released")
    .map((pane) => pane.pane_id);
}

function disposeInstance(paneId: string, instance: Instance) {
  instance.disposeHandlers();
  instance.term.dispose();
  instance.element.remove();
  instances.delete(paneId);
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
      // A DOM deltaY grows downward while AppKit's scrollingDeltaY grows
      // upward, so this ternary is the mirror of the Swift one, not its
      // opposite; Herdr's `up` shows older lines. The core's line count is
      // a u16, so a scripted burst is clamped rather than refused.
      direction: rows > 0 ? "down" : "up",
      lines: Math.min(Math.abs(rows), SCROLL_LINES_MAX),
      column: wheel.column,
      row: wheel.row,
      modifiers: wheel.modifiers,
    },
  });
}

function onWheel(paneId: string, instance: Instance, event: WheelEvent) {
  // The wheel has one owner. Left to xterm, a buffer without scrollback
  // (ours always, and any alternate-screen program) turns the same event
  // into cursor-key bytes on the PTY, and a mouse-tracking program gets
  // xterm's own wheel report; Herdr decides both from terminal_scroll.
  event.stopImmediatePropagation();
  // ⌥ + wheel stays with the browser, as in the Swift shell: nothing to
  // Herdr and nothing to the PTY.
  if (event.altKey) return;
  event.preventDefault();
  const geometry = cellGeometry(instance);
  if (!geometry) return;
  const wheel = instance.wheel;
  const moved = wheelRows(event.deltaY, event.deltaMode, geometry.rowHeight, wheel.remainder);
  wheel.remainder = moved.remainder;
  wheel.rows += moved.rows;
  const cell = cellAt(geometry, event);
  wheel.column = cell.column;
  wheel.row = cell.row;
  wheel.modifiers = pointerModifiers(event);
  if (wheel.frame === null) wheel.frame = requestAnimationFrame(() => flushWheel(paneId, instance));
}

type CellGeometry = { rect: DOMRect; cols: number; rows: number; colWidth: number; rowHeight: number };

/**
 * The shown pane's cell grid, or null while parked. The grid is read from
 * xterm's own screen element, which is exactly `cols` by `rows` cells: the
 * host is larger by the fit's remainder, so a cell derived from the host
 * drifts by up to one row and one column toward the far edge, which is where
 * a one-row button in a full-screen program sits.
 */
function cellGeometry(instance: Instance): CellGeometry | null {
  if (!instance.host) return null;
  const { term } = instance;
  const screen = instance.element.querySelector(".xterm-screen");
  if (!screen) return null;
  const rect = screen.getBoundingClientRect();
  return {
    rect,
    cols: term.cols,
    rows: term.rows,
    colWidth: term.cols > 0 ? rect.width / term.cols : 0,
    rowHeight: term.rows > 0 ? rect.height / term.rows : 0,
  };
}

/** The zero-based cell under the pointer, clamped to the grid. */
function cellAt(geometry: CellGeometry, event: { clientX: number; clientY: number }): { column: number; row: number } {
  const { rect, cols, rows, colWidth, rowHeight } = geometry;
  return {
    column: colWidth > 0 ? Math.max(0, Math.min(cols - 1, Math.floor((event.clientX - rect.left) / colWidth))) : 0,
    row: rowHeight > 0 ? Math.max(0, Math.min(rows - 1, Math.floor((event.clientY - rect.top) / rowHeight))) : 0,
  };
}

function onMouseDown(instance: Instance, event: MouseEvent) {
  instance.press = null;
  // The primary button alone can become a click; ⌥ + press and a second
  // click of a multi-click are the Swift `application` and `localSelection`
  // routes, and neither is replayed to the program.
  if (event.button !== 0 || event.altKey || event.detail !== 1) return;
  const geometry = cellGeometry(instance);
  if (!geometry) return;
  instance.press = cellAt(geometry, event);
}

function onMouseUp(paneId: string, instance: Instance, event: MouseEvent) {
  const press = instance.press;
  instance.press = null;
  if (!press || event.button !== 0) return;
  const geometry = cellGeometry(instance);
  if (!geometry) return;
  const release = cellAt(geometry, event);
  // Leaving the pressed cell, or a selection xterm made on the way, is a drag.
  if (release.column !== press.column || release.row !== press.row || instance.term.hasSelection()) return;
  instance.dispatch({
    schema_version: 2,
    kind: "terminal_click",
    payload: { pane_id: paneId, column: press.column, row: press.row, modifiers: pointerModifiers(event) },
  });
}

/** The text the current drag selection copies, or null when nothing is selected. */
function selectedText(instance: Instance): string | null {
  const { term } = instance;
  const range = term.getSelectionPosition();
  if (!range) return null;
  const buffer = term.buffer.active;
  const rows: CellRow[] = [];
  for (let y = range.start.y; y <= range.end.y; y += 1) {
    const line = buffer.getLine(y);
    const cells: CellRow = [];
    for (let x = 0; x < term.cols; x += 1) {
      const cell = line?.getCell(x);
      cells.push(cell ? (cell.getWidth() === 0 ? null : cell.getChars()) : "");
    }
    rows.push(cells);
  }
  return selectionToText(rows, range.start.x, range.end.x);
}

/** The copy text of the pane's selection, for the probe; null without a selection or an instance. */
export function terminalSelectionText(paneId: string): string | null {
  const instance = instances.get(paneId);
  return instance ? selectedText(instance) : null;
}

function onCopy(instance: Instance, event: ClipboardEvent) {
  const text = selectedText(instance);
  if (text === null || !event.clipboardData) return;
  // One owner again: xterm's own copy handler would put its padded,
  // hard-broken rows on the clipboard from the same event.
  event.stopImmediatePropagation();
  event.preventDefault();
  event.clipboardData.setData("text/plain", text);
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
    press: null,
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
  const down = (event: MouseEvent) => onMouseDown(instance, event);
  const up = (event: MouseEvent) => onMouseUp(paneId, instance, event);
  const copy = (event: ClipboardEvent) => onCopy(instance, event);
  element.addEventListener("wheel", wheel, { capture: true, passive: false });
  // The press and release are read on the way down and left to xterm, whose
  // selection needs them. Its own mouse reports never fire: Herdr's frames
  // carry no mouse mode (`selection.ts` on what they carry), so the program
  // hears about a click only through the core's terminal_click route.
  element.addEventListener("mousedown", down, { capture: true });
  element.addEventListener("mouseup", up, { capture: true });
  element.addEventListener("copy", copy, { capture: true });
  instance.disposeHandlers = () => {
    element.removeEventListener("wheel", wheel, { capture: true });
    element.removeEventListener("mousedown", down, { capture: true });
    element.removeEventListener("mouseup", up, { capture: true });
    element.removeEventListener("copy", copy, { capture: true });
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
      if (followingSnapshot || useShellStore.getState().focusedPaneId === paneId) return;
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
    // A pane closed while shown left the core's list before its view
    // unmounted, so the park decides for itself instead of waiting for a
    // later change to sweep it.
    if (!attachedPaneIds().includes(paneId)) disposeInstance(paneId, shown);
  };
}

/**
 * Disposes every parked instance whose pane the core no longer streams:
 * released beyond the attach window, or gone with its tab. A shown pane is
 * skipped here; its park checks the same list when the view unmounts.
 */
export function retainTerminals() {
  const attached = new Set(attachedPaneIds());
  for (const [paneId, instance] of instances) {
    if (attached.has(paneId) || instance.host) continue;
    disposeInstance(paneId, instance);
  }
}
