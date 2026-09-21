// The measurement and e2e seam over the terminal pane.
//
// xterm.js draws through WebGL, so neither Playwright nor a CDP driver can
// read the screen from the DOM. When the page URL carries `probe=1` this
// installs `window.__hideProbe`, which reads the parsed buffer and resolves a
// promise at the xterm write completion that first shows an armed marker
// (the S0 echo definition: t1 = write callback with the marker in the parsed
// buffer). Without the query nothing is installed and the writer path is the
// plain `term.write(data)`.

import type { Terminal } from "@xterm/xterm";

export type EchoSample = { arrival_ms: number; write_ms: number };

export type Probe = {
  paneId: () => string | null;
  screenText: () => string;
  /** Arms one marker; `waitArmed` resolves at the first write completion whose screen contains it. */
  arm: (marker: string, timeoutMs?: number) => void;
  waitArmed: () => Promise<EchoSample>;
  /** WebSocket frames received since the page loaded. */
  arrivals: () => number;
  /** Closes the live socket the way a server drop would; the shell reconnects on its own. */
  dropSocket: () => void;
  /** Pane ids whose terminal session the core reports as not released (the attach window). */
  attachedPanes: () => string[];
  /** Pane ids that hold an xterm instance right now, shown or parked (D-05). */
  liveTerminals: () => string[];
  /** The parsed buffer of any pane's instance, empty when it has none. */
  paneText: (paneId: string) => string;
};

declare global {
  interface Window {
    __hideProbe?: Probe;
  }
}

export const epochMs = () => performance.timeOrigin + performance.now();

let armed: {
  marker: string;
  resolve: (sample: EchoSample) => void;
  timer: ReturnType<typeof setTimeout>;
} | null = null;
let armedPromise: Promise<EchoSample> | null = null;
let lastArrivalMs = 0;
let arrivals = 0;

export function probeEnabled(search: string = window.location.search): boolean {
  return new URLSearchParams(search).get("probe") === "1";
}

export function screenText(term: Terminal): string {
  const buffer = term.buffer.active;
  const lines: string[] = [];
  for (let i = 0; i < buffer.length; i += 1) {
    lines.push(buffer.getLine(i)?.translateToString(true) ?? "");
  }
  return lines.join("\n");
}

/** Called at WebSocket message entry, before JSON parsing. */
export function noteArrival(): void {
  lastArrivalMs = epochMs();
  arrivals += 1;
}

/** Called from the terminal writer's write-completion callback. */
export function noteWriteComplete(term: Terminal): void {
  if (!armed) return;
  const write_ms = epochMs();
  if (!screenText(term).includes(armed.marker)) return;
  clearTimeout(armed.timer);
  armed.resolve({ arrival_ms: lastArrivalMs, write_ms });
  armed = null;
}

export function installProbe(
  term: () => Terminal | null,
  paneId: () => string | null,
  dropSocket: () => void = () => {},
  attachedPanes: () => string[] = () => [],
  liveTerminals: () => string[] = () => [],
  terminalOf: (paneId: string) => Terminal | null = () => null,
): void {
  window.__hideProbe = {
    paneId,
    dropSocket,
    attachedPanes,
    liveTerminals,
    paneText: (id) => {
      const current = terminalOf(id);
      return current ? screenText(current) : "";
    },
    screenText: () => {
      const current = term();
      return current ? screenText(current) : "";
    },
    arm: (marker, timeoutMs = 3000) => {
      if (armed) throw new Error("echo sample already armed");
      armedPromise = new Promise<EchoSample>((resolve, reject) => {
        const timer = setTimeout(() => {
          armed = null;
          reject(new Error(`echo timeout: ${marker}`));
        }, timeoutMs);
        armed = { marker, resolve, timer };
      });
      // A timeout is still retrieved through waitArmed when the driver is late.
      void armedPromise.catch(() => {});
    },
    waitArmed: () => {
      if (!armedPromise) return Promise.reject(new Error("echo sample not armed"));
      return armedPromise;
    },
    arrivals: () => arrivals,
  };
}
