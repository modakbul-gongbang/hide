// The disk cleanup sheet's model (PRD disk-layers): every rule the sheet draws
// is a pure function here, so a unit test asserts the selection, the filters,
// the folded row and the footer without a browser. The sheet's checkboxes and
// buttons only call these; nothing here talks to the core.
//
// One checkout is one row and its layers are its cells: the two cache layers
// a build tool remakes (`build_cache`, `dependencies`), the worktree folder
// itself, and `other`, which is size only (D-10). A cell is selectable only
// while the core's review says its checkout is not in use (D-14), so the
// selection can never reach what the core has not judged.

import { formatBytes, stageOf } from "./projectBoard";
import type { CacheLayer, Checkout, DiskCell, DiskCleanup, CleanupInUse, CleanupRow, Workspace } from "./snapshot";

export const CACHE_LAYERS: readonly CacheLayer[] = ["build_cache", "dependencies"];
export type Column = CacheLayer | "worktree";

/** A checkout under this joins the folded `작은 체크아웃` row (D-21). */
export const SMALL_CHECKOUT_BYTES = 1024 ** 3;
/** The warning cell stands only under this much free space (D-20). */
export const LOW_FREE_BYTES = 10 * 1024 ** 3;

export const LAYER_LABEL: Record<Column, string> = { build_cache: "빌드 캐시", dependencies: "의존성", worktree: "워크트리" };

export type Filter = "all" | "done" | "resting" | "working";
export type Bucket = Exclude<Filter, "all">;
export const FILTERS: readonly Filter[] = ["all", "done", "resting", "working"];
export const FILTER_LABEL: Record<Filter, string> = { all: "전체", done: "끝난 것", resting: "쉬는 것", working: "작업 중" };

// --- reasons -------------------------------------------------------------------------

export function inUseText(inUse: CleanupInUse): string {
  if (inUse.code === "agent_working") return "에이전트 작업 중";
  if (inUse.code === "process") return `터미널에서 ${inUse.name ?? "프로세스"} 실행 중`;
  return `포트 ${inUse.port ?? "?"} 서버`;
}

/** Why the core will not remove a worktree folder, in the operator's words (B15). */
export function exclusionText(row: CleanupRow): string | null {
  if (row.in_use) return inUseText(row.in_use);
  switch (row.exclusion_code) {
    case null:
      return null;
    case "main":
      return null;
    case "locked":
      return "잠김";
    case "current":
      return "지금 보고 있는 체크아웃";
    case "pane_open":
      return "pane이 열려 있음";
    case "dirty":
      return row.exclusion_count ? `바뀐 파일 ${row.exclusion_count}` : "바뀐 파일 있음";
    case "not_merged":
      return "main에 머지되지 않음";
    case "merge_unverified":
      return "머지를 확인하지 못함";
    case "nested_repository":
      return "안에 다른 저장소";
    case "detached":
      return "브랜치 없이 떨어진 HEAD";
    case "contains_worktree":
      return "다른 워크트리를 품고 있음";
    case "unknown_pane_cwd":
      return "pane의 위치를 알 수 없음";
    case "main_unavailable":
      return "로컬 main을 읽을 수 없음";
    case "alias":
    case "missing":
    case "unavailable":
      return "폴더를 읽을 수 없음";
    case "in_use":
      return "쓰는 중";
    default:
      return row.exclusion ?? "지울 수 없음";
  }
}

// --- rows ----------------------------------------------------------------------------

export type Measure = "pending" | "unavailable" | "measured";

export type CacheCell = DiskCell & {
  /** Whether the operator may tick it now; `why` says what stops them when it is a thing worth saying. */
  selectable: boolean;
  why: string | null;
};

export type SheetRow = {
  path: string;
  checkout: Checkout;
  label: string;
  isMain: boolean;
  /** D-22: in use, else finished (the Overview's done stage), else resting. */
  bucket: Bucket;
  measure: Measure;
  /** Allocated bytes of the whole folder, the worktree cell's size. */
  total: number | null;
  cache: Record<CacheLayer, CacheCell>;
  other: DiskCell | null;
  worktree: { selectable: boolean; why: string | null } | null;
  inUse: string | null;
};

