// Which host runs the shell: a browser tab, or the desktop app
// (`desktop/`), whose preload exposes `window.hideHost` and nothing else.
// The bridge carries the app menu's commands in; the shell never reaches
// the host any other way (desktop PRD B11).

export type HostKind = "browser" | "electron";

export type HostBridge = {
  kind: "electron";
  /** Delivers each app-menu command id; returns the unsubscribe. */
  onCommand(listener: (id: string) => void): () => void;
};

declare global {
  interface Window {
    hideHost?: HostBridge;
  }
}

export function hostBridge(): HostBridge | null {
  return typeof window !== "undefined" && window.hideHost?.kind === "electron" ? window.hideHost : null;
}

export function hostKind(): HostKind {
  return hostBridge() ? "electron" : "browser";
}
