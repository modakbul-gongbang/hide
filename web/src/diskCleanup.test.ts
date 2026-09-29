// The disk cleanup sheet's rules (PRD disk-layers): the selection math, the
// filters, the folded row, the footer and the entrance numbers, read against
// small fixtures. Expected answers are the PRD's Behaviors (B2, B4, B10-B16, B22).

import { describe, expect, it } from "vitest";
import {
  EMPTY_SELECTION,
  LOW_FREE_BYTES,
  SMALL_CHECKOUT_BYTES,
  allocatedTotal,
  bundleRefs,
  bundleState,
  filterCounts,
  footerOf,
  freedFree,
  layoutRows,
  lowFree,
  needsConfirm,
  planOf,
  pruneSelection,
  reclaimable,
  removingElsewhere,
  resultLines,
  sheetModel,
  toggleBundle,
  toggleCell,
  visibleRows,
  type SheetRow,
} from "./diskCleanup";
import type { Checkout, CleanupRow, DiskCleanup, DiskLayers, Workspace } from "./snapshot";

const GB = 1024 ** 3;
const MB = 1024 ** 2;

function layers(build: number, deps: number, other = 0): DiskLayers {
  const cell = (bytes: number) => ({ bytes, folders: bytes > 0 ? 1 : 0, largest_name: bytes > 0 ? "x" : null });
  return { build_cache: cell(build), dependencies: cell(deps), other: cell(other), source_bytes: 10 * MB };
}

type Options = { main?: boolean; merged?: boolean; working?: number; build?: number; deps?: number; other?: number; unmeasured?: boolean; unavailable?: boolean };

function checkout(name: string, options: Options = {}): Checkout {
  const build = options.build ?? 2 * GB;
  const deps = options.deps ?? 300 * MB;
  const total = build + deps + (options.other ?? 0) + 10 * MB;
  const disk = options.unmeasured
    ? { total_bytes: null, unavailable_reason: null }
    : options.unavailable
      ? { total_bytes: null, unavailable_reason: "limit" }
      : { total_bytes: total, unavailable_reason: null, layers: layers(build, deps, options.other ?? 0) };
  return {
    id: name,
    workspace_id: "p",
    label: name,
    path: `/r/${name}`,
    branch: name,
    purpose: null,
    is_worktree: !options.main,
    exists: true,
    has_panes: false,
    agent_summary: { working: options.working ?? 0 } as Checkout["agent_summary"],
    worktree: { is_main: options.main ?? false, merged: options.merged ?? null, disk } as Checkout["worktree"],
    pull_request: null,
    tabs: [],
    active_tab_id: null,
    strip: [],
    next_tab_label: "Tab 2",
  } as unknown as Checkout;
}

function core(path: string, extra: Partial<CleanupRow> = {}): CleanupRow {
  return { path, branch: null, head: null, is_main: false, exclusion: null, exclusion_code: null, exclusion_count: null, in_use: null, result: null, message: null, ...extra };
}

function workspace(checkouts: Checkout[], cleanup: Partial<DiskCleanup> | null, rows: Record<string, Partial<CleanupRow>> = {}, id = "p"): Workspace {
  const review: DiskCleanup | null = cleanup
    ? {
        id: 1,
        workspace_id: id,
        repository_root: "/r",
        phase: "review",
        main_head: null,
        message: null,
        usage_error: null,
        free_bytes: null,
        progress: null,
        cell_results: [],
        free_before: null,
        free_after: null,
        ...cleanup,
        rows: checkouts.map((c) => core(c.path, { is_main: c.worktree?.is_main ?? false, exclusion_code: c.worktree?.is_main ? "main" : null, in_use: (c.agent_summary?.working ?? 0) > 0 ? { code: "agent_working", name: null, port: null } : null, ...rows[c.label] })),
      }
    : null;
  return { id, label: id, path: "/r", device_id: "local", is_git: true, registered: true, temporary: false, pinned: false, checkouts, inactive_checkouts: { expanded: false, checkout_ids: [] }, cleanup: review } as Workspace;
}

function ready(checkouts: Checkout[], rows: Record<string, Partial<CleanupRow>> = {}): SheetRow[] {
  return sheetModel(workspace(checkouts, {}, rows), false).rows;
}