export type SheetState =
  /** The review has not answered yet, or measurement is still coming. */
  | "pending"
  | "ready"
  /** In-use could not be read: nothing is selectable (B25). */
  | "unreadable"
  /** Another project's cleanup is running (B20). */
  | "busy"
  | "removing"
  | "complete";

export type SheetModel = { state: SheetState; message: string | null; rows: SheetRow[] };

const EMPTY_CELL: DiskCell = { bytes: 0, folders: 0, largest_name: null };

/** The busiest cleanup on this daemon: a removing one anywhere blocks a new one (B20). */
export function removingElsewhere(workspaces: readonly Workspace[], workspaceId: string): boolean {
  return workspaces.some((workspace) => workspace.id !== workspaceId && workspace.cleanup?.phase === "removing");
}

export function sheetModel(workspace: Workspace, busyElsewhere: boolean): SheetModel {
  const cleanup = workspace.cleanup ?? null;
  const phase = cleanup?.phase ?? null;
  let state: SheetState = "pending";
  if (phase === "removing") state = "removing";
  else if (phase === "complete") state = "complete";
  else if (busyElsewhere) state = "busy";
  else if (phase === "failed" || (phase === "review" && cleanup?.usage_error)) state = "unreadable";
  else if (phase === "review") state = "ready";
  const message = state === "unreadable" ? (cleanup?.usage_error ?? cleanup?.message ?? null) : null;
  const rows = workspace.checkouts.filter((checkout) => checkout.worktree).map((checkout) => rowOf(checkout, cleanup, state === "ready"));
  return { state, message, rows };
}

function rowOf(checkout: Checkout, cleanup: DiskCleanup | null, ready: boolean): SheetRow {
  const core = cleanup?.rows.find((row) => row.path === checkout.path) ?? null;
  const worktree = checkout.worktree!;
  const disk = worktree.disk;
  const layers = disk?.layers ?? null;
  let measure: Measure = "pending";
  if (disk?.unavailable_reason) measure = "unavailable";
  else if (disk?.total_bytes != null) measure = layers ? "measured" : "unavailable";
  const isMain = worktree.is_main;
  const reviewed = ready && core !== null;
  const agentWorking = (checkout.agent_summary?.working ?? 0) > 0;
  // The review's judgement wins once it has answered; before that only the
  // agent projection the sidebar already carries can say a checkout is busy.
  const inUse = reviewed ? (core.in_use ? inUseText(core.in_use) : null) : agentWorking ? "에이전트 작업 중" : null;
  const usable = reviewed && measure === "measured" && inUse === null;
  const cell = (layer: CacheLayer): CacheCell => {
    const source = layers?.[layer] ?? EMPTY_CELL;
    return {
      ...source,
      selectable: usable && source.bytes > 0,
      why: measure === "unavailable" ? "크기를 재지 못함" : inUse,
    };
  };
  const bucket: Bucket = inUse ? "working" : !isMain && stageOf(checkout) === "done" ? "done" : "resting";
  const worktreeWhy = core ? exclusionText(core) : null;
  return {
    path: checkout.path,
    checkout,
    label: checkout.branch ?? checkout.label,
    isMain,
    bucket,
    measure,
    total: measure === "measured" ? (disk?.total_bytes ?? null) : null,
    cache: { build_cache: cell("build_cache"), dependencies: cell("dependencies") },
    other: layers ? layers.other : null,
    worktree: isMain ? null : { selectable: usable && core !== null && core.exclusion_code === null && core.result === null, why: measure === "unavailable" ? "크기를 재지 못함" : worktreeWhy },
    inUse,
  };
}

// --- filters and layout (B4, B12, D-21, D-22) ------------------------------------------

export function filterCounts(rows: readonly SheetRow[]): Record<Filter, number> {
  return {
    all: rows.length,
    done: rows.filter((row) => row.bucket === "done").length,
    resting: rows.filter((row) => row.bucket === "resting").length,
    working: rows.filter((row) => row.bucket === "working").length,
  };
}

export function visibleRows(rows: readonly SheetRow[], filter: Filter): SheetRow[] {
  return filter === "all" ? [...rows] : rows.filter((row) => row.bucket === filter);
}

