// The card's rules that are not drawing (PRD work-tab-pr-lens D-06, D-21,
// D-27; B27, B28, B30): the outline of a body, which button leads for a
// case, why Merge cannot be pressed, and where a changed file opens.

import { describe, expect, it } from "vitest";
import { bodyOutline, bodyPreview, confirmHolds, fileOpen, mergeWhy, primaryAction, splitFiles } from "./prCardModel";
import type { PrCard, PullRequest } from "./snapshot";

const pr = (over: Omit<Partial<PullRequest>, "work"> & { work?: Record<string, unknown> }): PullRequest =>
  ({ number: 7, url: "https://github.com/acme/repo/pull/7", head_branch: "feat/x", base_branch: "main", is_draft: false, ...over, work: { merge_state: "clean", pending_checks: 0, ...over.work } }) as unknown as PullRequest;

const card = (merge: Partial<NonNullable<PrCard["merge"]>> | null): PrCard => ({ merge: merge === null ? null : { can_merge: true, methods: ["merge"], queue: false, ...merge } }) as unknown as PrCard;

describe("bodyOutline", () => {
  it("lists the ## headings in order and ignores headings inside fenced code", () => {
    const body = "# Title\n\n## Summary\ntext\n```md\n## Not a heading\n```\n## Test plan ##\n### Deeper\n~~~\n## Also fenced\n~~~\n";
    expect(bodyOutline(body)).toEqual(["Summary", "Test plan"]);
  });
});

describe("bodyPreview", () => {
  it("keeps the first lines without heading marks or blank runs", () => {
    expect(bodyPreview("## Summary\n\n\nOne\nTwo\nThree\nFour")).toBe("Summary\nOne\nTwo");
  });
});

describe("primaryAction", () => {
  it("leads with the button of the case", () => {
    expect(primaryAction("merge")).toBe("merge");
    expect(primaryAction("update")).toBe("update");
    expect(primaryAction("team_review")).toBe("review");
    expect(primaryAction("reply")).toBe("answer");
    expect(primaryAction("check")).toBe("view_pane");
    expect(primaryAction("conflict")).toBe("assign");
    expect(primaryAction("assign_reviewer")).toBe("assign_reviewer");
    expect(primaryAction("queued")).toBe("github");
  });
});

describe("mergeWhy", () => {
  it("is null only when the state is mergeable, permitted, not a draft and no agent is working", () => {
    expect(mergeWhy(pr({}), card({}), false)).toBeNull();
    expect(mergeWhy(pr({ work: { merge_state: "unstable" } }), card({}), false)).toBeNull();
  });

  it("names the first thing in the way", () => {
    expect(mergeWhy(pr({}), null, false)?.key).toBe("unread");
    expect(mergeWhy(pr({}), card({ can_merge: false }), false)?.key).toBe("permission");
    expect(mergeWhy(pr({ is_draft: true }), card({}), false)?.key).toBe("draft");
    expect(mergeWhy(pr({}), card({}), true)?.key).toBe("agent");
    expect(mergeWhy(pr({ work: { merge_state: "dirty" } }), card({}), false)).toMatchObject({ key: "conflict", base: "main" });
    expect(mergeWhy(pr({ work: { merge_state: "blocked", pending_checks: 2 } }), card({}), false)).toMatchObject({ key: "ci", count: 2 });
    expect(mergeWhy(pr({ checks: "failed", work: { merge_state: "blocked", failed_checks: [{ name: "verify", url: null }] } }), card({}), false)).toMatchObject({ key: "failed", count: 1 });
    expect(mergeWhy(pr({ review: "changes_requested", work: { merge_state: "blocked" } }), card({}), false)?.key).toBe("changes");
    expect(mergeWhy(pr({ work: { merge_state: "blocked" } }), card({}), false)?.key).toBe("reviewer");
    expect(mergeWhy(pr({ work: { merge_state: "unknown" } }), card({}), false)?.key).toBe("unknown");
  });
});

describe("confirmHolds", () => {
  const merge = { action: "merge", head: "a".repeat(40), method: "squash", queue: false } as const;
  const same = { head: "a".repeat(40), queue: false, method: "squash", mergeBlocked: false } as const;

  it("holds while the card shows what the confirmation named", () => {
    expect(confirmHolds(merge, same)).toBe(true);
    expect(confirmHolds({ ...merge, action: "update", method: null }, { ...same, method: "merge", mergeBlocked: true })).toBe(true);
  });

  it("closes when the head, the queue, the method or Merge's permission moved", () => {
    expect(confirmHolds(merge, { ...same, head: "b".repeat(40) })).toBe(false);
    expect(confirmHolds(merge, { ...same, queue: true })).toBe(false);
    expect(confirmHolds(merge, { ...same, method: "rebase" })).toBe(false);
    expect(confirmHolds(merge, { ...same, mergeBlocked: true })).toBe(false);
    expect(confirmHolds({ ...merge, action: "update", method: null }, { ...same, head: null })).toBe(false);
  });
});

describe("fileOpen", () => {
  const committed = [{ path: "/work/repo/src/app.ts" }];
  const input = { pr: { url: "https://github.com/acme/repo/pull/7", head_branch: "feat/x" }, committed, file: "src/app.ts" };

  it("opens the Hide diff only in a checkout of the branch whose committed changes hold the file", () => {
    expect(fileOpen({ ...input, front: { branch: "feat/x", path: "/work/repo" } })).toEqual({ kind: "diff", path: "/work/repo/src/app.ts" });
  });

  it("goes to GitHub's files page otherwise", () => {
    const github = { kind: "github", url: "https://github.com/acme/repo/pull/7/files" };
    expect(fileOpen({ ...input, front: { branch: "main", path: "/work/repo" } })).toEqual(github);
    expect(fileOpen({ ...input, front: null })).toEqual(github);
    expect(fileOpen({ ...input, front: { branch: "feat/x", path: "/work/repo" }, committed: null })).toEqual(github);
    expect(fileOpen({ ...input, front: { branch: "feat/x", path: "/work/repo" }, file: "src/other.ts" })).toEqual(github);
  });
});

describe("splitFiles", () => {
  it("folds the generated files into a count", () => {
    const files = [{ path: "a.ts", generated: false }, { path: "Cargo.lock", generated: true }, { path: "dist/a.js", generated: true }] as NonNullable<PrCard["files"]>;
    expect(splitFiles(files)).toEqual({ shown: [files[0]], generated: 2 });
    expect(splitFiles(null)).toEqual({ shown: [], generated: 0 });
  });
});
