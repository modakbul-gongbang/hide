import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProbedPath } from "./host";
import type { CellRow } from "./selection";
import { linkCandidates } from "./terminalLinks";
import { PROBE_CACHE_CAP, PROBE_TTL_MS, clearProbeCache, owningCheckout, pathLookups, probePaths, resolveGroups } from "./terminalLinkProvider";

function row(text: string, width: number): CellRow {
  const cells: CellRow = [...text];
  while (cells.length < width) cells.push(" ");
  return cells;
}

/** A host that knows exactly `files` and `dirs`, each its own physical path. */
function host(files: string[], dirs: string[] = []) {
  return vi.fn(async (paths: string[]): Promise<ProbedPath[]> =>
    paths.map((path) => (files.includes(path) ? { real: path, kind: "file" } : dirs.includes(path) ? { real: path, kind: "directory" } : null)),
  );
}

beforeEach(() => clearProbeCache());

describe("pathLookups", () => {
  it("looks a relative path up under the pane's folder, then the checkout's root", () => {
    expect(pathLookups("src/a.ts", "/w/proj/sub", "/w/proj")).toEqual(["/w/proj/sub/src/a.ts", "/w/proj/src/a.ts"]);
    expect(pathLookups("../b.ts", "/w/proj/sub", "/w/proj")).toEqual(["/w/proj/b.ts", "/w/b.ts"]);
    expect(pathLookups("a.ts", "/w/proj", "/w/proj")).toEqual(["/w/proj/a.ts"]);
  });

  it("takes an absolute or home path as written", () => {
    expect(pathLookups("/tmp/./x/../out.png", "/w", "/w")).toEqual(["/tmp/out.png"]);
    expect(pathLookups("~/notes.md", "/w", "/w")).toEqual(["~/notes.md"]);
  });

  it("has nowhere to look for a relative path without a folder", () => {
    expect(pathLookups("src/a.ts", null, null)).toEqual([]);
  });
});

describe("resolveGroups", () => {
  it("links the longest spelling that exists and drops a token that names nothing", async () => {
    const rows = [row("open web/src/termi", 18), row("nals.ts or nowhere.ts", 30)];
    const probe = host(["/w/web/src/terminals.ts"]);
    const chosen = await resolveGroups(linkCandidates(rows, 1), { cwd: "/w", root: "/w" }, probe);
    expect(chosen.map(({ candidate }) => candidate.text)).toEqual(["web/src/terminals.ts"]);
    expect(chosen[0]!.resolved.found).toEqual({ real: "/w/web/src/terminals.ts", kind: "file" });
    // Every lookup of the row went out in one call.
    expect(probe).toHaveBeenCalledTimes(1);
  });

  it("falls back to the unjoined piece when the joined path does not exist", async () => {
    const rows = [row("see src/a.ts", 12), row("src/b.ts", 12)];
    const chosen = await resolveGroups(linkCandidates(rows, 0), { cwd: "/w", root: "/w" }, host(["/w/src/a.ts", "/w/src/b.ts"]));
    expect(chosen.map(({ candidate }) => candidate.text)).toEqual(["src/a.ts"]);
  });

  it("offers URLs alone where paths cannot be checked: a device pane or a page without the desktop host", async () => {
    const rows = [row("https://example.com src/a.ts", 40)];
    const probe = host(["/w/src/a.ts"]);
    expect((await resolveGroups(linkCandidates(rows, 0), null, probe)).map(({ candidate }) => candidate.text)).toEqual(["https://example.com"]);
    expect((await resolveGroups(linkCandidates(rows, 0), { cwd: "/w", root: "/w" }, null)).map(({ candidate }) => candidate.text)).toEqual(["https://example.com"]);
    expect(probe).not.toHaveBeenCalled();
  });
});

describe("probePaths", () => {
  it("asks once per path within the cache's time and again after it", async () => {
    const probe = host(["/a"]);
    await probePaths(["/a", "/b"], probe, 1_000);
    await probePaths(["/a", "/b"], probe, 1_000 + PROBE_TTL_MS - 1);
    expect(probe).toHaveBeenCalledTimes(1);
    const answers = await probePaths(["/a"], probe, 1_000 + PROBE_TTL_MS);
    expect(probe).toHaveBeenCalledTimes(2);
    expect(answers.get("/a")).toEqual({ real: "/a", kind: "file" });
  });

  it("splits a long request into the host's batches and keeps at most the cap", async () => {
    const probe = host([]);
    const paths = Array.from({ length: PROBE_CACHE_CAP + 10 }, (_, index) => `/p${index}`);
    await probePaths(paths, probe, 0);
    expect(probe.mock.calls.every(([batch]) => batch.length <= 64)).toBe(true);
    // The oldest answers left, so the first path is asked again.
    probe.mockClear();
    await probePaths(["/p0"], probe, 1);
    expect(probe).toHaveBeenCalledTimes(1);
  });

  it("leaves a failed or refused call's paths unanswered, apart from a path that does not exist, and caches nothing from it", async () => {
    const failing = vi.fn(async () => {
      throw new Error("gone");
    });
    const refused = vi.fn(async (): Promise<ProbedPath[]> => []);
    expect((await probePaths(["/a"], failing, 0)).has("/a")).toBe(false);
    expect((await probePaths(["/a"], refused, 0)).has("/a")).toBe(false);
    const probe = host(["/a"]);
    const answers = await probePaths(["/a", "/b"], probe, 1);
    expect(answers.get("/a")).toEqual({ real: "/a", kind: "file" });
    expect(answers.has("/b") && answers.get("/b")).toBeNull();
  });
});

describe("owningCheckout", () => {
  const checkouts = [{ path: "/w/proj" }, { path: "/w/proj/.worktrees/feature" }, { path: "/links/other" }];
  const roots = new Map<string, ProbedPath>([
    ["/w/proj", { real: "/w/proj", kind: "directory" }],
    ["/w/proj/.worktrees/feature", { real: "/w/proj/.worktrees/feature", kind: "directory" }],
    // A checkout registered through a symlink is matched by its physical root.
    ["/links/other", { real: "/real/other", kind: "directory" }],
  ]);

  it("gives a path to the checkout with the longest physical root that holds it", () => {
    expect(owningCheckout("/w/proj/src/a.ts", checkouts, roots)).toEqual({ checkout: checkouts[0], root: "/w/proj" });
    expect(owningCheckout("/w/proj/.worktrees/feature/a.ts", checkouts, roots)?.checkout).toBe(checkouts[1]);
    expect(owningCheckout("/real/other/x.md", checkouts, roots)).toEqual({ checkout: checkouts[2], root: "/real/other" });
    expect(owningCheckout("/w/proj", checkouts, roots)?.checkout).toBe(checkouts[0]);
  });

  it("gives nothing for a path outside every checkout or beside one with a shared prefix", () => {
    expect(owningCheckout("/tmp/out.png", checkouts, roots)).toBeNull();
    expect(owningCheckout("/w/project-two/a.ts", checkouts, roots)).toBeNull();
  });
});