const merged = { merged: true };

describe("filters (B12, D-22)", () => {
  const rows = ready([
    checkout("main", { main: true }),
    checkout("done-one", merged),
    checkout("busy", { ...merged, working: 1 }),
    checkout("idle"),
  ], { idle: {}, "done-one": {} });

  it("splits into finished, resting and working, and main is never finished", () => {
    expect(filterCounts(rows)).toEqual({ all: 4, done: 1, resting: 2, working: 1 });
    expect(visibleRows(rows, "done").map((r) => r.label)).toEqual(["done-one"]);
    expect(visibleRows(rows, "resting").map((r) => r.label).sort()).toEqual(["idle", "main"]);
  });

  it("puts a checkout in use in Working even when its pull request finished", () => {
    expect(rows.find((r) => r.label === "busy")?.bucket).toBe("working");
  });

  it("takes an in-use signal from the core review, not only from the agent", () => {
    const withPort = ready([checkout("srv")], { srv: { in_use: { code: "port", name: null, port: 5173 } } });
    expect(withPort[0]?.bucket).toBe("working");
    expect(withPort[0]?.inUse).toBe("포트 5173 서버");
  });
});

describe("layout (B4, D-21)", () => {
  it("puts main first, the rest by size, and folds checkouts under 1 GB", () => {
    const rows = ready([
      checkout("main", { main: true, build: 1 * GB }),
      checkout("small-a", { build: 100 * MB, deps: 0 }),
      checkout("large", { build: 9 * GB }),
      checkout("mid", { build: 3 * GB }),
      checkout("small-b", { build: 200 * MB, deps: 0 }),
    ]);
    const layout = layoutRows(rows);
    expect(layout.main?.label).toBe("main");
    expect(layout.big.map((r) => r.label)).toEqual(["large", "mid"]);
    expect(layout.small.map((r) => r.label)).toEqual(["small-b", "small-a"]);
    expect(layout.smallBytes).toBe(layout.small.reduce((sum, r) => sum + (r.total ?? 0), 0));
    expect(layout.small.every((r) => (r.total ?? 0) < SMALL_CHECKOUT_BYTES)).toBe(true);
  });

  it("keeps an unmeasured checkout out of the fold", () => {
    const layout = layoutRows(ready([checkout("main", { main: true }), checkout("later", { unmeasured: true })]));
    expect(layout.big.map((r) => r.label)).toEqual(["later"]);
    expect(layout.small).toEqual([]);
  });
});

describe("cell availability (B5, B6, B14, B15, B25)", () => {
  it("is a skeleton with nothing selectable while the review has not answered", () => {
    const pending = sheetModel(workspace([checkout("a")], { phase: "loading" }), false);
    expect(pending.state).toBe("pending");
    expect(pending.rows[0]?.cache.build_cache.selectable).toBe(false);
  });

  it("leaves an unmeasured row unselectable and an unreadable row named", () => {
    const model = ready([checkout("wait", { unmeasured: true }), checkout("big", { unavailable: true })]);
    expect(model.map((r) => r.measure)).toEqual(["pending", "unavailable"]);
    expect(model.every((r) => !r.cache.build_cache.selectable)).toBe(true);
    expect(model[1]?.cache.build_cache.why).toBe("크기를 재지 못함");
  });

  it("blocks a checkout in use with the reason and lets an open pane alone through, main included", () => {
    const rows = ready([checkout("main", { main: true }), checkout("run")], { run: { in_use: { code: "process", name: "cargo", port: null } } });
    expect(rows[0]?.cache.build_cache.selectable).toBe(true);
    expect(rows[1]?.cache.build_cache.selectable).toBe(false);
    expect(rows[1]?.cache.build_cache.why).toBe("터미널에서 cargo 실행 중");
    expect(rows[1]?.worktree?.why).toBe("터미널에서 cargo 실행 중");
  });

  it("has no worktree cell for main and gives the reason a worktree cannot be ticked", () => {
    const rows = ready([checkout("main", { main: true }), checkout("wip"), checkout("ok", merged)], {
      wip: { exclusion_code: "dirty", exclusion_count: 3 },
      ok: {},
    });
    expect(rows[0]?.worktree).toBeNull();
    expect(rows[1]?.worktree).toEqual({ selectable: false, why: "바뀐 파일 3" });
    expect(rows[2]?.worktree?.selectable).toBe(true);
  });

  it("makes nothing selectable when in-use could not be read, without a banner state of its own", () => {
    const model = sheetModel(workspace([checkout("a")], { usage_error: "Herdr is not connected" }), false);
    expect(model.state).toBe("unreadable");
    expect(model.rows[0]?.cache.build_cache.selectable).toBe(false);
    expect(model.rows[0]?.worktree?.selectable).toBe(false);
  });

  it("is busy while another project's cleanup removes", () => {
    const other = workspace([checkout("a")], { phase: "removing" }, {}, "other");
    const mine = workspace([checkout("a")], null);
    expect(removingElsewhere([other, mine], "p")).toBe(true);
    expect(removingElsewhere([other], "other")).toBe(false);
    expect(sheetModel(mine, true).state).toBe("busy");
  });

  it("leaves a layer with nothing in it unselectable", () => {
    const rows = ready([checkout("none", { deps: 0 })]);
    expect(rows[0]?.cache.dependencies.selectable).toBe(false);
    expect(rows[0]?.cache.build_cache.selectable).toBe(true);
  });
});

