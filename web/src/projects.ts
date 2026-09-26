// The Projects list as the sidebar draws it, mirrored from the Swift shell's
// `SidebarProjectSections` and `SidebarInactiveProjection`: pinned rows
// under their own header, then the activity rows with a per-device fold of
// inactive projects, and per project a fold of inactive checkouts. The
// split only reads flags the core set; no age or merge rule is repeated.

import type { AgentRow, Checkout, InactiveProjectGroup, PullRequest, Workspace } from "./snapshot";

export type ProjectRow =
  | { kind: "header"; title: string; count: number }
  | { kind: "workspace"; workspace: Workspace; level: "root" | "child" }
  | { kind: "inactive_projects"; group: InactiveProjectGroup; count: number };

export const PINNED_TITLE = "Pinned";
export const RECENT_TITLE = "Projects · Recent activity";

export function projectRows(workspaces: Workspace[], groups: InactiveProjectGroup[]): ProjectRow[] {
  const pinned = workspaces.filter((row) => row.pinned);
  const recent = workspaces.filter((row) => !row.pinned);
  const rows: ProjectRow[] = [];
  if (pinned.length > 0) {
    rows.push({ kind: "header", title: PINNED_TITLE, count: pinned.length });
    for (const workspace of pinned) rows.push({ kind: "workspace", workspace, level: "root" });
  }
  rows.push({ kind: "header", title: RECENT_TITLE, count: recent.length });
  const byId = new Map(recent.map((row) => [row.id, row]));
  const deviceIds = [...new Set(recent.map((row) => row.device_id))];
  for (const deviceId of deviceIds) {
    const group = groups.find((row) => row.device_id === deviceId);
    const inactiveIds = new Set(group?.project_ids ?? []);
    for (const workspace of recent) {
      if (workspace.device_id === deviceId && !inactiveIds.has(workspace.id)) {
        rows.push({ kind: "workspace", workspace, level: "root" });
      }
    }
    if (!group) continue;
    const inactive = group.project_ids.map((id) => byId.get(id)).filter((row): row is Workspace => !!row);
    if (inactive.length === 0) continue;
    rows.push({ kind: "inactive_projects", group, count: inactive.length });
    if (group.expanded) for (const workspace of inactive) rows.push({ kind: "workspace", workspace, level: "child" });
  }
  return rows;
}

export function activeCheckouts(workspace: Workspace): Checkout[] {
  const inactive = new Set(workspace.inactive_checkouts.checkout_ids);
  return workspace.checkouts.filter((row) => !inactive.has(row.id));
}

export function inactiveCheckouts(workspace: Workspace): Checkout[] {
  const byId = new Map(workspace.checkouts.map((row) => [row.id, row]));
  return workspace.inactive_checkouts.checkout_ids.map((id) => byId.get(id)).filter((row): row is Checkout => !!row);
}

/** "now" for the first minute, then m / h / d; a timestamp from the future is "now". */
export function relativeActivity(unixMs: number | null | undefined, nowMs: number): string | null {
  if (unixMs == null) return null;
  const seconds = (nowMs - unixMs) / 1000;
  if (seconds < 60) return "now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  return `${Math.floor(hours / 24)}d`;
}

/**
 * A plain folder's only checkout: a project that is not a Git repository and
 * holds one checkout is drawn as that checkout, on one row.
 */
export function folderCheckout(workspace: Workspace): Checkout | null {
  return !workspace.is_git && workspace.checkouts.length === 1 ? (workspace.checkouts[0] ?? null) : null;
}

/** The count first (it names the project's contents), then the recency the order was decided by. */
export function activityLabel(workspace: Workspace, agents: AgentRow[], nowMs: number): string {
  const paneIds = new Set(workspace.checkouts.flatMap((c) => c.tabs).flatMap((t) => t.panes).map((p) => p.id));
  const agentCount = agents.filter((agent) => paneIds.has(agent.pane_id)).length;
  const checkoutCount = workspace.checkouts.length;
  const counts =
    agentCount > 0
      ? agentCount === 1
        ? "1 agent"
        : `${agentCount} agents`
      : checkoutCount === 1
        ? "1 workspace"
        : `${checkoutCount} workspaces`;
  const recency = relativeActivity(workspace.last_activity_unix_ms, nowMs);
  return recency ? `${counts} · ${recency}` : counts;
}

/** The PR badge's text and color token, mirrored from `CheckoutCardPresentation.badgeLabel/badgeColor`. */
export function pullRequestBadge(pr: PullRequest): { label: string; color: string } {
  switch (pr.badge) {
    case "merged":
      return { label: "merged", color: "text-pr-merged" };
    case "closed":
      return { label: "closed", color: "text-pr-closed" };
    case "open":
      return { label: pr.is_draft ? "draft" : "open", color: pr.is_draft ? "text-pr-draft" : "text-pr-open" };
    case "review":
      switch (pr.review) {
        case "approved":
          return { label: "approved", color: "text-success" };
        case "changes_requested":
          return { label: "changes", color: "text-destructive" };
        default:
          return { label: "review", color: "text-warning" };
      }
  }
}

