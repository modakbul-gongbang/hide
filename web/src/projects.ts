// The Projects list as the sidebar draws it: the raised Needs You and Done
// agents, then pinned rows under their own header, then the activity rows
// with a per-device fold of inactive projects, and per project a fold of
// inactive checkouts. The split only reads flags and groups the core set; no
// age, merge or attention rule is repeated.

import type { TFunction } from "i18next";
import type { MessageKey } from "./i18n/catalogs";
import { AGENT_GROUPS, agentGroupTitle, type AgentGroup, type ListedAgent } from "./navigation";
import type { Checkout, InactiveProjectGroup, MarkCounts, PullRequest, RecentCheckout, Workspace } from "./snapshot";

/** Return to this device's last usable checkout, then its primary, then row order. */
export function projectCheckout(workspace: Workspace, recent: readonly RecentCheckout[] = []): Checkout | null {
  const usable = workspace.checkouts.filter((checkout) => checkout.exists);
  for (const visit of recent) {
    if (visit.device_id !== workspace.device_id) continue;
    const checkout = usable.find((checkout) => checkout.id === visit.checkout_id);
    if (checkout) return checkout;
  }
  return usable.find((checkout) => checkout.is_primary) ?? usable[0] ?? null;
}

export type ProjectRow =
  | { kind: "header"; section: "pinned" | "recent"; count: number }
  | { kind: "raised"; group: AgentGroup; agents: ListedAgent[]; more: ListedAgent[]; expanded: boolean }
  | { kind: "workspace"; workspace: Workspace; level: "root" | "child" }
  | { kind: "inactive_projects"; group: InactiveProjectGroup; count: number };

/**
 * The groups the Projects list raises above its tree, and how many of each it
 * draws before folding the rest (docs/status-model.md); Working and Seen are the Agents tab's.
 */
const RAISED_CAP: Partial<Record<AgentGroup, number>> = { needs_you: 5, done: 3 };

/**
 * `listed` is every agent the Agents list shows. A raised section holds, in
 * the core's order, the Needs You or Done agents whose pane a drawn project's
 * (or the device's Home's) checkout owns, so every raised agent is also in the tree below; an empty
 * section is left out, and a raised row never unfolds, so it is handed on
 * folded (docs/status-model.md, The descendant badge). Past its cap a
 * section draws the most recent ones in `agents` and folds the rest into
 * `more`, drawn only while `openRaised` names the group.
 */