describe("selection (D-09, B10, B11)", () => {
  const rows = ready([
    checkout("main", { main: true }),
    checkout("a", merged),
    checkout("b", merged),
    checkout("run", { ...merged, working: 1 }),
  ]);
  const visible = rows;

  it("ticks a cell alone and reads the row checkbox as partial", () => {
    let selection = toggleCell(EMPTY_SELECTION, { path: "/r/a", column: "build_cache" });
    const row = bundleRefs(visible.filter((r) => r.label === "a"), selection, ["build_cache", "dependencies"]);
    expect(bundleState(selection, row)).toBe("indeterminate");
    selection = toggleBundle(selection, row);
    expect(bundleState(selection, row)).toBe("checked");
    selection = toggleBundle(selection, row);
    expect(bundleState(selection, row)).toBe("unchecked");
  });

  it("turns a partial bundle fully on, then off", () => {
    const column = bundleRefs(visible, EMPTY_SELECTION, ["dependencies"]);
    expect(column.map((ref) => ref.path)).toEqual(["/r/main", "/r/a", "/r/b"]);
    let selection = toggleCell(EMPTY_SELECTION, column[0]!);
    expect(bundleState(selection, column)).toBe("indeterminate");
    selection = toggleBundle(selection, column);
    expect(bundleState(selection, column)).toBe("checked");
    expect(toggleBundle(selection, column).cells.size).toBe(0);
  });

  it("skips a disabled cell in every bundle, so the row in use stays unticked", () => {
    const all = bundleRefs(visible, EMPTY_SELECTION, ["build_cache", "dependencies"]);
    expect(all.some((ref) => ref.path === "/r/run")).toBe(false);
    const selection = toggleBundle(EMPTY_SELECTION, all);
    expect(selection.cells.size).toBe(all.length);
    expect([...selection.cells].some((key) => key.startsWith("/r/run"))).toBe(false);
  });

  it("covers the worktree column separately and never through the top-left bundle", () => {
    const worktrees = bundleRefs(visible, EMPTY_SELECTION, ["worktree"]);
    expect(worktrees.map((ref) => ref.path)).toEqual(["/r/a", "/r/b"]);
    const topLeft = toggleBundle(EMPTY_SELECTION, bundleRefs(visible, EMPTY_SELECTION, ["build_cache", "dependencies"]));
    expect(topLeft.worktrees.size).toBe(0);
  });

  it("includes a row's caches once its worktree is ticked and gives them back when it is unticked", () => {
    let selection = toggleCell(EMPTY_SELECTION, { path: "/r/a", column: "build_cache" });
    selection = toggleCell(selection, { path: "/r/a", column: "worktree" });
    const cachesOfA = bundleRefs(visible.filter((r) => r.label === "a"), selection, ["build_cache", "dependencies"]);
    expect(cachesOfA).toEqual([]);
    expect(bundleState(selection, cachesOfA)).toBe("none");
    const plan = planOf(visible, selection);
    expect(plan.cells).toEqual([]);
    expect(plan.worktrees.map((w) => w.label)).toEqual(["a"]);
    // The total counts the folder once: its caches are part of it.
    expect(plan.bytes).toBe(visible.find((r) => r.label === "a")?.total);
    selection = toggleCell(selection, { path: "/r/a", column: "worktree" });
    expect(planOf(visible, selection).cells.map((c) => `${c.path}:${c.layer}`)).toEqual(["/r/a:build_cache"]);
  });

  it("drops a ticked cell that stopped being selectable from the plan", () => {
    const selection = toggleCell(EMPTY_SELECTION, { path: "/r/run", column: "build_cache" });
    expect(planOf(visible, selection).cells).toEqual([]);
  });

  it("unticks what a filter hides (B12)", () => {
    let selection = toggleBundle(EMPTY_SELECTION, bundleRefs(visible, EMPTY_SELECTION, ["build_cache"]));
    selection = toggleCell(selection, { path: "/r/a", column: "worktree" });
    const kept = pruneSelection(selection, visibleRows(rows, "resting"));
    expect([...kept.cells]).toEqual(["/r/main\u0000build_cache"]);
    expect(kept.worktrees.size).toBe(0);
  });
});

