// The one rule behind `reveal_external` (issue 324), the item the
// Explorer, History, View tab and sidebar menus share: it selects a file or
// folder in its parent folder in the OS file manager through the desktop
// host, and opens nothing. The label is the host's OS's own name for that
// act; a plain browser tab has no file manager to hand anything to, so it
// offers no such item, and a path on another device is not on this computer.

/** What a menu reads from the host for the item: its label, or null where the host has no file manager. */
export type RevealHost = { label: string } | null;

/** The item's label on the desktop host's OS (`process.platform`). */
export function revealLabel(platform: string | null | undefined): string {
  if (platform === "darwin") return "Reveal in Finder";
  if (platform === "win32") return "Reveal in File Explorer";
  if (platform === "linux") return "Open Containing Folder";
  return "Show in File Manager";
}

/** The item as every menu draws it (`MenuEntry` in `components/entry-menu.tsx`, kept JSX-free for the desktop host's imports). */
export type RevealExternalEntry = { id: "reveal_external"; label: string; unavailable: string | null; separated?: boolean };

export const REVEAL_HERE_ONLY = "Only for files and folders on this computer.";

/**
 * The item for a target on `device`, or none in a plain browser tab. A
 * target on another device, or one already known to be gone (`blocked`),
 * is listed disabled with its reason rather than hidden.
 */
export function revealExternalEntry(host: RevealHost, device: string, blocked: string | null = null, separated = false): RevealExternalEntry[] {
  if (!host) return [];
  const unavailable = device !== "local" ? REVEAL_HERE_ONLY : blocked;
  return [{ id: "reveal_external", label: host.label, unavailable, ...(separated ? { separated } : {}) }];
}
