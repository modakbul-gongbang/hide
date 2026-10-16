// The rules of a pull request's decision card that are not drawing (PRD
// work-tab-pr-lens D-06, D-21, D-27; PRD sidebar-task-lens D-35, D-36): the
// body's outline, which button is primary for a case, why Merge is not
// available, the decision box's facts, the four steps, and where a changed
// file opens. The core decided the case (`reason.kind`) and read the card's parts;
// nothing here re-derives a group.

import type { PrReason } from "./agentScope";
import type { PrCard, PrMergeMethod, PullRequest, PullRequestWork } from "./snapshot";

/** The `##` headings of a body, in order, as the card's table of contents (B27). Fenced code is not searched. */
export function bodyOutline(body: string): string[] {
  const headings: string[] = [];
  let fence: string | null = null;
  for (const line of body.split("\n")) {
    const marker = /^\s{0,3}(```+|~~~+)/.exec(line);
    if (marker) {
      const mark = marker[1]!;
      if (fence === null) fence = mark[0]!;
      else if (mark[0] === fence) fence = null;
      continue;
    }
    if (fence !== null) continue;
    const heading = /^\s{0,3}##\s+(.+?)\s*#*\s*$/.exec(line);
    if (heading?.[1]) headings.push(heading[1]);
  }
  return headings;
}

/** The first lines of a body for the folded preview: no headings' marks, no blank runs. */
export function bodyPreview(body: string, lines = 3): string {
  return body
    .split("\n")
    .map((line) => line.replace(/^\s{0,3}#{1,6}\s+/, "").trim())
    .filter((line) => line !== "")
    .slice(0, lines)
    .join("\n");
}

/** What the primary button of the button row is (D-21). */
export type PrimaryAction = "merge" | "update" | "review" | "assign" | "answer" | "view_pane" | "assign_reviewer" | "github";

export function primaryAction(kind: PrReason["kind"]): PrimaryAction {
  switch (kind) {
    case "merge":
      return "merge";
    case "update":
      return "update";
    case "review":
    case "team_review":
      return "review";
    case "reply":
      return "answer";
    case "check":
      return "view_pane";
    case "failed":
    case "conflict":
    case "changes":
      return "assign";
    case "assign_reviewer":
      return "assign_reviewer";
    default:
      return "github";
  }
}

/** Why Merge cannot be pressed now, as the key of the reason word and its values; null when it can (B30). */
export type MergeWhy = { key: "ci" | "failed" | "changes" | "agent" | "conflict" | "reviewer" | "blocked" | "draft" | "permission" | "unknown" | "unread"; count: number; base: string };

export function mergeWhy(pr: PullRequest, card: PrCard | null, working: boolean): MergeWhy | null {
  const work = pr.work;
  if (!card?.merge) return { key: "unread", count: 0, base: "" };
  if (!card.merge.can_merge) return { key: "permission", count: 0, base: "" };
  if (pr.is_draft) return { key: "draft", count: 0, base: "" };
  if (working) return { key: "agent", count: 0, base: "" };
  switch (work?.merge_state) {
    case "clean":
    case "has_hooks":
    case "unstable":
      return null;
    case "dirty":
      return { key: "conflict", count: 0, base: pr.base_branch ?? "" };
    case "behind":
      return { key: "blocked", count: 0, base: "" };
    case "blocked":
      if ((work.pending_checks ?? 0) > 0) return { key: "ci", count: work.pending_checks, base: "" };
      if (pr.checks === "failed") return { key: "failed", count: Math.max(work.failed_checks?.length ?? 0, 1), base: "" };
      if (pr.review === "changes_requested") return { key: "changes", count: 0, base: "" };
      return { key: "reviewer", count: 0, base: "" };
    case "draft":
      return { key: "draft", count: 0, base: "" };
    default:
      return { key: "unknown", count: 0, base: "" };
  }
}

/** The method Merge uses unless the operator picked another: GitHub's own default order. */
export function defaultMethod(card: PrCard | null): PrMergeMethod | null {
  const methods = card?.merge?.methods ?? [];
  return methods[0] ?? null;
}

/**
 * Where a changed file opens (D-27): the Hide diff when the Workspace in
 * front is a checkout of the pull request's branch and its committed changes
 * hold the file, else GitHub's files page of the pull request.
 */
export type FileOpen = { kind: "diff"; path: string } | { kind: "github"; url: string };

export function fileOpen(input: { pr: Pick<PullRequest, "url" | "head_branch">; front: { branch: string | null; path: string } | null; committed: readonly { path: string }[] | null; file: string }): FileOpen {
  const { pr, front, committed, file } = input;
  if (front && front.branch !== null && front.branch === pr.head_branch && committed) {
    const absolute = `${front.path}/${file}`;
    if (committed.some((entry) => entry.path === absolute)) return { kind: "diff", path: absolute };
  }
  return { kind: "github", url: `${pr.url}/files` };
}

/** The files the card draws and the generated ones folded into one line (B28). */
export function splitFiles(files: PrCard["files"]): { shown: NonNullable<PrCard["files"]>; generated: number } {
  const all = files ?? [];
  return { shown: all.filter((file) => !file.generated), generated: all.filter((file) => file.generated).length };
}

/** What a confirmation named when it opened (D-22). */
export type Confirmed = { action: "merge" | "update"; head: string; method: PrMergeMethod | null; queue: boolean };

/**
 * Whether a confirmation still holds: the card shows the same head, the same
 * queue and, for a merge, the same method and Merge still allowed, or for an
 * update the branch still behind its base. Else
 * it closes and nothing is sent, so a click on Yes never writes what the
 * operator was not shown.
 */
export function confirmHolds(confirm: Confirmed, now: { head: string | null; queue: boolean; method: PrMergeMethod | null; mergeBlocked: boolean; behind: boolean }): boolean {
  if (confirm.head !== now.head || confirm.queue !== now.queue) return false;
  if (confirm.action === "update") return now.behind;
  return !now.mergeBlocked && (now.queue || confirm.method === now.method);
}

/** Where the merge stands with its base, as GitHub said it last. */
const MERGEABLE: readonly PullRequestWork["merge_state"][] = ["clean", "has_hooks", "unstable"];

/**
 * One fact of the decision box's fact line (PRD sidebar-task-lens B33): the
 * checks, the review and the conflict with the base, each said only when
 * GitHub said it. The card words each.
 */
export type PrFact =
  | { kind: "checks_passing" }
  | { kind: "checks_running"; count: number }
  | { kind: "checks_failed"; count: number }
  | { kind: "approved" }
  | { kind: "changes" }
  | { kind: "review_required" }
  | { kind: "conflict"; base: string }
  | { kind: "no_conflict"; base: string }
  | { kind: "behind"; base: string };

export function prFacts(pr: PullRequest): PrFact[] {
  const facts: PrFact[] = [];
  const work = pr.work;
  const base = pr.base_branch ?? "";
  if (pr.checks === "pending" || (work?.pending_checks ?? 0) > 0) facts.push({ kind: "checks_running", count: Math.max(work?.pending_checks ?? 0, 1) });
  else if (pr.checks === "failed") facts.push({ kind: "checks_failed", count: Math.max(work?.failed_checks.length ?? 0, 1) });
  else if (pr.checks === "passing") facts.push({ kind: "checks_passing" });
  if (pr.review === "approved") facts.push({ kind: "approved" });
  else if (pr.review === "changes_requested") facts.push({ kind: "changes" });
  else if (pr.review === "review_required") facts.push({ kind: "review_required" });
  if (pr.state === "merged" || pr.state === "closed" || !work) return facts;
  if (work.merge_state === "dirty") facts.push({ kind: "conflict", base });
  else if (work.merge_state === "behind") facts.push({ kind: "behind", base });
  else if (MERGEABLE.includes(work.merge_state)) facts.push({ kind: "no_conflict", base });
  return facts;
}

/** How one step of `작업 · PR · 검토 · 머지` is drawn (D-36). */
export type StepState = "done" | "active" | "failed" | "empty";

/** The line under a step's label: who worked, the checks, the review, the merge. */
export type StepNote =
  | { kind: "agent"; name: string }
  | { kind: "external" }
  | { kind: "draft" }
  | { kind: "checks_passing" }
  | { kind: "checks_running"; count: number }
  | { kind: "checks_failed"; count: number }
  | { kind: "approved" }
  | { kind: "changes" }
  | { kind: "review_waiting" }
  | { kind: "asked_of_me" }
  | { kind: "no_reviewer" }
  | { kind: "merged" }
  | { kind: "queued"; count: number }
  | { kind: "closed" };

export type PrStep = { id: "work" | "pr" | "review" | "merge"; state: StepState; note: StepNote | null };

/**
 * The card's four steps (PRD sidebar-task-lens D-35, D-36): a pull request
 * has done its work; the PR step runs while it is a draft or its checks run,
 * fails when a check failed; review follows once the PR step is done, and is
 * done when it is approved or GitHub would merge it; merge is done once it
 * merged. `worker` is the product of the agent that worked on it, when one
 * is known.
 */
export function prSteps(pr: PullRequest, worker: string | null, reason: PrReason["kind"] | null): PrStep[] {
  const work = pr.work;
  const who: StepNote | null = worker ? { kind: "agent", name: worker } : work?.mine === false ? { kind: "external" } : null;
  const steps: PrStep[] = [{ id: "work", state: "done", note: who }];
  if (pr.state === "merged") {
    return [...steps, { id: "pr", state: "done", note: pr.checks === "passing" ? { kind: "checks_passing" } : null }, { id: "review", state: "done", note: pr.review === "approved" ? { kind: "approved" } : null }, { id: "merge", state: "done", note: { kind: "merged" } }];
  }
  const closed = pr.state === "closed";
  const running = pr.checks === "pending" || (work?.pending_checks ?? 0) > 0;
  const prStep: PrStep = pr.is_draft
    ? { id: "pr", state: "active", note: { kind: "draft" } }
    : running
      ? { id: "pr", state: "active", note: { kind: "checks_running", count: Math.max(work?.pending_checks ?? 0, 1) } }
      : pr.checks === "failed"
        ? { id: "pr", state: "failed", note: { kind: "checks_failed", count: Math.max(work?.failed_checks.length ?? 0, 1) } }
        : { id: "pr", state: "done", note: pr.checks === "passing" ? { kind: "checks_passing" } : null };
  steps.push(prStep);
  let review: PrStep = { id: "review", state: "empty", note: null };
  if (prStep.state === "done" && !closed) {
    if (pr.review === "changes_requested") review = { id: "review", state: "failed", note: { kind: "changes" } };
    else if (pr.review === "approved") review = { id: "review", state: "done", note: { kind: "approved" } };
    else if (work && MERGEABLE.includes(work.merge_state)) review = { id: "review", state: "done", note: null };
    else if (reason === "assign_reviewer") review = { id: "review", state: "active", note: { kind: "no_reviewer" } };
    else if (work && (work.asked_of_me === "direct" || work.asked_of_me === "team")) review = { id: "review", state: "active", note: { kind: "asked_of_me" } };
    else review = { id: "review", state: "active", note: { kind: "review_waiting" } };
  }
  steps.push(review);
  const queued = work?.queue.state === "at" ? work.queue.position : null;
  steps.push(closed
    ? { id: "merge", state: "empty", note: { kind: "closed" } }
    : queued !== null && review.state === "done"
      ? { id: "merge", state: "active", note: { kind: "queued", count: queued } }
      : { id: "merge", state: "empty", note: null });
  return steps;
}
