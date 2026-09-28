// Handing a pull request to an agent (PRD overview-lenses-prs D-12, D-46):
// the first prompt the dialog opens with, from what the core read of the pull
// request. Pure, so the rule reads and tests on its own; the operator edits
// the prompt before starting, and it is never cut (PRD Risks).

import type { PrFeedback } from "./snapshot";

/**
 * Which pull request and branch to fix, then each failed check by name with
 * its link, then each change request as its reviewer wrote it. A section with
 * nothing in it is left out.
 */
export function delegatePrompt(pr: { number: number; title: string; branch: string }, feedback: Pick<PrFeedback, "failed_checks" | "change_requests">): string {
  const lines = [`PR #${pr.number} (${pr.branch})의 CI 실패와 변경 요청을 고쳐줘: ${pr.title}`];
  if (feedback.failed_checks.length > 0) {
    lines.push("", "실패한 검사:");
    for (const check of feedback.failed_checks) lines.push(`- ${check.name}${check.url ? ` ${check.url}` : ""}`);
  }
  const requests = feedback.change_requests.filter((request) => request.body.trim());
  if (requests.length > 0) {
    lines.push("", "변경 요청:");
    for (const request of requests) lines.push(`- ${request.author ?? "리뷰어"}: ${request.body.trim()}`);
  }
  return lines.join("\n");
}