export type SheetLayout = { main: SheetRow | null; big: SheetRow[]; small: SheetRow[]; smallBytes: number };

/** main on top, the rest by size, and the checkouts under 1 GB folded (B4). A row not yet measured stays out of the fold. */
export function layoutRows(rows: readonly SheetRow[]): SheetLayout {
  const main = rows.find((row) => row.isMain) ?? null;
  const others = rows.filter((row) => !row.isMain);
  const isSmall = (row: SheetRow) => row.total !== null && row.total < SMALL_CHECKOUT_BYTES;
  const bySize = (a: SheetRow, b: SheetRow) => (b.total ?? -1) - (a.total ?? -1) || a.label.localeCompare(b.label);
  const small = others.filter(isSmall).sort(bySize);
  return { main, big: others.filter((row) => !isSmall(row)).sort(bySize), small, smallBytes: small.reduce((sum, row) => sum + (row.total ?? 0), 0) };
}

// --- selection (D-09, B10, B11) ---------------------------------------------------------

export type Selection = { readonly cells: ReadonlySet<string>; readonly worktrees: ReadonlySet<string> };
export const EMPTY_SELECTION: Selection = { cells: new Set(), worktrees: new Set() };

export type CellRef = { path: string; column: Column };

const cellKey = (path: string, layer: CacheLayer) => `${path}\u0000${layer}`;

export function isChecked(selection: Selection, ref: CellRef): boolean {
  return ref.column === "worktree" ? selection.worktrees.has(ref.path) : selection.cells.has(cellKey(ref.path, ref.column));
}

/** A cache cell of a row whose worktree is ticked shows as included: not its own to tick (B11). */
export function isIncluded(selection: Selection, ref: CellRef): boolean {
  return ref.column !== "worktree" && selection.worktrees.has(ref.path);
}

/** The cells a bundle checkbox covers: only the selectable ones of the given rows, never an included one (B10). */
export function bundleRefs(rows: readonly SheetRow[], selection: Selection, columns: readonly Column[]): CellRef[] {
  const refs: CellRef[] = [];
  for (const row of rows) {
    for (const column of columns) {
      const selectable = column === "worktree" ? (row.worktree?.selectable ?? false) : row.cache[column].selectable;
      const ref = { path: row.path, column };
      if (selectable && !isIncluded(selection, ref)) refs.push(ref);
    }
  }
  return refs;
}

export type BundleState = "checked" | "indeterminate" | "unchecked" | "none";

export function bundleState(selection: Selection, refs: readonly CellRef[]): BundleState {
  if (refs.length === 0) return "none";
  const checked = refs.filter((ref) => isChecked(selection, ref)).length;
  return checked === 0 ? "unchecked" : checked === refs.length ? "checked" : "indeterminate";
}

function withRefs(selection: Selection, refs: readonly CellRef[], on: boolean): Selection {
  const cells = new Set(selection.cells);
  const worktrees = new Set(selection.worktrees);
  for (const ref of refs) {
    const [set, key] = ref.column === "worktree" ? [worktrees, ref.path] : [cells, cellKey(ref.path, ref.column)];
    if (on) set.add(key);
    else set.delete(key);
  }
  return { cells, worktrees };
}

/** A bundle checkbox: all on unless all are already on (a partial bundle turns fully on, as a tri-state checkbox does). */
export function toggleBundle(selection: Selection, refs: readonly CellRef[]): Selection {
  return withRefs(selection, refs, bundleState(selection, refs) !== "checked");
}

/** One cell's own checkbox. */
export function toggleCell(selection: Selection, ref: CellRef): Selection {
  return withRefs(selection, [ref], !isChecked(selection, ref));
}

/** Changing the filter unticks what it hides, so a cleanup only ever reaches visible rows (B12). */
export function pruneSelection(selection: Selection, visible: readonly SheetRow[]): Selection {
  const paths = new Set(visible.map((row) => row.path));
  const cells = new Set([...selection.cells].filter((key) => paths.has(key.slice(0, key.indexOf("\u0000")))));
  const worktrees = new Set([...selection.worktrees].filter((path) => paths.has(path)));
  return { cells, worktrees };
}

