// The registration input's own checks (PRD S2 B9): what the shell can tell
// from the snapshot and the listing it already holds is refused before an
// event goes out; everything else hided decides and answers with a reason
// code, shown in the same place. Recent registrations are a shell
// convenience kept in localStorage.

import type { WorkspaceRegistration } from "./snapshot";
import type { DirectoryList } from "./store";

export const RECENT_KEY = "hide.recentWorkspacePaths";
export const RECENT_CAP = 5;

export type LocalRefusal = "empty" | "already_registered" | "outside_home" | "not_in_listing";

export function normalizePath(raw: string): string {
  const trimmed = raw.trim();
  if (trimmed.length > 1 && trimmed.endsWith("/")) return trimmed.replace(/\/+$/, "");
  return trimmed;
}

/** The reason not to send `create_workspace`, or null when hided gets to decide. */
export function localRefusal(
  raw: string,
  home: string,
  registrations: WorkspaceRegistration[],
  listing: DirectoryList | null,
): LocalRefusal | null {
  const path = normalizePath(raw);
  if (!path) return "empty";
  if (!(path === home || path.startsWith(`${home}/`))) return "outside_home";
  if (registrations.some((row) => normalizePath(row.path) === path)) return "already_registered";
  const parent = parentOf(path);
  if (listing && listing.root_path === parent && !listing.truncated && !listing.entries.some((row) => row.path === path)) {
    return "not_in_listing";
  }
  return null;
}

export function parentOf(path: string): string {
  const index = path.lastIndexOf("/");
  return index <= 0 ? "/" : path.slice(0, index);
}

/** The reason as one line under the input, for a local refusal or a hided reason code. */
export function refusalText(reason: string): string {
  switch (reason) {
    case "empty":
      return "경로를 입력하세요.";
    case "already_registered":
      return "이미 등록된 경로입니다.";
    case "outside_home":
      return "홈 디렉터리 아래의 경로만 등록할 수 있습니다.";
    case "not_in_listing":
    case "not_found":
      return "존재하지 않는 경로입니다.";
    case "not_a_directory":
      return "파일이 아니라 디렉터리를 입력하세요.";
    case "home_root":
      return "홈 디렉터리 자체는 등록할 수 없습니다.";
    case "invalid_path":
      return "절대 경로만 등록할 수 있습니다(`..` 없이).";
    default:
      return `등록 거부: ${reason}`;
  }
}

/** Listing entries that continue what was typed: the parent's children matching the last segment. */
export function suggestions(raw: string, listing: DirectoryList | null): string[] {
  const path = raw.trim();
  if (!listing) return [];
  const slash = path.lastIndexOf("/");
  const parent = slash <= 0 ? "/" : path.slice(0, slash);
  const prefix = path.slice(slash + 1).toLowerCase();
  if (listing.root_path !== parent && !(path.endsWith("/") && listing.root_path === normalizePath(path))) return [];
  const head = path.endsWith("/") ? "" : prefix;
  return listing.entries.filter((row) => row.name.toLowerCase().startsWith(head)).map((row) => row.path);
}

/** The directory whose listing the input needs for `raw`. */
export function listingRootFor(raw: string, home: string): string {
  const path = raw.trim();
  if (!path.startsWith(home)) return home;
  if (path.endsWith("/")) return normalizePath(path);
  return parentOf(path) || home;
}

export function readRecent(storage: Pick<Storage, "getItem">): string[] {
  try {
    const raw = storage.getItem(RECENT_KEY);
    const parsed = raw ? (JSON.parse(raw) as unknown) : [];
    return Array.isArray(parsed) ? parsed.filter((row): row is string => typeof row === "string").slice(0, RECENT_CAP) : [];
  } catch {
    return [];
  }
}

export function rememberRecent(storage: Pick<Storage, "getItem" | "setItem">, path: string): string[] {
  const next = [path, ...readRecent(storage).filter((row) => row !== path)].slice(0, RECENT_CAP);
  try {
    storage.setItem(RECENT_KEY, JSON.stringify(next));
  } catch {
    /* a full or disabled storage loses the convenience, not the registration */
  }
  return next;
}
