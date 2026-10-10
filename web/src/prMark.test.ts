import fs from "node:fs";
import path from "node:path";
import { GitMergeIcon, GitPullRequestClosedIcon, GitPullRequestDraftIcon, GitPullRequestIcon } from "lucide-react";
import { describe, expect, it } from "vitest";
import { legacyAgentRow } from "../test/legacyAgentRow";
import { initializeInterfaceI18n } from "./i18n/instance";
import { agentMark, badgeWord, checksWord, PR_LOOK, reviewWord, staleLabel, staleness } from "./prMark";
import type { AgentPullRequest, AgentRow, PullRequest } from "./snapshot";

// The one PR mark (docs/status-model.md, Pull request visual states): the
// palette the operator approved on 2026-10-10, one token per state.

const t = initializeInterfaceI18n("en").getFixedT(null, "translation");
const ko = initializeInterfaceI18n("ko").getFixedT(null, "translation");

describe("the PR look", () => {
  it("has a look for every state the core sends, in the core's order", () => {
    const contract = JSON.parse(fs.readFileSync(path.resolve(__dirname, "../../contracts/snapshot-wire-enums.json"), "utf8"));
    expect(Object.keys(PR_LOOK)).toEqual(contract.pr_state);
  });

  it("draws failed red, pending amber, mergeable green, merged purple, draft grey and closed dim, each in its own token", () => {
    expect(Object.fromEntries(Object.entries(PR_LOOK).map(([state, look]) => [state, look.tone]))).toEqual({
      failed: "text-pr-failed",
      pending: "text-pr-pending",
      mergeable: "text-pr-mergeable",
      draft: "text-pr-draft",
      merged: "text-pr-merged",
      closed: "text-pr-closed",
    });
  });

  it("draws a merge for a merged pull request, a draft and a closed one in their own shapes, and the pull request icon for the rest", () => {
    expect([PR_LOOK.merged.icon, PR_LOOK.draft.icon, PR_LOOK.closed.icon]).toEqual([GitMergeIcon, GitPullRequestDraftIcon, GitPullRequestClosedIcon]);
    expect([PR_LOOK.failed.icon, PR_LOOK.pending.icon, PR_LOOK.mergeable.icon]).toEqual([GitPullRequestIcon, GitPullRequestIcon, GitPullRequestIcon]);
  });
});

describe("the card words", () => {
  it("reads a change request red as a failed check, an approval green, and a review still required quiet", () => {
    expect(reviewWord("changes_requested")).toEqual({ key: "board.review.changes", tone: "text-pr-failed" });
    expect(reviewWord("approved")).toEqual({ key: "board.review.approved", tone: "text-pr-mergeable" });
    expect(reviewWord("review_required")?.tone).toBe("text-muted-foreground");
    expect(reviewWord(null)).toBeNull();
  });

  it("names checks in the colour of the state they lead to, and says nothing until GitHub read them", () => {
    expect([checksWord("passing")?.tone, checksWord("failed")?.tone, checksWord("pending")?.tone]).toEqual(["text-pr-mergeable", "text-pr-failed", "text-pr-pending"]);
    expect([checksWord("none"), checksWord("unknown"), checksWord(undefined)]).toEqual([null, null, null]);
  });

  it("names the lifecycle on the badge, or the review decision under review with Draft beside a draft, in the interface language", () => {
    const base: PullRequest = { number: 1, title: "", url: "", badge: "open", review: null, is_draft: false, state: "pending" };
    expect(badgeWord({ ...base, badge: "merged" }, t)).toEqual({ label: "Merged", draft: false });
    expect(badgeWord({ ...base, badge: "closed" }, t)).toEqual({ label: "Closed", draft: false });
    expect(badgeWord(base, t)).toEqual({ label: "Open", draft: false });
    expect(badgeWord({ ...base, is_draft: true }, t)).toEqual({ label: "Draft", draft: false });
    expect(badgeWord({ ...base, badge: "review", review: "changes_requested" }, t)).toEqual({ label: "Changes requested", draft: false });
    expect(badgeWord({ ...base, badge: "review", review: "approved", is_draft: true }, t)).toEqual({ label: "Approved", draft: true });
    expect(badgeWord({ ...base, badge: "merged" }, ko).label).toBe("머지됨");
  });
});

describe("staleness", () => {
  it("dims only once GitHub could not be read again, and says when it last was", () => {
    expect(staleness(null)).toBeUndefined();
    const read = { failure_category: null, available: true, loading: false, stale: false, last_success_at_unix_ms: 1_800_000_000_000, unavailable_reason: null };
    expect(staleness(read)).toEqual({ stale: false, lastRead: 1_800_000_000_000 });
    expect(staleLabel(staleness(read), t, "en")).toBeNull();
    expect(staleLabel(staleness({ ...read, stale: true }), t, "en")).toMatch(/^GitHub last read .+ · cannot read now$/);
    expect(staleLabel({ stale: true, lastRead: null }, t, "en")).toBe("GitHub last read - · cannot read now");
  });
});

describe("an agent row's mark", () => {
  const pull = (number: number): AgentPullRequest => ({ number, title: `PR ${number}`, url: `https://github.com/acme/project/pull/${number}`, badge: "open", is_draft: false, checks: "passing", head_branch: `b${number}`, closing_issues: [], live: true, duty: false, created: true, settled_at_unix_ms: null });
  const row = (pr: AgentRow["state"]["pr"], pulls: AgentPullRequest[]): AgentRow => {
    const agent = legacyAgentRow({ id: "a", pane_id: "a", identity_label: "a", agent_kind: "claude", symbol: "●", group: "working", status_code: "working", changed_at_unix_ms: null, emphasized: false, unread: false, demand: "none", activity: "working", last_activity: "0" });
    return { ...agent, state: { ...agent.state, pr }, request: { verb: "working" as const, verb_since_unix_ms: 0, request: null, later_by: null, reply: null, pull_requests: pulls } };
  };

  it("stands for the worst PR the core ordered first, with how many others", () => {
    const agent = row({ worst: "failed", count: 2, worst_count: 1, pulls: [{ index: 1, state: "failed" }, { index: 0, state: "pending" }] }, [pull(923), pull(924)]);
    expect(agentMark(agent)).toEqual({ state: "failed", number: 924, more: 1 });
  });

  it("draws nothing for a row without its own PR", () => {
    expect(agentMark(row(null, []))).toBeNull();
  });
});