// --- the plan the footer and the events read (B16-B18) ----------------------------------

export type CleanupPlan = {
  /** Worktree folders to remove; their own caches go with them. */
  worktrees: { path: string; label: string; bytes: number }[];
  /** Cache cells to empty, never one whose worktree is going too. */
  cells: { path: string; layer: CacheLayer; bytes: number }[];
  counts: { build_cache: number; dependencies: number; worktrees: number };
  bytes: number;
};

/** What the selection would do to the visible rows now: a ticked cell that stopped being selectable is not part of it. */
export function planOf(rows: readonly SheetRow[], selection: Selection): CleanupPlan {
  const plan: CleanupPlan = { worktrees: [], cells: [], counts: { build_cache: 0, dependencies: 0, worktrees: 0 }, bytes: 0 };
  for (const row of rows) {
    if (row.worktree?.selectable && selection.worktrees.has(row.path)) {
      const bytes = row.total ?? 0;
      plan.worktrees.push({ path: row.path, label: row.label, bytes });
      plan.counts.worktrees += 1;
      plan.bytes += bytes;
      continue;
    }
    for (const layer of CACHE_LAYERS) {
      const cell = row.cache[layer];
      if (!cell.selectable || !selection.cells.has(cellKey(row.path, layer))) continue;
      plan.cells.push({ path: row.path, layer, bytes: cell.bytes });
      plan.counts[layer] += 1;
      plan.bytes += cell.bytes;
    }
  }
  return plan;
}

export type Footer = {
  /** Nothing in the visible rows can be ticked at all (B13). */
  nothingToClear: boolean;
  empty: boolean;
  summary: string;
  /** Red: what is deleted with the folder (B16). */
  destructive: string | null;
  /** What comes back by itself. */
  note: string | null;
};

export function footerOf(rows: readonly SheetRow[], selection: Selection): Footer {
  const plan = planOf(rows, selection);
  const anySelectable = rows.some((row) => CACHE_LAYERS.some((layer) => row.cache[layer].selectable) || row.worktree?.selectable);
  const empty = plan.cells.length === 0 && plan.worktrees.length === 0;
  const { counts } = plan;
  const summary = empty
    ? anySelectable
      ? "고른 칸 없음"
      : "비울 캐시가 없다"
    : `빌드 캐시 ${counts.build_cache} · 의존성 ${counts.dependencies} · 워크트리 ${counts.worktrees} · ${formatBytes(plan.bytes)}`;
  const destructive =
    plan.worktrees.length === 0 ? null : plan.worktrees.length === 1 ? `${plan.worktrees[0]!.label}은 폴더째 지워진다` : `워크트리 ${plan.worktrees.length}개는 폴더째 지워진다`;
  return {
    nothingToClear: !anySelectable,
    empty,
    summary,
    destructive,
    note: counts.dependencies > 0 ? "의존성은 다음 install이 다시 받는다" : null,
  };
}

/** The confirm step is asked only when a worktree is in the plan (D-15). */
export function needsConfirm(plan: CleanupPlan): boolean {
  return plan.worktrees.length > 0;
}

// --- the entrance (B1, B2, D-29) ----------------------------------------------------------

/** The warning cell shows only once measured and under the limit (B2, D-20). */
export function lowFree(workspace: Workspace): number | null {
  const disk = workspace.disk;
  if (!disk || disk.total_bytes == null || disk.free_bytes == null) return null;
  return disk.free_bytes < LOW_FREE_BYTES ? disk.free_bytes : null;
}

/**
 * What the warning cell says can be freed: the caches of finished linked
 * checkouts that no agent is working in. Process and port signals are read
 * only when the sheet opens, so this can overstate; the sheet says the rest.
 */
export function reclaimable(workspace: Workspace): number {
  let bytes = 0;
  for (const checkout of workspace.checkouts) {
    const worktree = checkout.worktree;
    if (!worktree || worktree.is_main || stageOf(checkout) !== "done") continue;
    if ((checkout.agent_summary?.working ?? 0) > 0) continue;
    const layers = worktree.disk?.layers;
    if (!layers) continue;
    bytes += layers.build_cache.bytes + layers.dependencies.bytes;
  }
  return bytes;
}