/** The checkout row's leading glyph: a pull request's lifecycle, else what kind of checkout it is. */
export type CheckoutKind = "pr_open" | "pr_draft" | "pr_merged" | "pr_closed" | "folder" | "primary" | "detached" | "branch";

/** One checkout row of the Projects tab, mirrored from `SidebarCheckoutPresentation`. */
export type CheckoutPresentation = {
  kind: CheckoutKind;
  /** The glyph's color class: danger for a missing folder, muted for stale GitHub data, else the lifecycle color. */
  kindTone: string;
  /** The last commit's age; absent for a missing folder and until Git has been read. */
  age: string | null;
  /** Live agents here, each counted once. */
  agentCount: number;
  /** Line two waits for Git to be read, so a row does not grow and shrink as the facts arrive. */
  secondLineReady: boolean;
  /** A merged or closed pull request's row is drawn dimmed. */
  settled: boolean;
  /** The tooltip: the pull request, the agents by state, and the branch and path. */
  detail: string;
};

const LIFECYCLE_TONE: Record<Extract<CheckoutKind, `pr_${string}`>, string> = {
  pr_open: "text-pr-open",
  pr_draft: "text-pr-draft",
  pr_merged: "text-pr-merged",
  pr_closed: "text-pr-closed",
};

export function checkoutPresentation(workspace: Workspace, checkout: Checkout, nowMs: number): CheckoutPresentation {
  const pr = checkout.pull_request;
  const github = checkout.github;
  // A stale refresh keeps the last known pull request and mutes its glyph; an
  // unavailable answer that is not stale falls back to the branch glyph.
  const showsPr = pr !== null && (!github || github.available || github.stale || github.unavailable_reason === null);
  const primary = !checkout.is_worktree && checkout.path === workspace.path;
  const detached = checkout.worktree ? checkout.worktree.branch === null : false;
  const kind: CheckoutKind =
    pr && showsPr
      ? pr.badge === "merged"
        ? "pr_merged"
        : pr.badge === "closed"
          ? "pr_closed"
          : pr.is_draft
            ? "pr_draft"
            : "pr_open"
      : !workspace.is_git
        ? "folder"
        : primary
          ? "primary"
          : detached
            ? "detached"
            : "branch";
  const kindTone = !checkout.exists
    ? "text-destructive"
    : kind.startsWith("pr_")
      ? github?.stale
        ? "text-muted-foreground"
        : LIFECYCLE_TONE[kind as keyof typeof LIFECYCLE_TONE]
      : "text-subtle-foreground";
  const gitLoading = !!workspace.is_git && !checkout.worktree;
  const commitSeconds = checkout.worktree?.last_commit_unix_seconds;
  const age = checkout.exists && !gitLoading && commitSeconds != null ? relativeActivity(commitSeconds * 1000, nowMs) : null;
  const summary = checkout.agent_summary;
  const agentCount = summary ? summary.needs_you + summary.done + summary.working + summary.seen : 0;

  const lines: string[] = [];
  if (pr && showsPr) {
    let first = `#${pr.number} · ${pullRequestBadge(pr).label}`;
    if (pr.title) first += ` · ${pr.title}`;
    if (github?.stale && github.last_success_at_unix_ms != null) first += ` · Last known ${relativeActivity(github.last_success_at_unix_ms, nowMs)}`;
    lines.push(first);
  }
  if (github?.unavailable_reason) lines.push(github.unavailable_reason);
  if (summary && agentCount > 0) {
    const counts = (
      [
        ["Needs You", summary.needs_you],
        ["Done", summary.done],
        ["Working", summary.working],
        ["Seen", summary.seen],
      ] as const
    )
      .filter(([, count]) => count > 0)
      .map(([name, count]) => `${name}: ${count}`)
      .join(" · ");
    lines.push(summary.unknown > 0 ? `${counts} (${summary.unknown} Unknown)` : counts);
  }
  if (detached) lines.push(`Detached HEAD${checkout.worktree?.head_sha ? ` at ${checkout.worktree.head_sha}` : ""}`);
  else if (checkout.branch) lines.push(checkout.branch);
  lines.push(checkout.path);

  return {
    kind,
    kindTone,
    age,
    agentCount,
    secondLineReady: !gitLoading,
    settled: pr?.badge === "merged" || pr?.badge === "closed",
    detail: lines.join("\n"),
  };
}