describe("the folded small row (D-21, B10)", () => {
  const rows = ready([
    checkout("main", { main: true }),
    checkout("big", { build: 5 * GB }),
    checkout("s1", { build: 100 * MB, deps: 0 }),
    checkout("s2", { build: 200 * MB, deps: 0 }),
  ]);
  const layout = layoutRows(rows);

  it("ticks and clears its rows' caches as one, partial when only some are on, and never their worktrees", () => {
    const fold = bundleRefs(layout.small, EMPTY_SELECTION, ["build_cache", "dependencies"]);
    expect(fold.map((ref) => `${ref.path}:${ref.column}`)).toEqual(["/r/s2:build_cache", "/r/s1:build_cache"]);
    let selection = toggleCell(EMPTY_SELECTION, fold[0]!);
    expect(bundleState(selection, fold)).toBe("indeterminate");
    selection = toggleBundle(selection, fold);
    expect(bundleState(selection, fold)).toBe("checked");
    expect(selection.worktrees.size).toBe(0);
  });

  it("keeps the folded rows' selection when the fold opens, and the top-left bundle and footer include them", () => {
    const fold = bundleRefs(layout.small, EMPTY_SELECTION, ["build_cache"]);
    const selection = toggleBundle(EMPTY_SELECTION, fold);
    const all = bundleRefs(rows, selection, ["build_cache", "dependencies"]);
    expect(all.some((ref) => ref.path === "/r/s1")).toBe(true);
    expect(planOf(rows, selection).counts.build_cache).toBe(2);
  });
});

describe("footer (B13, B16, B17, B18)", () => {
  const rows = ready([
    checkout("main", { main: true, build: 0, deps: 0 }),
    checkout("done", { ...merged, build: 3 * GB, deps: 1 * GB }),
    checkout("done-2", { ...merged, build: 2 * GB, deps: 0 }),
  ]);

  it("says there is nothing to clear when no cell can be ticked", () => {
    const nothing = ready([checkout("main", { main: true, build: 0, deps: 0 })]);
    expect(footerOf(nothing, EMPTY_SELECTION)).toMatchObject({ nothingToClear: true, empty: true, summary: "비울 캐시가 없다" });
  });

  it("asks for a choice when cells exist but none is ticked", () => {
    expect(footerOf(rows, EMPTY_SELECTION)).toMatchObject({ nothingToClear: false, empty: true, summary: "고른 칸 없음" });
  });

  it("counts cells and sums their bytes, warning that dependencies come back by install", () => {
    const selection = toggleBundle(EMPTY_SELECTION, bundleRefs(rows, EMPTY_SELECTION, ["build_cache", "dependencies"]));
    const footer = footerOf(rows, selection);
    expect(footer.summary).toBe("빌드 캐시 2 · 의존성 1 · 워크트리 0 · 6.0 GB");
    expect(footer.note).toBe("의존성은 다음 install이 다시 받는다");
    expect(footer.destructive).toBeNull();
    expect(needsConfirm(planOf(rows, selection))).toBe(false);
  });

  it("names one worktree, or counts several, in red, and asks the confirmation", () => {
    const one = toggleCell(EMPTY_SELECTION, { path: "/r/done", column: "worktree" });
    expect(footerOf(rows, one).destructive).toBe("done은 폴더째 지워진다");
    expect(needsConfirm(planOf(rows, one))).toBe(true);
    const both = toggleBundle(EMPTY_SELECTION, bundleRefs(rows, EMPTY_SELECTION, ["worktree"]));
    expect(footerOf(rows, both).destructive).toBe("워크트리 2개는 폴더째 지워진다");
    expect(footerOf(rows, both).summary).toContain("워크트리 2");
  });
});

