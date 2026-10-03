import type { Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const MAX_RECORDS = 1024;
const MAX_MARKERS = 2;
const MAX_INPUT_BYTES = 64 * 1024;
const MAX_JSON_BYTES = 2 * 1024 * 1024;

type RendererObservation = {
  records: unknown[];
  droppedRecords: number;
  inputBytes: number;
  redactedInputs: number;
  incomplete: string[];
};

export type TerminalInputObservation = {
  markTarget: (paneId: string) => void;
  exportOnce: () => Promise<void>;
};

/** Test-only observation of one private renderer; it grants no input authority. */
export async function observeTerminalInput(page: Page, panes: string[], candidatePid: number | undefined): Promise<TerminalInputObservation> {
  const markers: { atMs: number; paneId: string }[] = [];
  const nodeIncomplete: string[] = [];
  const startedAtMs = Date.now();
  const paneIds = panes.slice(0, 3);
  let exported = false;
  if (candidatePid === undefined) nodeIncomplete.push("candidate-pid-unavailable");

  try {
    await page.evaluate(({ paneIds, maxRecords, maxInputBytes }) => {
      const records: unknown[] = [];
      const incomplete = new Set<string>();
      const noteIncomplete = (reason: string) => {
        try { incomplete.add(reason); } catch { /* Observation must not affect input. */ }
      };
      let droppedRecords = 0;
      let inputBytes = 0;
      let redactedInputs = 0;
      const increment = (value: number) => Math.min(Number.MAX_SAFE_INTEGER, value + 1);
      const owner = (target: EventTarget | null) => {
        const element = target instanceof Element ? target : null;
        const view = element?.closest("[data-pane-view]");
        const paneId = view?.getAttribute("data-pane-view") ?? null;
        return {
          tag: element?.tagName.slice(0, 16) ?? null,
          paneId: paneId !== null && paneIds.includes(paneId) ? paneId : null,
          focused: view?.getAttribute("data-focused") === "true",
          transport: (view?.getAttribute("data-transport") ?? "").slice(0, 32),
        };
      };
      const paneStates = () => paneIds.map((paneId) => owner(document.querySelector('[data-pane-view="' + CSS.escape(paneId) + '"]')));
      const append = (record: Record<string, unknown>) => {
        if (records.length >= maxRecords) {
          droppedRecords = increment(droppedRecords);
          noteIncomplete("record-cap");
          return;
        }
        records.push({ index: records.length, atMs: Date.now(), monotonicMs: performance.now(), ...record });
      };
      // Only the original fixture command's ASCII alphabet and Enter can be saved.
      const alphabet = new Set("printf 'desktop-group-live\\n\r");
      const syntheticText = (value: string | null) => {
        if (value === null) return null;
        if (value.length > maxInputBytes - inputBytes) {
          noteIncomplete("input-cap");
          return null;
        }
        for (const character of value) {
          if (!alphabet.has(character)) {
            redactedInputs = increment(redactedInputs);
            noteIncomplete("non-synthetic-input");
            return null;
          }
        }
        inputBytes += value.length;
        return value;
      };
      const onEvent = (event: Event) => {
        try {
          const active = owner(document.activeElement);
          const target = owner(event.target);
          const record: Record<string, unknown> = { type: event.type, active, target };
          if (event instanceof KeyboardEvent) {
            record.code = event.code.slice(0, 32);
            record.key = ["Enter", "Meta", "Control", "Alt", "Shift", "Tab", "Escape"].includes(event.key)
              ? event.key : active.paneId !== null || target.paneId !== null ? syntheticText(event.key) : null;
            record.modifiers = { alt: event.altKey, ctrl: event.ctrlKey, meta: event.metaKey, shift: event.shiftKey };
            record.repeat = event.repeat;
          } else if (event instanceof InputEvent) {
            record.inputType = event.inputType.slice(0, 32);
            record.data = target.paneId === null ? null : syntheticText(event.data);
            record.composing = event.isComposing;
          }
          append(record);
        } catch {
          noteIncomplete("dom-observation-error");
        }
      };
      const subject = window as Window & { __hideTerminalInputObservation?: () => RendererObservation };
      if (subject.__hideTerminalInputObservation) throw new Error("observation already installed");
      const eventTypes = ["keydown", "keypress", "keyup", "beforeinput", "input", "focusin", "focusout", "click"];
      for (const type of eventTypes) document.addEventListener(type, onEvent, { capture: true, passive: true });

      const prototype = WebSocket.prototype;
      const descriptor = Object.getOwnPropertyDescriptor(prototype, "send");
      const original = prototype.send;
      const observeSend = (socket: WebSocket, args: Parameters<WebSocket["send"]>, outcome: string) => {
        try {
          const data = args[0];
          if (typeof data !== "string") return;
          if (data.length > maxInputBytes * 2) {
            noteIncomplete("ws-frame-cap");
            return;
          }
          const event = JSON.parse(data) as { schema?: unknown; kind?: unknown; payload?: { paneId?: unknown; base64?: unknown } };
          if (event.schema !== 2 || event.kind !== "key" || typeof event.payload?.paneId !== "string" || !paneIds.includes(event.payload.paneId)) return;
          const encoded = event.payload.base64;
          if (typeof encoded !== "string") {
            noteIncomplete("invalid-key-payload");
            return;
          }
          if (encoded.length > Math.ceil(maxInputBytes / 3) * 4) {
            noteIncomplete("input-cap");
            return;
          }
          const bytes = atob(encoded);
          append({ type: "ws-key", paneId: event.payload.paneId, byteLength: bytes.length,
            bytes: syntheticText(bytes), readyState: socket.readyState, outcome, active: owner(document.activeElement) });
        } catch {
          noteIncomplete("ws-observation-error");
        }
      };
      const observedSend = function (this: WebSocket, ...args: Parameters<WebSocket["send"]>): ReturnType<WebSocket["send"]> {
        let result: ReturnType<WebSocket["send"]>;
        try {
          result = Reflect.apply(original, this, args);
        } catch (error) {
          observeSend(this, args, "threw");
          throw error;
        }
        observeSend(this, args, "returned");
        return result;
      };
      let wrapped = false;
      try {
        if (!descriptor || typeof descriptor.value !== "function") throw new Error("send descriptor unavailable");
        Object.defineProperty(prototype, "send", { ...descriptor, value: observedSend });
        wrapped = true;
      } catch {
        noteIncomplete("ws-install-error");
      }

      const finish = () => {
        append({ type: "finished", active: owner(document.activeElement), panes: paneStates() });
        for (const type of eventTypes) document.removeEventListener(type, onEvent, true);
        if (wrapped) {
          // A later wrapper belongs to its writer; never overwrite it at teardown.
          if (prototype.send === observedSend && descriptor) Object.defineProperty(prototype, "send", descriptor);
          else noteIncomplete("ws-wrapper-replaced");
        }
        return { records, droppedRecords, inputBytes, redactedInputs, incomplete: [...incomplete] };
      };
      subject.__hideTerminalInputObservation = () => {
        const result = finish();
        delete subject.__hideTerminalInputObservation;
        return result;
      };
      append({ type: "installed", active: owner(document.activeElement), panes: paneStates() });
    }, { paneIds, maxRecords: MAX_RECORDS - MAX_MARKERS, maxInputBytes: MAX_INPUT_BYTES });
  } catch {
    nodeIncomplete.push("renderer-install-error");
  }
  const installedAtMs = Date.now();

  return {
    // Node metadata only: no extra evaluate, focus, readiness wait, or input action.
    markTarget(paneId) {
      if (markers.length < MAX_MARKERS && paneIds.includes(paneId)) markers.push({ atMs: Date.now(), paneId });
      else if (!nodeIncomplete.includes("marker-cap")) nodeIncomplete.push("marker-cap");
    },
    async exportOnce() {
      if (exported) return;
      exported = true;
      let renderer: RendererObservation | null = null;
      try {
        renderer = await page.evaluate(() => {
          const subject = window as Window & { __hideTerminalInputObservation?: () => RendererObservation };
          return subject.__hideTerminalInputObservation?.() ?? null;
        });
        if (renderer === null) nodeIncomplete.push("renderer-unavailable");
      } catch {
        nodeIncomplete.push("renderer-export-error");
      }
      const metadata = { schema: 1, candidatePid: candidatePid ?? null, paneIds, startedAtMs, installedAtMs,
        exportedAtMs: Date.now(), markers, incomplete: nodeIncomplete,
        caps: { records: MAX_RECORDS, inputBytes: MAX_INPUT_BYTES, jsonBytes: MAX_JSON_BYTES } };
      let json = JSON.stringify({ ...metadata, renderer });
      if (Buffer.byteLength(json, "utf8") > MAX_JSON_BYTES) {
        json = JSON.stringify({ ...metadata, incomplete: [...nodeIncomplete, "json-cap"],
          omittedRendererRecords: renderer?.records.length ?? 0 });
      }
      try {
        const directory = fileURLToPath(new URL("../../agents/runs/terminal-input-diagnosis/", import.meta.url));
        fs.mkdirSync(directory, { recursive: true });
        fs.writeFileSync(path.join(directory, "desktop-terminal-input-" + startedAtMs + "-" + process.pid + ".json"), json,
          { encoding: "utf8", flag: "wx", mode: 0o600 });
      } catch {
        // Fixed metadata only; export failure neither masks the test nor skips cleanup.
        console.warn("Terminal input observation incomplete: artifact export unavailable");
      }
    },
  };
}