export type LayerLine = { key: "build_cache" | "dependencies" | "source" | "other" | "shared_git"; label: string; bytes: number };

/** The tooltip's lines, in the order the board draws them (D-29); null until the project's layers are known. */
export function layerLines(workspace: Workspace): LayerLine[] | null {
  const layers = workspace.disk?.layers;
  if (!layers) return null;
  return [
    { key: "build_cache", label: "빌드 캐시", bytes: layers.build_cache },
    { key: "dependencies", label: "의존성", bytes: layers.dependencies },
    { key: "other", label: "기타 · 지우지 않음", bytes: layers.other },
    { key: "source", label: "소스", bytes: layers.source },
    { key: "shared_git", label: ".git 공유", bytes: layers.shared_git },
  ];
}

// --- results (B22) --------------------------------------------------------------------------

const SKIP_TEXT: Record<string, string> = {
  in_use: "확인 사이에 쓰는 중이 됨",
  changed: "확인 사이에 바뀜",
  tracked_files: "추적 파일이 있음",
  nested_repository: "안에 다른 저장소",
  symlink: "심링크라 건너뜀",
  not_found: "이미 없음",
};

export type ResultLine = { key: string; label: string; what: string; outcome: "removed" | "skipped" | "failed"; bytes: number | null; reason: string | null };

/**
 * One line per worktree and per cell the confirm touched. `sizes` holds the
 * folder sizes the sheet saw when the operator confirmed, because a removed
 * worktree leaves the catalog and takes its size with it.
 */
export function resultLines(cleanup: DiskCleanup, labels: ReadonlyMap<string, string>, sizes: ReadonlyMap<string, number>): ResultLine[] {
  const label = (path: string) => labels.get(path) ?? path.slice(path.lastIndexOf("/") + 1);
  const lines: ResultLine[] = [];
  for (const row of cleanup.rows) {
    if (row.result === null) continue;
    lines.push({
      key: `worktree:${row.path}`,
      label: label(row.path),
      what: "워크트리 삭제",
      outcome: row.result === "removed" ? "removed" : "skipped",
      bytes: row.result === "removed" ? (sizes.get(row.path) ?? null) : null,
      reason: row.result === "removed" ? null : (exclusionTextOrMessage(row)),
    });
  }
  const byPath = new Map<string, typeof cleanup.cell_results>();
  for (const cell of cleanup.cell_results) byPath.set(cell.path, [...(byPath.get(cell.path) ?? []), cell]);
  for (const [path, cells] of byPath) {
    for (const outcome of ["removed", "skipped", "failed"] as const) {
      const group = cells.filter((cell) => cell.outcome === outcome);
      if (group.length === 0) continue;
      lines.push({
        key: `cells:${path}:${outcome}`,
        label: label(path),
        what: group.map((cell) => LAYER_LABEL[cell.layer]).join(" · "),
        outcome,
        bytes: outcome === "removed" ? group.reduce((sum, cell) => sum + cell.bytes, 0) : null,
        reason: outcome === "removed" ? null : (group[0]!.reason ?? (group[0]!.reason_code ? (SKIP_TEXT[group[0]!.reason_code] ?? group[0]!.reason_code) : null)),
      });
    }
  }
  return lines;
}

function exclusionTextOrMessage(row: CleanupRow): string | null {
  return row.message ?? exclusionText(row);
}

/** The allocated bytes the confirm removed; the volume's own free space says what actually came back. */
export function allocatedTotal(lines: readonly ResultLine[]): number {
  return lines.reduce((sum, line) => sum + (line.outcome === "removed" ? (line.bytes ?? 0) : 0), 0);
}

export function freedFree(cleanup: DiskCleanup): number | null {
  return cleanup.free_before != null && cleanup.free_after != null ? cleanup.free_after - cleanup.free_before : null;
}

/** A signed size for a change of free space; the sign is the system's, not made up. */
export function signedBytes(bytes: number): string {
  return bytes < 0 ? `-${formatBytes(-bytes)}` : formatBytes(bytes);
}
