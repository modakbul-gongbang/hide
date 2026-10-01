// What ⌘K asks GitHub and how it says so (PRD cmdk-navigation B8-B10, B16-B19).
// Two readings meet here: the one read of the project in front, which ⌘K asks
// for once and which the core answers on the project's own checkouts, and the
// explicit search a `GitHub에서 "…" 검색` row starts. Typing never calls
// GitHub; both are pure decisions over the snapshot so they test without a
// browser.

import { frontDeviceId, localDeviceId } from "./devices";
import { ageWords } from "./overviewLens";
import type { Checkout, GithubSearch, SnapshotRest, Workspace } from "./snapshot";

/** The decimal minutes-ago words the detail and the failure tooltip use. */
function readAge(at: number | null | undefined, now: number): string | null {
  return at == null ? null : ageWords(now - at);
}

function githubStatus(workspace: Workspace) {
  return workspace.checkouts.find((checkout) => checkout.github)?.github ?? null;
}

/** Whether the core has answered this project's pull requests: a success on record, or the answer that it has no GitHub remote. */
function answered(workspace: Workspace): boolean {
  const status = githubStatus(workspace);
  return status?.last_success_at_unix_ms != null || status?.failure_category === "no GitHub remote";
}

/**
 * The project ⌘K reads when it opens (B8, D-12): the local project in front,
 * once per app run, when no screen has asked for it. A project the core has
 * answered, one already asked for here, and a device's project are left alone.
 * `source.reading` alone is not "in flight": a project nobody asked for
 * reports it forever (tasks.rs), so it is never read as a request.
 */
export function projectToRead(rest: SnapshotRest | null, frontCheckout: Checkout | null, asked: ReadonlySet<string>): Workspace | null {
  if (!rest || !frontCheckout || frontDeviceId(rest) !== localDeviceId(rest)) return null;
  const workspace = rest.navigator?.workspaces?.find((row) => row.id === frontCheckout.workspace_id);
  if (!workspace || workspace.is_home || workspace.remote_target_id || !workspace.is_git) return null;
  if (asked.has(workspace.id) || answered(workspace)) return null;
  return workspace;
}

/** How a project's read stands for the rows that name its checkouts. */
export type ProjectRead = { state: "reading" } | { state: "failed"; tooltip: string } | null;

/**
 * Reading while ⌘K asked and the core has not answered, failed once the core
 * says so, with the last value's age (B9); nothing otherwise, so a project
 * nobody asked for never shows a progress mark.
 */
export function projectRead(workspace: Workspace, asked: ReadonlySet<string>, now: number): ProjectRead {
  const status = githubStatus(workspace);
  const failure = workspace.tasks?.source?.failure ?? null;
  const failed = status?.failure_category !== "no GitHub remote" && (status?.stale === true || (status?.unavailable_reason != null && !status.available) || failure !== null);
  if (failed) {
    const age = readAge(status?.last_success_at_unix_ms ?? workspace.tasks?.source?.last_read_at_unix_ms, now);
    return { state: "failed", tooltip: ["GitHub 읽기 실패", age ? `${age} 값` : "읽은 값 없음", "이유는 로그에"].join(" · ") };
  }
  if (asked.has(workspace.id) && !answered(workspace)) return { state: "reading" };
  return null;
}

/** The `N분 전 읽음` a pull request's or issue's detail carries (B10): when its project was last read. */
export function lastReadWords(workspace: Workspace, now: number): string | null {
  const at = githubStatus(workspace)?.last_success_at_unix_ms ?? workspace.tasks?.source?.last_read_at_unix_ms;
  const age = readAge(at, now);
  return age ? `${age} 읽음` : null;
}

/** Whether this Mac has a project a GitHub search could look in (B16). */
export function hasGithubProject(rest: SnapshotRest | null): boolean {
  return (rest?.navigator?.workspaces ?? []).some((workspace) => !workspace.is_home && !workspace.remote_target_id && workspace.is_git && workspace.tasks?.source?.kind === "github");
}

/** The state of the `GitHub에서 "…" 검색` row. */
export type GithubRow = { state: "idle" | "working" | "failed" | "none"; label: string };

/**
 * The row's words for the answer to this query: the search itself while none
 * ran or one is running, `GitHub 검색 실패 · 다시 시도` when it failed (B18),
 * `GitHub에도 없음` once it answered with nothing.
 */
export function githubRow(query: string, answer: GithubSearch | null): GithubRow {
  const label = `GitHub에서 "${query}" 검색`;
  if (!answer) return { state: "idle", label };
  if (answer.phase === "working") return { state: "working", label };
  if (answer.phase === "failed") return { state: "failed", label: "GitHub 검색 실패 · 다시 시도" };
  return answer.results.length === 0 ? { state: "none", label: "GitHub에도 없음" } : { state: "idle", label };
}

/** The core's answer for what this palette asked: only the request it sent, and only while the query is still the one it searched (B19). */
export function ownAnswer(search: GithubSearch | null | undefined, asked: { requestId: string; query: string } | null, query: string): GithubSearch | null {
  if (!search || !asked || search.request_id !== asked.requestId || asked.query !== query) return null;
  return search;
}

/** Whether choosing the row starts a search: not while the same query is already running (B19). */
export function startsSearch(row: GithubRow): boolean {
  return row.state !== "working" && row.state !== "none";
}
