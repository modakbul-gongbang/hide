import { describe, expect, it } from "vitest";
import { expandedUnderRoot, WATCH_CAP, watchedFolders, watchedSet } from "./watch";

describe("watched folders", () => {
  it("keeps the root first and ignores another checkout's folders", () => {
    expect(watchedFolders("/repo", ["/repo/a", "/repo/b", "/other/c"])).toEqual(["/repo", "/repo/a", "/repo/b"]);
  });

  it("releases the least recently expanded folders past the cap", () => {
    const expanded = Array.from({ length: WATCH_CAP + 5 }, (_, index) => `/repo/f${index}`);
    const watched = watchedFolders("/repo", expanded);
    expect(watched).toHaveLength(WATCH_CAP);
    expect(watched[0]).toBe("/repo");
    expect(watched[watched.length - 1]).toBe(`/repo/f${WATCH_CAP + 4}`);
    expect(watched).not.toContain("/repo/f0");
  });

  it("lists the root once and filters by the checkout prefix", () => {
    expect(expandedUnderRoot("/repo", ["/repo", "/repo/a", "/repo-two/b"])).toEqual(["/repo", "/repo/a"]);
    expect(watchedSet("/repo", ["/repo/a"]).has("/repo/a")).toBe(true);
    expect(watchedSet("/repo", ["/repo/a"]).has("/repo/b")).toBe(false);
  });
});
