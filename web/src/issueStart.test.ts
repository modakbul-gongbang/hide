// The Start dialog's first name and first prompt for an issue.

import { describe, expect, it } from "vitest";
import { initializeInterfaceI18n } from "./i18n/instance";
import { branchSlug, defaultWorktreeName, firstPrompt, namePrefix } from "./issueStart";
import type { Task } from "./snapshot";

const t = initializeInterfaceI18n("ko").getFixedT(null, "translation");
const english = initializeInterfaceI18n("en").getFixedT(null, "translation");

const github: Task = { key: "github:acme/hide#192", source: "github", id: "#192", url: null, title: "hided has no SIGTERM handler: AI children are torn down", open: true };
const local: Task = { key: "local:/p#3", source: "local", id: "L-3", url: null, title: "Overview 재설계 v4 보드", open: true };

describe("the worktree name an issue starts with", () => {
  it("keeps the issue's number first, so the branch still names the issue whoever renames the rest", () => {
    expect(namePrefix(github)).toBe("192-");
    expect(namePrefix(local)).toBe("L-3-");
    expect(defaultWorktreeName(github)).toBe("192-hided-has-no-sigterm-handler-ai-children-are");
    expect(defaultWorktreeName(local)).toBe("L-3-overview-v4");
    expect(defaultWorktreeName({ ...github, title: "사이드바 폭 조절" })).toBe("issue-192");
    expect(defaultWorktreeName({ ...local, title: "사이드바 폭 조절" })).toBe("L-3");
  });

  it("slugs the way the core does: lowercase ASCII words, cut at a word", () => {
    expect(branchSlug("  Fix: the --weird__ title!! ")).toBe("fix-the-weird-title");
    expect(branchSlug("a".repeat(30) + " " + "b".repeat(30))).toBe("a".repeat(30));
  });
});

describe("the first prompt", () => {
  it("names the issue, carries its body, and asks for a closing pull request only for a GitHub issue when asked to", () => {
    expect(firstPrompt(github, "Steps:\n1. quit", true, t)).toBe(
      "Issue #192를 해결해줘: hided has no SIGTERM handler: AI children are torn down\n\nSteps:\n1. quit\n\n완료되면 이 이슈를 닫는 PR을 열어줘 (PR 본문에 Closes #192).",
    );
    expect(firstPrompt(github, "", false, t)).toBe("Issue #192를 해결해줘: hided has no SIGTERM handler: AI children are torn down");
    expect(firstPrompt(local, "본문", true, t)).toBe("로컬 이슈 L-3를 해결해줘: Overview 재설계 v4 보드\n\n본문");
  });

  it("is written in the interface language, with the issue's id and title as data", () => {
    expect(firstPrompt(github, "Steps", true, english)).toBe(
      "Solve issue #192: hided has no SIGTERM handler: AI children are torn down\n\nSteps\n\nWhen done, open a PR that closes this issue (put Closes #192 in the PR body).",
    );
    expect(firstPrompt(local, null, true, english)).toBe("Solve local issue L-3: Overview 재설계 v4 보드");
  });
});
