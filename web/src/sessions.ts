// The rules of a Project's Sessions screen (PRD S8 B2-B7): which rows the
// provider filter and the search leave, what each row says, and which state
// the list and the detail are in. Everything here reads the named Project's
// section the core publishes (`project_sessions`); nothing is counted, dated
// or titled that it does not carry.

import type { ArchiveDetail, ArchiveEvent, ProjectSessionDetail, ProjectSessions, SessionRow, Workspace } from "./snapshot";

export type ProviderFilter = "all" | "codex" | "claude";

export const PROVIDER_FILTERS: readonly { id: ProviderFilter; label: string }[] = [
  { id: "all", label: "All" },
  { id: "codex", label: "Codex" },
  { id: "claude", label: "Claude Code" },
];

/**
 * The rows a provider filter and a search leave, in the core's newest-first
 * order: the core's own rule for the native panel (`apply_session_filters`),
 * a case-insensitive match on the first request, the title or the folder.
 * It runs here so typing costs no round trip and resends nothing.
 */
export function filterSessions(rows: SessionRow[], provider: ProviderFilter, query: string): SessionRow[] {
  const needle = query.trim().toLowerCase();
  return rows.filter(
    (row) =>
      (provider === "all" || row.provider === provider) &&
      (needle === "" || [row.first_human_request, row.title, row.checkout_path].some((value) => value?.toLowerCase().includes(needle))),
  );
}

/** What a session is called: its first request, else its title, else nothing it could be named by. */
export function sessionTitle(row: Pick<SessionRow, "first_human_request" | "title">): string | null {
  return row.first_human_request ?? row.title ?? null;
}

/**
 * The Workspace a session ran in: the Project's checkout whose folder holds
 * the session's folder (the longest one), named as the Overview names it, or
 * the folder's own name when no current checkout holds it (a worktree since
 * removed). `path` is the folder the session recorded.
 */
export function sessionCheckout(row: Pick<SessionRow, "checkout_path">, workspace: Workspace | null): { label: string; path: string } {
  const path = row.checkout_path;
  let best: { label: string; length: number } | null = null;
  for (const checkout of workspace?.checkouts ?? []) {
    const root = checkout.path.replace(/\/+$/, "");
    if (path !== root && !path.startsWith(`${root}/`)) continue;
    if (best && best.length >= root.length) continue;
    best = { label: checkout.branch ?? checkout.label, length: root.length };
  }
  return { label: best?.label ?? path.replace(/\/+$/, "").split("/").pop() ?? path, path };
}

/** A session's time as the operator reads it, or null when neither the session nor its file carried one. */
export function sessionTime(unixMs: number | null | undefined, now = new Date(), locale?: string): string | null {
  if (!unixMs) return null;
  const date = new Date(unixMs);
  const options: Intl.DateTimeFormatOptions = { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" };
  if (date.getFullYear() !== now.getFullYear()) options.year = "numeric";
  return new Intl.DateTimeFormat(locale, options).format(date);
}

/**
 * Whether the core's named Project is this screen's (A7). The core names one
 * Project at a time for every window of the daemon: `pending` until this
 * screen's request is answered, `ours` once it is, and `replaced` when
 * another window named another Project after that. A replaced screen asks
 * the operator before naming its Project again, so two windows cannot keep
 * taking it from each other.
 */
export type Naming = "pending" | "ours" | "replaced";

export function naming(sessions: ProjectSessions | null, project: { id: string; deviceId: string }, acknowledged: boolean): Naming {
  // A Project's id is its path's, so a device folder at the same path shares it; the device tells them apart.
  if (sessions?.workspace_id === project.id && sessions.device_id === project.deviceId) return "ours";
  return acknowledged && sessions ? "replaced" : "pending";
}

export type ListState =
  | { kind: "loading" }
  | { kind: "replaced" }
  | { kind: "unavailable"; reason: string }
  | { kind: "failed"; reason: string }
  | { kind: "empty" }
  | { kind: "no_match" }
  | { kind: "rows"; rows: SessionRow[] };

/**
 * The list place (B4, B7): the Project's own reason when it cannot be read
 * here at all, loading until its first answer, the read failure with Retry,
 * "none yet" when it has no session, "no match" when the filters leave none,
 * else the rows. A refresh keeps the rows it has while it reads.
 */
export function listState(sessions: ProjectSessions | null, named: Naming, provider: ProviderFilter, query: string): ListState {
  if (named === "replaced") return { kind: "replaced" };
  if (named === "pending" || !sessions) return { kind: "loading" };
  if (sessions.unavailable_reason) return { kind: "unavailable", reason: sessions.unavailable_reason };
  if (sessions.failure) return { kind: "failed", reason: sessions.failure };
  if (sessions.rows.length === 0) return sessions.loading ? { kind: "loading" } : { kind: "empty" };
  const rows = filterSessions(sessions.rows, provider, query);
  return rows.length === 0 ? { kind: "no_match" } : { kind: "rows", rows };
}

export type DetailState =
  | { kind: "none" }
  | { kind: "loading"; detail: ProjectSessionDetail }
  | { kind: "failed"; detail: ProjectSessionDetail; reason: string }
  | { kind: "open"; detail: ProjectSessionDetail; archive: ArchiveDetail };

/** The detail place (B3, B5): nothing open, reading, the reason it cannot open with Retry, or the conversation. */
export function detailState(sessions: ProjectSessions | null, named: Naming): DetailState {
  const detail = named === "ours" ? sessions?.detail : null;
  if (!detail) return { kind: "none" };
  if (detail.failure) return { kind: "failed", detail, reason: detail.failure };
  if (detail.archive) return { kind: "open", detail, archive: detail.archive };
  return { kind: "loading", detail };
}

/**
 * The turns a read-only archive shows: what the person asked and what the
 * agent answered, in order. Injected context, which is where Project
 * Memory's provided items travel, is left to the Memory surface (D-17).
 */
export function conversationTurns(archive: ArchiveDetail): ArchiveEvent[] {
  return archive.events.filter((event) => event.kind !== "injected" && (event.role === "user" || event.role === "assistant"));
}

/** A row read aloud: provider, first request, checkout, time and availability, in that order. */
export function sessionAccessibleName(row: SessionRow, checkout: string, time: string | null): string {
  return [row.provider_label, sessionTitle(row) ?? "Untitled session", checkout, time, row.unavailable_reason ? "unavailable" : "available"].filter(Boolean).join(", ");
}