describe("the entrance (B2, D-29)", () => {
  const measured = (free: number | null, checkouts: Checkout[]): Workspace => ({ ...workspace(checkouts, null), disk: { total_bytes: 50 * GB, unavailable_reason: null, measuring: false, free_bytes: free } });

  it("stands only once measured and under 10 GB free", () => {
    expect(lowFree(measured(1.6 * GB, []))).toBe(1.6 * GB);
    expect(lowFree(measured(LOW_FREE_BYTES, []))).toBeNull();
    expect(lowFree(measured(null, []))).toBeNull();
    expect(lowFree({ ...measured(1 * GB, []), disk: { total_bytes: null, unavailable_reason: null, measuring: true, free_bytes: 1 * GB } })).toBeNull();
  });

  it("counts the caches of finished linked checkouts no agent works in", () => {
    const value = measured(1 * GB, [
      checkout("main", { main: true, build: 10 * GB }),
      checkout("done-a", { ...merged, build: 3 * GB, deps: 1 * GB }),
      checkout("done-busy", { ...merged, working: 1, build: 5 * GB }),
      checkout("open", { build: 7 * GB }),
    ]);
    expect(reclaimable(value)).toBe(4 * GB);
  });
});

describe("result (B22)", () => {
  const cleanup: DiskCleanup = {
    id: 1,
    workspace_id: "p",
    repository_root: "/r",
    phase: "complete",
    main_head: null,
    message: null,
    usage_error: null,
    free_bytes: null,
    progress: null,
    free_before: 1.6 * GB,
    free_after: 18.3 * GB,
    rows: [core("/r/gone", { result: "removed" }), core("/r/kept", { result: "refused", message: "State changed." })],
    cell_results: [
      { path: "/r/a", layer: "build_cache", outcome: "removed", bytes: 8 * GB, folders: 2, reason_code: null, reason: null },
      { path: "/r/a", layer: "dependencies", outcome: "removed", bytes: 1 * GB, folders: 1, reason_code: null, reason: null },
      { path: "/r/b", layer: "build_cache", outcome: "skipped", bytes: 2 * GB, folders: 1, reason_code: "in_use", reason: null },
      { path: "/r/c", layer: "dependencies", outcome: "failed", bytes: 0, folders: 1, reason_code: "io", reason: "permission denied" },
    ],
  };

  it("lists each removed, skipped and failed cell with its reason and never invents a size for a skip", () => {
    const lines = resultLines(cleanup, new Map([["/r/a", "a"]]), new Map([["/r/gone", 4 * GB]]));
    const by = (key: string) => lines.find((line) => line.key === key);
    expect(by("cells:/r/a:removed")).toMatchObject({ label: "a", what: "빌드 캐시 · 의존성", bytes: 9 * GB });
    expect(by("cells:/r/b:skipped")).toMatchObject({ reason: "확인 사이에 쓰는 중이 됨", bytes: null });
    expect(by("cells:/r/c:failed")).toMatchObject({ reason: "permission denied" });
    expect(by("worktree:/r/gone")).toMatchObject({ outcome: "removed", bytes: 4 * GB });
    expect(by("worktree:/r/kept")).toMatchObject({ outcome: "skipped", reason: "State changed." });
    expect(allocatedTotal(lines)).toBe(13 * GB);
  });

  it("reports the volume's own change beside the allocated total", () => {
    expect(freedFree(cleanup)).toBe(16.7 * GB);
    expect(freedFree({ ...cleanup, free_after: null })).toBeNull();
  });
});
