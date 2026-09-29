// The start panel's own page state (PRD home-device-rail D-17, B30): whether
// it is open, the text the operator wrote, the target they picked while it is
// open, and the request in flight. The text outlives a close, so Esc keeps
// the draft until the next ⌘N and a start clears it; an agent that then fails
// to start puts the text it was given back (B31). The target is not kept:
// every open reads what is in front again (D-19).

import { create } from "zustand";

type StartPanelState = {
  isOpen: boolean;
  text: string;
  /** The target picked in this open; null follows what is in front. */
  target: string | null;
  /** The id of the request whose answer the panel waits for. */
  request: string | null;
  /** Why the last start did not go, shown inside the panel. */
  failure: string | null;
  /** The text the request in flight carries. */
  sent: string;
  /** A start whose tab is open but whose agent has not answered yet, with its text. */
  spent: { taskId: number; text: string } | null;
  open: () => void;
  close: () => void;
  setText: (text: string) => void;
  setTarget: (key: string) => void;
  begin: (requestId: string) => void;
  fail: (message: string) => void;
  /** The start went: the text is spent and the panel is done; `taskId` names an agent still starting. */
  finish: (taskId: number | null) => void;
  /** The spent start's agent did not start: its text comes back unless a new draft took its place. */
  restore: (message: string) => void;
  /** The spent start's agent answered; its text is no longer kept. */
  settle: () => void;
};

export const useStartPanel = create<StartPanelState>((set) => ({
  isOpen: false,
  text: "",
  target: null,
  request: null,
  failure: null,
  sent: "",
  spent: null,
  // A reason the last start did not go stays with its text until the text changes.
  open: () => set({ isOpen: true, target: null }),
  close: () => set({ isOpen: false }),
  setText: (text) => set({ text, failure: null }),
  setTarget: (target) => set({ target }),
  begin: (request) => set((s) => ({ request, failure: null, sent: s.text })),
  fail: (failure) => set({ request: null, failure }),
  finish: (taskId) => set((s) => ({ isOpen: false, text: "", request: null, failure: null, spent: taskId === null ? null : { taskId, text: s.sent } })),
  restore: (failure) => set((s) => (s.spent ? { text: s.text === "" ? s.spent.text : s.text, failure, spent: null } : {})),
  settle: () => set({ spent: null }),
}));