export function projectRows(
  workspaces: Workspace[],
  groups: InactiveProjectGroup[],
  listed: ListedAgent[],
  home: Workspace | null = null,
  openRaised: readonly string[] = [],
): ProjectRow[] {
  const rows: ProjectRow[] = [];
  const drawnPanes = new Set<string>();
  // The device's Home is drawn as its own row above, so its agents are raised like a project's.
  for (const workspace of home ? [...workspaces, home] : workspaces) {
    for (const checkout of workspace.checkouts) for (const tab of checkout.tabs) for (const pane of tab.panes) drawnPanes.add(pane.id);
  }
  for (const { group } of AGENT_GROUPS) {
    const cap = RAISED_CAP[group];
    if (cap === undefined) continue;
    const raised = listed.filter((row) => row.agent.group === group && drawnPanes.has(row.agent.pane_id));
    if (raised.length === 0) continue;
    const agents = raised.map((row) => (row.agent.lineage_collapsed === false ? { ...row, agent: { ...row.agent, lineage_collapsed: true } } : row));
    rows.push({ kind: "raised", group, agents: agents.slice(0, cap), more: agents.slice(cap), expanded: openRaised.includes(group) });
  }
  const pinned = workspaces.filter((row) => row.pinned);
  const recent = workspaces.filter((row) => !row.pinned);
  if (pinned.length > 0) {
    rows.push({ kind: "header", section: "pinned", count: pinned.length });
    for (const workspace of pinned) rows.push({ kind: "workspace", workspace, level: "root" });
  }
  rows.push({ kind: "header", section: "recent", count: recent.length });
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

type Age = { unit: "now" } | { unit: "minutes" | "hours" | "days"; count: number };

/** The first minute is "now", then minutes, hours and days; a timestamp from the future is "now". */
export function activityAge(unixMs: number | null | undefined, nowMs: number): Age | null {
  if (unixMs == null) return null;
  const seconds = (nowMs - unixMs) / 1000;
  if (seconds < 60) return { unit: "now" };
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return { unit: "minutes", count: minutes };
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return { unit: "hours", count: hours };
  return { unit: "days", count: Math.floor(hours / 24) };
}

export function ageText(age: Age, t: TFunction<"translation">): string {
  switch (age.unit) {
    case "now":
      return t("common.now");
    case "minutes":
      return t("common.ageMinutes", { count: age.count });
    case "hours":
      return t("common.ageHours", { count: age.count });
    case "days":
      return t("common.ageDays", { count: age.count });
  }
}

/** "now" for the first minute, then minutes, hours and days in the interface language; a timestamp from the future is "now". */
export function relativeActivity(unixMs: number | null | undefined, nowMs: number, t: TFunction<"translation">): string | null {
  const age = activityAge(unixMs, nowMs);
  return age === null ? null : ageText(age, t);
}

/**
 * A plain folder's only checkout: a project that is not a Git repository and
 * holds one checkout is drawn as that checkout, on one row.
 */
export function folderCheckout(workspace: Workspace): Checkout | null {
  return !workspace.is_git && workspace.checkouts.length === 1 ? (workspace.checkouts[0] ?? null) : null;
}

/**
 * A project's status badge: every checkout's marks added up, so the project
 * row says what its checkouts' rows would show (docs/status-model.md, The
 * status badge).
 */
export function projectMarks(workspace: Workspace): MarkCounts {
  return workspace.agent_scope.marks;
}

/** The glyph a pull request draws: its lifecycle, with a draft keeping its own shape while it is open or under review. */
export type PullRequestKind = Extract<CheckoutKind, `pr_${string}`>;

export function pullRequestKind(pr: PullRequest): PullRequestKind {
  if (pr.badge === "merged") return "pr_merged";
  if (pr.badge === "closed") return "pr_closed";
  return pr.is_draft ? "pr_draft" : "pr_open";
}

/** The word and tone of a review decision, on the badge of a pull request under review and on the card's Review row. */
const REVIEW_WORD: Record<NonNullable<PullRequest["review"]>, { value: MessageKey; tone: string }> = {
  approved: { value: "board.review.approved", tone: "text-success" },
  changes_requested: { value: "board.review.changes", tone: "text-destructive" },
  review_required: { value: "board.review.required", tone: "text-muted-foreground" },
};

/**
 * The pull request's badge (PRD checkout-pr-glyph-card D-08): the lifecycle
 * word for a merged, closed, open or draft pull request, and the review
 * decision for one under review. A draft that is under review keeps the
 * decision as its badge and says `draft` beside it, in the draft color.
 */
export function pullRequestBadge(pr: PullRequest, t: TFunction<"translation">): { label: string; color: string; draft: boolean } {
  switch (pr.badge) {
    case "merged":
      return { label: t("requests.badge.merged"), color: "text-pr-merged", draft: false };
    case "closed":
      return { label: t("requests.badge.closed"), color: "text-pr-closed", draft: false };
    case "open":
      return pr.is_draft ? { label: t("overview.draft"), color: "text-pr-draft", draft: false } : { label: t("requests.badge.open"), color: "text-pr-open", draft: false };
    case "review": {
      const word = REVIEW_WORD[pr.review ?? "review_required"];
      return { label: t(word.value), color: word.tone, draft: pr.is_draft };
    }
  }
}

/**
 * The pull request a checkout's row speaks for: a stale refresh keeps the
 * last known one (its glyph muted), an unavailable answer that is not stale
 * shows none, so the row falls back to its branch.
 */
export function shownPullRequest(checkout: Checkout): PullRequest | null {
  const pr = checkout.pull_request;
  const github = checkout.github;
  if (pr === null) return null;
  return !github || github.available || github.stale || github.unavailable_reason === null ? pr : null;
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
  /** Line two waits for Git to be read, so a row does not grow and shrink as the facts arrive. */
  secondLineReady: boolean;
  /** A merged or closed pull request's row is drawn dimmed. */
  settled: boolean;
  /** The row's screen-reader description: the pull request, the agents by state, and the branch and path. */
  detail: string;
  /** The pull request the row shows, whose glyph is a button and whose card has a header. */
  pullRequest: PullRequest | null;
};

const LIFECYCLE_TONE: Record<Extract<CheckoutKind, `pr_${string}`>, string> = {
  pr_open: "text-pr-open",
  pr_draft: "text-pr-draft",
  pr_merged: "text-pr-merged",
  pr_closed: "text-pr-closed",
};

export function checkoutPresentation(workspace: Workspace, checkout: Checkout, nowMs: number, t: TFunction<"translation">): CheckoutPresentation {
  const github = checkout.github;
  const pr = shownPullRequest(checkout);
  const primary = checkout.is_primary === true;
  const detached = checkout.worktree ? checkout.worktree.branch === null : false;
  const kind: CheckoutKind = primary ? "primary" : pr ? pullRequestKind(pr) : !workspace.is_git ? "folder" : detached ? "detached" : "branch";
  const kindTone = !checkout.exists
    ? "text-destructive"
    : kind.startsWith("pr_")
      ? github?.stale
        ? "text-muted-foreground"
        : LIFECYCLE_TONE[kind as keyof typeof LIFECYCLE_TONE]
      : "text-subtle-foreground";
  const gitLoading = !!workspace.is_git && !checkout.worktree;
  const commitSeconds = checkout.worktree?.last_commit_unix_seconds;
  const age = checkout.exists && !gitLoading && commitSeconds != null ? relativeActivity(commitSeconds * 1000, nowMs, t) : null;
  const summary = checkout.agent_summary;
  const agentCount = summary ? summary.needs_you + summary.done + summary.working + summary.seen : 0;

  const lines: string[] = [];
  if (pr) {
    let first = `#${pr.number} · ${pullRequestBadge(pr, t).label}`;
    if (pr.title) first += ` · ${pr.title}`;
    const lastKnown = github?.stale ? activityAge(github.last_success_at_unix_ms, nowMs) : null;
    if (lastKnown) first += ` · ${t("card.lastKnown", { age: ageText(lastKnown, t) })}`;
    lines.push(first);
  }
  if (github?.unavailable_reason) lines.push(github.unavailable_reason);
  if (summary && agentCount > 0) {
    const counts = (
      [
        ["needs_you", summary.needs_you],
        ["done", summary.done],
        ["working", summary.working],
        ["seen", summary.seen],
      ] as const
    )
      .filter(([, count]) => count > 0)
      .map(([group, count]) => t("card.stateCount", { name: agentGroupTitle(group, t), count }))
      .join(" · ");
    lines.push(summary.unknown > 0 ? t("card.countsWithUnknown", { counts, unknown: summary.unknown }) : counts);
  }
  if (detached) lines.push(checkout.worktree?.head_sha ? t("card.detachedAt", { sha: checkout.worktree.head_sha }) : t("card.detached"));
  else if (checkout.branch) lines.push(checkout.branch);
  lines.push(checkout.path);

  return {
    kind,
    kindTone,
    age,
    secondLineReady: !gitLoading,
    settled: pr?.badge === "merged" || pr?.badge === "closed",
    detail: lines.join("\n"),
    pullRequest: pr,
  };
}

/** One line of the checkout card: a label and its value in a tone; the Agents row carries marks instead of a word. */
export type CheckoutCardRow =
  | { key: "review" | "checks"; label: string; value: string; tone: string }
  | { key: "branch" | "commit" | "base" | "changes"; label: string; value: string }
  | { key: "path"; label: string; value: string }
  | { key: "agents"; label: string; marks: MarkCounts };

/** The card a checkout row opens on hover and focus (PRD checkout-pr-glyph-card D-03, D-08, D-09, D-12). */
export type CheckoutCard = {
  /** A pull request's badge line, or the danger header of a folder that is gone; null on a plain checkout. */
  header:
    | { kind: "pull_request"; badge: ReturnType<typeof pullRequestBadge>; glyph: PullRequestKind; number: number; url: string; title: string }
    | { kind: "missing"; label: string }
    | null;
  /** Each row only where its source has a value; a row with nothing to say takes no place. */
  rows: CheckoutCardRow[];
};

const CHECKS_WORD: Partial<Record<NonNullable<PullRequest["checks"]>, { value: MessageKey; tone: string }>> = {
  passing: { value: "card.checkPassing", tone: "text-success" },
  failed: { value: "card.checkFailed", tone: "text-destructive" },
  pending: { value: "card.checkPending", tone: "text-muted-foreground" },
};

/**
 * Every value the card draws comes from the snapshot (design principle 10):
 * Review and Checks from the pull request, Branch from the worktree (a
 * detached HEAD names its short sha), Agents from the summary while any agent
 * runs, Commit from the last commit's age once Git has been read, Path always.
 * A folder that is gone has only its Path under a danger header.
 */
export function checkoutCard(workspace: Workspace, checkout: Checkout, nowMs: number, t: TFunction<"translation">): CheckoutCard {
  const rows: CheckoutCardRow[] = [];
  if (!checkout.exists) {
    rows.push({ key: "path", label: t("card.path"), value: checkout.path });
    return { header: { kind: "missing", label: t("graph.folderMissing") }, rows };
  }
  const pr = shownPullRequest(checkout);
  if (pr) rows.push(...pullRequestRows(pr, t));
  const worktree = checkout.worktree;
  if (workspace.is_git && worktree) {
    if (worktree.branch) rows.push({ key: "branch", label: t("card.branch"), value: worktree.branch });
    else rows.push({ key: "branch", label: t("card.branch"), value: worktree.head_sha ? t("card.detachedAt", { sha: worktree.head_sha.slice(0, 7) }) : t("card.detached") });
  }
  const summary = checkout.agent_summary;
  if (summary && summary.needs_you + summary.done + summary.working + summary.seen > 0) rows.push({ key: "agents", label: t("overview.agents"), marks: summary.marks });
  const commitSeconds = worktree?.last_commit_unix_seconds;
  const age = activityAge(commitSeconds != null ? commitSeconds * 1000 : null, nowMs);
  if (age) rows.push({ key: "commit", label: t("card.commit"), value: age.unit === "now" ? t("common.now") : t("card.ago", { age: ageText(age, t) }) });
  rows.push({ key: "path", label: t("card.path"), value: checkout.path });
  return {
    header: pr ? pullRequestHeader(pr, t) : null,
    rows,
  };
}

/**
 * The PR card of a pull request on its own, for a PRs tab row whose branch
 * has no checkout here (PRD overview-lenses-prs B7): the same header and
 * Review and Checks rows the checkout card draws, and the branch.
 */
export function pullRequestCard(pr: PullRequest, t: TFunction<"translation">): CheckoutCard {
  const rows = pullRequestRows(pr, t);
  if (pr.head_branch) rows.push({ key: "branch", label: t("card.branch"), value: pr.head_branch });
  return { header: pullRequestHeader(pr, t), rows };
}

function pullRequestHeader(pr: PullRequest, t: TFunction<"translation">): NonNullable<CheckoutCard["header"]> {
  return { kind: "pull_request", badge: pullRequestBadge(pr, t), glyph: pullRequestKind(pr), number: pr.number, url: pr.url, title: pr.title };
}

/** A pull request's Review and Checks rows, each once GitHub has answered it. */
function pullRequestRows(pr: PullRequest, t: TFunction<"translation">): CheckoutCardRow[] {
  const rows: CheckoutCardRow[] = [];
  if (pr.review) rows.push({ key: "review", label: t("card.review"), value: t(REVIEW_WORD[pr.review].value), tone: REVIEW_WORD[pr.review].tone });
  const checks = pr.checks ? CHECKS_WORD[pr.checks] : undefined;
  if (checks) rows.push({ key: "checks", label: t("card.checks"), value: t(checks.value), tone: checks.tone });
  return rows;
}

/** `↑N ↓N`, each part only above zero; empty when the branch is even with both. */
export function distanceText(ahead: number, behind: number): string {
  return [ahead > 0 ? `↑${ahead}` : null, behind > 0 ? `↓${behind}` : null].filter(Boolean).join(" ");
}

/** `1 file`, `N files`. */
export function filesText(count: number, t: TFunction<"translation">): string {
  return t("card.files", { count });
}

/**
 * The card an Agents graph box head opens (PRD agents-graph-view
 * B15): the checkout card with where the branch stands against its base
 * (`↑` commits of its own, `↓` behind its upstream) and how many files it
 * changed, each only once Git has said so. Lines added and removed are not
 * in the snapshot, so the card does not draw them (design 10).
 */
export function laneCheckoutCard(workspace: Workspace, checkout: Checkout, nowMs: number, t: TFunction<"translation">): CheckoutCard {
  const card = checkoutCard(workspace, checkout, nowMs, t);
  const worktree = checkout.worktree;
  if (!checkout.exists || !workspace.is_git || !worktree) return card;
  const extra: CheckoutCardRow[] = [];
  const ahead = checkout.ahead ?? 0;
  const behind = worktree.behind_upstream ?? 0;
  if (checkout.is_worktree && checkout.is_primary !== true && workspace.default_branch) {
    extra.push({ key: "base", label: t("card.base"), value: [workspace.default_branch, distanceText(ahead, behind)].filter(Boolean).join(" ") });
  }
  extra.push({ key: "changes", label: t("card.changes"), value: worktree.changed_file_count === 0 ? t("card.clean") : filesText(worktree.changed_file_count, t) });
  const at = card.rows.findIndex((row) => row.key === "branch");
  const rows = [...card.rows];
  rows.splice(at < 0 ? 0 : at + 1, 0, ...extra);
  return { ...card, rows };
}

/** A card with one value and no header is the plain text tooltip instead (D-09): that one value, never an empty card. */
export function cardSingleValue(card: CheckoutCard): string | null {
  if (card.header !== null || card.rows.length !== 1) return null;
  const row = card.rows[0]!;
  return row.key === "agents" ? null : row.value;
}

/** A sidebar activation opens agents unless this Workspace is already open and unfolded. */
export function checkoutRowExpansion(foldable: boolean, workspaceSelected: boolean, expanded: boolean): boolean | undefined {
  return foldable ? !(workspaceSelected && expanded) : undefined;
}

/** A project row opens its usable checkout, then folds if that target is already open and unfolded. */
export function projectRowExpansion(workspace: Workspace, target: Checkout | null, focusedCheckoutId: string | null, foldable: boolean): boolean | undefined {
  return checkoutRowExpansion(foldable, target !== null && target.id === focusedCheckoutId, workspace.expanded !== false);
}

/**
 * Whether a checkout row draws line two: only when it has something to say,
 * a purpose or the parent its agents were raised from, once Git has been read
 * (PRD sidebar-typography D-04). Agents alone never earn a line; the row's age
 * then ends line one.
 */
export function checkoutHasSecondLine(view: Pick<CheckoutPresentation, "secondLineReady">, purpose: string | null, raisedFrom: string | null): boolean {
  return view.secondLineReady && (purpose !== null || raisedFrom !== null);
}

/**
 * A checkout name read as its path prefix, up to and including the first
 * slash (`prd/`, `gen-prd/`), which the row mutes, and the rest (D-06). A
 * name without a slash, or with nothing on either side of it, has no prefix.
 */
export function checkoutNameParts(name: string): { prefix: string; rest: string } {
  const slash = name.indexOf("/");
  if (slash <= 0 || slash === name.length - 1) return { prefix: "", rest: name };
  return { prefix: name.slice(0, slash + 1), rest: name.slice(slash + 1) };
}
