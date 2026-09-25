import { describe, expect, it } from "vitest";
import { conversationTurns, detailState, filterSessions, listState, naming, sessionCheckout, sessionTime } from "./sessions";
import type { ArchiveDetail, ProjectSessions, SessionRow, Workspace } from "./snapshot";

const row = (id: string, provider: "claude" | "codex", fields: Partial<SessionRow> = {}): SessionRow => ({
  id,
  provider,
  provider_label: provider === "claude" ? "Claude Code" : "Codex",
  locator: `/home/.${provider}/${id}.jsonl`,
  checkout_path: "/p/app",
  first_human_request: null,
  started_at_unix_ms: null,
  updated_at_unix_ms: 1,
  title: null,
  unavailable_reason: null,
  ...fields,
});

const named = (fields: Partial<ProjectSessions> = {}): ProjectSessions => ({
  device_id: "local",
  workspace_id: "workspace:a",
  unavailable_reason: null,
  loading: false,
  failure: null,
  rows: [],
  detail: null,
  ...fields,
});

const project = { id: "workspace:a", deviceId: "local" };

describe("narrowing a Project's sessions", () => {
  const rows = [
    row("one", "claude", { first_human_request: "배포 스크립트 정리 request" }),
    row("two", "codex", { title: "Fix the Login flow" }),
    row("three", "codex", { checkout_path: "/p/app-feature" }),
  ];

  it("keeps the core's order and matches the request, the title or the folder regardless of case", () => {
    expect(filterSessions(rows, "all", "").map((r) => r.id)).toEqual(["one", "two", "three"]);
    expect(filterSessions(rows, "all", "  LOGIN ").map((r) => r.id)).toEqual(["two"]);
    expect(filterSessions(rows, "all", "배포").map((r) => r.id)).toEqual(["one"]);
    expect(filterSessions(rows, "all", "feature").map((r) => r.id)).toEqual(["three"]);
  });

  it("combines the provider with the search", () => {
    expect(filterSessions(rows, "codex", "").map((r) => r.id)).toEqual(["two", "three"]);
    expect(filterSessions(rows, "claude", "login")).toEqual([]);
  });
});

describe("where a session ran", () => {
  const workspace = {
    checkouts: [
      { path: "/p/app", branch: "main", label: "app" },
      { path: "/p/app/.worktrees/feature/", branch: null, label: "feature" },
    ],
  } as unknown as Workspace;

  it("names the longest checkout that holds the session's folder", () => {
    expect(sessionCheckout(row("a", "claude", { checkout_path: "/p/app/src" }), workspace).label).toBe("main");
    expect(sessionCheckout(row("b", "claude", { checkout_path: "/p/app/.worktrees/feature" }), workspace).label).toBe("feature");
  });

  it("falls back to the folder's own name for a checkout that is gone, not to a neighbour with a shared prefix", () => {
    expect(sessionCheckout(row("c", "claude", { checkout_path: "/p/app-old" }), workspace)).toEqual({ label: "app-old", path: "/p/app-old" });
  });

  it("gives no time a session did not carry", () => {
    expect(sessionTime(0)).toBeNull();
    expect(sessionTime(null)).toBeNull();
    expect(sessionTime(Date.UTC(2020, 0, 2, 3, 4), new Date(Date.UTC(2026, 0, 1)), "en-US")).toContain("2020");
  });
});

describe("which Project the core has named", () => {
  it("is this screen's only when both the Project and its device match", () => {
    expect(naming(named(), project, false)).toBe("ours");
    // A device folder at the same path has the same id.
    expect(naming(named({ device_id: "mini" }), project, false)).toBe("pending");
  });

  it("is replaced only after this screen had it, and pending while nothing is named", () => {
    const other = named({ workspace_id: "workspace:b" });
    expect(naming(other, project, false)).toBe("pending");
    expect(naming(other, project, true)).toBe("replaced");
    expect(naming(null, project, true)).toBe("pending");
  });
});

describe("the list place", () => {
  it("shows the device reason before anything it could not read", () => {
    expect(listState(named({ unavailable_reason: "Sessions on mini are not available here.", failure: "x" }), "ours", "all", "")).toEqual({
      kind: "unavailable",
      reason: "Sessions on mini are not available here.",
    });
    expect(listState(named({ failure: "The session folder could not be read." }), "ours", "all", "")).toEqual({ kind: "failed", reason: "The session folder could not be read." });
  });

  it("tells none yet from none matching, and keeps the rows it has while it reads again", () => {
    expect(listState(named({ loading: true }), "ours", "all", "")).toEqual({ kind: "loading" });
    expect(listState(named(), "ours", "all", "")).toEqual({ kind: "empty" });
    const rows = [row("one", "claude")];
    expect(listState(named({ rows }), "ours", "codex", "")).toEqual({ kind: "no_match" });
    expect(listState(named({ rows, loading: true }), "ours", "all", "")).toEqual({ kind: "rows", rows });
  });

  it("shows nothing of another Project's history", () => {
    const other = named({ workspace_id: "workspace:b", rows: [row("theirs", "claude")] });
    expect(listState(other, "pending", "all", "")).toEqual({ kind: "loading" });
    expect(listState(other, "replaced", "all", "")).toEqual({ kind: "replaced" });
    expect(detailState(named({ detail: { session_id: "theirs", locator: "", loading: false, failure: null, archive: null } }), "replaced")).toEqual({ kind: "none" });
  });
});

describe("the detail place", () => {
  const archive: ArchiveDetail = {
    id: "one",
    kind: "session",
    title: "t",
    provider: "Claude Code",
    unavailable_reason: null,
    events: [
      { role: "user", kind: "message", at_unix_ms: 1, text: "배포 스크립트 정리" },
      { role: "user", kind: "injected", at_unix_ms: 2, text: "Project Memory items" },
      { role: "system", kind: "message", at_unix_ms: 3, text: "hidden" },
      { role: "assistant", kind: "message", at_unix_ms: 4, text: "Done." },
    ],
  };

  it("reads, fails with its reason, or shows the conversation without injected context", () => {
    const detail = { session_id: "one", locator: "/l", loading: true, failure: null, archive: null };
    expect(detailState(named({ detail }), "ours").kind).toBe("loading");
    expect(detailState(named({ detail: { ...detail, loading: false, failure: "The session file is missing." } }), "ours")).toMatchObject({
      kind: "failed",
      reason: "The session file is missing.",
    });
    expect(detailState(named({ detail: { ...detail, loading: false, archive } }), "ours").kind).toBe("open");
    expect(conversationTurns(archive).map((turn) => turn.text)).toEqual(["배포 스크립트 정리", "Done."]);
  });
});
