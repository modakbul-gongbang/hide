// The rules of a pull request's decision card that are not drawing (PRD
// work-tab-pr-lens D-06, D-21, D-27): the body's outline, which button is
// primary for a case, why Merge is not available, and where a changed file
// opens. The core decided the case (`reason.kind`) and read the card's parts;
// nothing here re-derives a group.

import type { PrReason } from "./agentScope";
import type { PrCard, PrMergeMethod, PullRequest } from "./snapshot";

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
export type MergeWhy = { key: "ci" | "agent" | "conflict" | "reviewer" | "blocked" | "draft" | "permission" | "unknown" | "unread"; count: number; base: string };

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
