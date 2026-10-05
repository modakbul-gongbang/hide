// The one rule behind `reveal_external` (issue 324), the item the
// Explorer, History, View tab and sidebar menus share: it selects a file or
// folder in its parent folder in the OS file manager through the desktop
// host, and opens nothing. The label is the host's OS's own name for that
// act; a plain browser tab has no file manager to hand anything to, so it
// offers no such item, and a path on another device is not on this computer.

// The desktop host imports this module (through `host.ts`), so it names the
// label by key and leaves the translating to the menu that draws it.
import type { TFunction } from "i18next";
import type { MessageKey } from "./i18n/catalogs";

export type RevealLabelKey = Extract<MessageKey, "explorer.revealFinder" | "explorer.revealFileExplorer" | "explorer.revealFolder" | "explorer.revealFileManager">;

/** What a menu reads from the host for the item: its label key, or null where the host has no file manager. */
export type RevealHost = { label: RevealLabelKey } | null;

/** The item's label key on the desktop host's OS (`process.platform`). */
export function revealLabel(platform: string | null | undefined): RevealLabelKey {
  if (platform === "darwin") return "explorer.revealFinder";
  if (platform === "win32") return "explorer.revealFileExplorer";
  if (platform === "linux") return "explorer.revealFolder";
  return "explorer.revealFileManager";
}

/** The item as every menu draws it (`MenuEntry` in `components/entry-menu.tsx`, kept JSX-free for the desktop host's imports). */
export type RevealExternalEntry = { id: "reveal_external"; label: string; unavailable: string | null; separated?: boolean };

/**
 * The item for a target on `device`, or none in a plain browser tab. A
 * target on another device, or one already known to be gone (`blocked`),
 * is listed disabled with its reason rather than hidden.
 */
export function revealExternalEntry(host: RevealHost, device: string, t: TFunction<"translation">, blocked: string | null = null, separated = false): RevealExternalEntry[] {
  if (!host) return [];
  const unavailable = device !== "local" ? t("explorer.revealHereOnly") : blocked;
  return [{ id: "reveal_external", label: t(host.label), unavailable, ...(separated ? { separated } : {}) }];
}
