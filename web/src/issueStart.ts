// Starting work from an issue (the Start dialog, `IssueDialogs.tsx`): the
// worktree name the dialog opens with before the background AI answers, and
// the first prompt it hands the agent. Pure, so the rules read and test on
// their own; the AI's name comes from the core (`worktree_name_suggest`) with
// the same prefix, so a name keeps the issue's number whoever wrote it.

import type { TFunction } from "i18next";
import type { Task } from "./snapshot";

/** A branch-safe slug of a title: lowercase ASCII words joined by hyphens, cut at a word. The core's `branch_slug` is the same rule. */
export function branchSlug(text: string, limit = 48): string {
  let slug = "";
  for (const character of text.toLowerCase()) {
    if (/[a-z0-9]/.test(character)) slug += character;
    else if (slug && !slug.endsWith("-")) slug += "-";
  }
  slug = slug.replace(/^-+|-+$/g, "");
  if (slug.length > limit) {
    slug = slug.slice(0, limit);
    const cut = slug.lastIndexOf("-");
    if (cut > 0) slug = slug.slice(0, cut);
  }
  return slug.replace(/^-+|-+$/g, "");
}

/** The issue's number as the source shows it: `42` for `#42` or `owner/repo#42`, `3` for `L-3`; null when the task has no id. */
function issueNumber(task: Task): string | null {
  const match = task.id?.match(/(\d+)$/);
  return match?.[1] ?? null;
}

/** What every name for this issue starts with: `42-` for a GitHub issue, `L-3-` for a local one. */
export function namePrefix(task: Task): string {
  const number = issueNumber(task);
  if (!number) return "";
  return task.source === "local" ? `L-${number}-` : `${number}-`;
}

/**
 * The name the dialog opens with: the prefix and the title's English words
 * (`42-fix-the-login-flow`); a title with none, such as a Korean
 * one, gives `issue-42` or `L-3`.
 */
export function defaultWorktreeName(task: Task): string {
  const prefix = namePrefix(task);
  const slug = branchSlug(task.title);
  if (slug) return `${prefix}${slug}`;
  const number = issueNumber(task);
  if (!number) return "work";
  return task.source === "local" ? `L-${number}` : `issue-${number}`;
}

/**
 * The agent's first prompt: which issue to solve, its title and body, and,
 * for a GitHub issue when the project row's Issue source menu asks for it, to open a pull
 * request that closes it. The operator edits it before starting.
 */
export function firstPrompt(task: Task, body: string | null, closes: boolean, t: TFunction<"translation">): string {
  const id = task.id ?? task.title;
  const lines: string[] = [t(task.source === "local" ? "issue.prompt.local" : "issue.prompt.github", { id, title: task.title })];
  const text = body?.trim();
  if (text) lines.push("", text);
  if (closes && task.source === "github" && task.id) lines.push("", t("issue.prompt.closes", { id: task.id }));
  return lines.join("\n");
}
