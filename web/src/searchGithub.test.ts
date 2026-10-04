import { describe, expect, it } from "vitest";
import { RICH } from "./gallery/cmdkSceneData";
import { githubEntries } from "./search";
import { githubRow, hasGithubProject, lastReadWords, ownAnswer, projectRead, projectToRead, startsSearch } from "./searchGithub";
import type { Checkout, GithubSearch, SnapshotRest } from "./snapshot";

const WORKSPACE = RICH.navigator!.workspaces![0]!;
const NEVER_READ = (() => {
  const rest = structuredClone(RICH) as SnapshotRest;
  for (const checkout of rest.navigator!.workspaces![0]!.checkouts) delete (checkout as { github?: unknown }).github;
  return rest;
})();
const front = (rest: SnapshotRest): Checkout => rest.navigator!.workspaces![0]!.checkouts[1]!;

describe("the one project read ⌘K asks for (PRD cmdk-navigation B8, B10, D-12)", () => {
  it("reads the local project in front once, when nothing has answered it", () => {
    const asked = new Set<string>();
    expect(projectToRead(NEVER_READ, front(NEVER_READ), asked)?.id).toBe("w1");
    asked.add("w1");
    expect(projectToRead(NEVER_READ, front(NEVER_READ), asked)).toBeNull();
  });

  it("leaves a project the core has answered alone", () => {
    expect(projectToRead(RICH, front(RICH), new Set())).toBeNull();
  });

  it("never reads a device's project or an Overview-less front", () => {
    const onMini = structuredClone(NEVER_READ) as SnapshotRest;
    onMini.navigator!.focused_device_id = "mini";
    expect(projectToRead(onMini, front(onMini), new Set())).toBeNull();
    expect(projectToRead(NEVER_READ, null, new Set())).toBeNull();
  });
});

describe("a project's read as its checkouts show it (B8, B9)", () => {
  const now = 1_000_000 + 5 * 60_000;
  it("is nothing for a project nobody asked for, even while the source says reading", () => {
    const rest = structuredClone(NEVER_READ) as SnapshotRest;
    rest.navigator!.workspaces![0]!.tasks!.source!.reading = true;
    expect(projectRead(rest.navigator!.workspaces![0]!, new Set(), now)).toBeNull();
  });

  it("reads while ⌘K's request has no answer yet", () => {
    expect(projectRead(NEVER_READ.navigator!.workspaces![0]!, new Set(["w1"]), now)).toEqual({ state: "reading" });
    expect(projectRead(WORKSPACE, new Set(["w1"]), now)).toBeNull();
  });

  it("says a failed read with the last value's age and no reason (B9)", () => {
    const failed = structuredClone(RICH) as SnapshotRest;
    failed.navigator!.workspaces![0]!.checkouts[0]!.github = { failure_category: "network or rate limit", available: false, loading: false, stale: true, last_success_at_unix_ms: 1_000_000, unavailable_reason: "gh timed out" };
    const read = projectRead(failed.navigator!.workspaces![0]!, new Set(["w1"]), now);
    expect(read).toEqual({ state: "failed", tooltip: "GitHub 읽기 실패 · 5분 전 값 · 이유는 로그에" });
    expect(JSON.stringify(read)).not.toContain("timed out");
  });

  it("says when its pull requests and issues were read", () => {
    expect(lastReadWords(WORKSPACE, now)).toBe("5분 전 읽음");
  });
});

describe("the explicit GitHub search (B16-B19)", () => {
  const answer = (patch: Partial<GithubSearch>): GithubSearch => ({ request_id: "r1", query: "sandbox", phase: "ready", results: [], message: null, ...patch });

  it("offers the row only while this Mac has a GitHub project", () => {
    expect(hasGithubProject(RICH)).toBe(true);
    expect(hasGithubProject({ navigator: { workspaces: [] } } as unknown as SnapshotRest)).toBe(false);
  });

  it("words the row for each state", () => {
    expect(githubRow("sandbox", null)).toEqual({ state: "idle", label: 'GitHub에서 "sandbox" 검색' });
    expect(githubRow("sandbox", answer({ phase: "working" })).state).toBe("working");
    expect(githubRow("sandbox", answer({ phase: "failed" }))).toEqual({ state: "failed", label: "GitHub 검색 실패 · 다시 시도" });
    expect(githubRow("sandbox", answer({}))).toEqual({ state: "none", label: "GitHub에도 없음" });
    expect(githubRow("sandbox", answer({ message: "저장소 25개 중 20개만 검색했습니다." })).label).toBe("찾은 결과 없음 · 일부 저장소만 검색");
  });

  it("does not start the same query again while it runs, and does after a failure (B19)", () => {
    expect(startsSearch(githubRow("sandbox", answer({ phase: "working" })))).toBe(false);
    expect(startsSearch(githubRow("sandbox", answer({ phase: "failed" })))).toBe(true);
    expect(startsSearch(githubRow("sandbox", null))).toBe(true);
  });

  it("drops an answer for another request or another query, so changing the query clears the results (B19)", () => {
    const asked = { requestId: "r1", query: "sandbox" };
    expect(ownAnswer(answer({}), asked, "sandbox")).not.toBeNull();
    expect(ownAnswer(answer({}), asked, "sandbox2")).toBeNull();
    expect(ownAnswer(answer({ request_id: "r2" }), asked, "sandbox")).toBeNull();
    expect(ownAnswer(answer({}), null, "sandbox")).toBeNull();
  });

  it("shows a result a held row already is only once (B17)", () => {
    const results = [
      { kind: "pr" as const, repository: "acme/herdr-ide", number: 275, title: "Surface mailbox sandbox refusals", state: "open", url: "https://github.com/acme/herdr-ide/pull/275" },
      { kind: "pr" as const, repository: "acme/herdr-ide", number: 12, title: "Old", state: "merged", url: "https://github.com/acme/herdr-ide/pull/12" },
    ];
    const held = [{ url: "https://github.com/acme/herdr-ide/pull/275" }] as unknown as Parameters<typeof githubEntries>[1];
    const shown = githubEntries(results, held);
    expect(shown.map((entry) => entry.id)).toEqual(["github:pr:acme/herdr-ide#12"]);
    expect(shown[0]).toMatchObject({ external: true, kind: "pr", url: "https://github.com/acme/herdr-ide/pull/12" });
  });
});
