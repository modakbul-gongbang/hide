// The start panel's own page state (PRD home-device-rail D-17, B30): whether
// it is open, the text the operator wrote, the target they picked while it is
// open, and the request in flight. The text outlives a close, so Esc keeps
// the draft until the next ⌘N and a start clears it. The target is not kept:
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
  open: () => void;
  close: () => void;
  setText: (text: string) => void;
  setTarget: (key: string) => void;
  begin: (requestId: string) => void;
  fail: (message: string) => void;
  /** The start went: the text is spent and the panel is done. */
  finish: () => void;
};

export const useStartPanel = create<StartPanelState>((set) => ({
  isOpen: false,
  text: "",
  target: null,
  request: null,
  failure: null,
  open: () => set({ isOpen: true, target: null, failure: null }),
  close: () => set({ isOpen: false }),
  setText: (text) => set({ text }),
  setTarget: (target) => set({ target }),
  begin: (request) => set({ request, failure: null }),
  fail: (failure) => set({ request: null, failure }),
  finish: () => set({ isOpen: false, text: "", request: null, failure: null }),
}));
