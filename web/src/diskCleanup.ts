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

import type { TFunction } from "i18next";
import type { MessageKey } from "./i18n/catalogs";
import { formatBytes } from "./i18n/format";
import type { InterfaceLanguage } from "./i18n/locale";
import { stageOf } from "./projectBoard";
import type { CacheLayer, Checkout, CleanupCellResult, CleanupInUse, CleanupResultCode, CleanupRow, DiskCell, DiskCleanup, Workspace } from "./snapshot";

type CleanupCellReason = NonNullable<CleanupCellResult["reason_code"]>;

export const CACHE_LAYERS: readonly CacheLayer[] = ["build_cache", "dependencies"];
export type Column = CacheLayer | "worktree";

/** A checkout under this joins the folded small-checkouts row (D-21). */
export const SMALL_CHECKOUT_BYTES = 1024 ** 3;
/** The warning cell stands only under this much free space (D-20). */
export const LOW_FREE_BYTES = 10 * 1024 ** 3;

export const LAYER_KEY: Record<Column, MessageKey> = { build_cache: "cleanup.layer.build_cache", dependencies: "cleanup.layer.dependencies", worktree: "cleanup.layer.worktree" };

export type Filter = "all" | "done" | "resting" | "working";
export type Bucket = Exclude<Filter, "all">;
export const FILTERS: readonly Filter[] = ["all", "done", "resting", "working"];
export const FILTER_KEY: Record<Filter, MessageKey> = { all: "cleanup.filter.all", done: "cleanup.filter.done", resting: "cleanup.filter.resting", working: "cleanup.filter.working" };

// --- reasons -------------------------------------------------------------------------

export function inUseText(inUse: CleanupInUse, t: TFunction<"translation">): string {
  if (inUse.code === "agent_working") return t("cleanup.inUse.agent");
  if (inUse.code === "agent_waiting") return t("cleanup.inUse.agentWaiting");
  if (inUse.code === "descendant_busy") return t("cleanup.inUse.descendantBusy");
  if (inUse.code === "process") return t("cleanup.inUse.process", { name: inUse.name ?? t("cleanup.inUse.processName") });
  if (inUse.code === "unverified") return t("cleanup.inUse.unverified");
  return t("cleanup.inUse.port", { port: String(inUse.port ?? "?") });
}

/** Every reason is worded here from the core's codes; the core's own sentences are for its log only (design 13). */
const CODE_KEY: Record<CleanupResultCode, MessageKey> = {
  main: "cleanup.reason.main",
  locked: "cleanup.reason.locked",
  current: "cleanup.reason.current",
  pane_open: "cleanup.reason.paneOpen",
  dirty: "cleanup.reason.dirty",
  not_merged: "cleanup.reason.notMerged",
  merge_unverified: "cleanup.reason.mergeUnverified",
  nested_repository: "cleanup.reason.nestedRepository",
  detached: "cleanup.reason.detached",
  contains_worktree: "cleanup.reason.containsWorktree",
  unverified: "cleanup.reason.unverified",
  main_unavailable: "cleanup.reason.mainUnavailable",
  alias: "cleanup.reason.folderUnreadable",
  missing: "cleanup.reason.folderUnreadable",
  unavailable: "cleanup.reason.folderUnreadable",
  in_use: "cleanup.reason.inUse",
  changed: "cleanup.reason.changed",
  not_found: "cleanup.reason.notFound",
  close_refused: "cleanup.reason.closeRefused",
  remove_refused: "cleanup.reason.removeRefused",
};

/** A reason code in the operator's words; a code this build does not know is said as such, never as the raw code. */
export function reasonText(code: CleanupResultCode, count: number | null | undefined, t: TFunction<"translation">): string {
  if (code === "dirty" && count) return t("cleanup.reason.dirtyCount", { count });
  return t(CODE_KEY[code] ?? "cleanup.reason.cannotRemove");
}

/** Why the core will not remove a worktree folder (B15); null for a row that may be, and for main, which has no worktree cell. */
export function exclusionText(row: CleanupRow, t: TFunction<"translation">): string | null {
  if (row.in_use) return inUseText(row.in_use, t);
  if (row.exclusion_code === null || row.exclusion_code === "main") return null;
  return reasonText(row.exclusion_code, row.exclusion_count, t);
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
  /** `panes` is how many live panes close with the folder: finished agents and shells, the core's count. */
  worktree: { selectable: boolean; why: string | null; panes: number } | null;
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

export type SheetModel = { state: SheetState; rows: SheetRow[] };

/** What the disabled clean-up button says while another project's worker holds the daemon: honest about a review that only reads. */
export const BUSY_KEY: Record<Elsewhere, MessageKey> = { loading: "cleanup.otherReview", removing: "cleanup.otherRemoval" };

const EMPTY_CELL: DiskCell = { bytes: 0, folders: 0, largest_name: null };

/** What another project's cleanup is doing: `loading` is its review still reading, `removing` its confirmed cleanup. */
export type Elsewhere = "loading" | "removing";

/**
 * The daemon runs one cleanup worker at a time, and the core refuses a review
 * or a dismiss while it runs, so a review or a removal of another project
 * holds this one back (B20). A finished review that nobody closed does not.
 */
export function cleanupElsewhere(workspaces: readonly Workspace[], workspaceId: string): Elsewhere | null {
  const phases = workspaces.filter((workspace) => workspace.id !== workspaceId).map((workspace) => workspace.cleanup?.phase);
  return phases.includes("removing") ? "removing" : phases.includes("loading") ? "loading" : null;
}

export function sheetModel(workspace: Workspace, elsewhere: Elsewhere | null, t: TFunction<"translation">): SheetModel {
  const cleanup = workspace.cleanup ?? null;
  const phase = cleanup?.phase ?? null;
  let state: SheetState = "pending";
  if (phase === "removing") state = "removing";
  else if (phase === "complete") state = "complete";
  else if (elsewhere) state = "busy";
  else if (phase === "failed" || (phase === "review" && cleanup?.usage_error)) state = "unreadable";
  else if (phase === "review") state = "ready";
  // The in-use answer arrives before the slow worktree checks: caches may be
  // ticked from then on, a worktree only once the review is whole.
  const open = !elsewhere && !cleanup?.usage_error && (phase === "review" || (phase === "loading" && cleanup?.usage_ready === true));
  const rows = workspace.checkouts.filter((checkout) => checkout.worktree).map((checkout) => rowOf(checkout, cleanup, open, state === "ready", t));
  return { state, rows };
}

function rowOf(checkout: Checkout, cleanup: DiskCleanup | null, cachesOpen: boolean, worktreesOpen: boolean, t: TFunction<"translation">): SheetRow {
  const core = cleanup?.rows.find((row) => row.path === checkout.path) ?? null;
  const worktree = checkout.worktree!;
  const disk = worktree.disk;
  const layers = disk?.layers ?? null;
  let measure: Measure = "pending";
  if (disk?.unavailable_reason) measure = "unavailable";
  else if (disk?.total_bytes != null) measure = layers ? "measured" : "unavailable";
  const isMain = worktree.is_main;
  const reviewed = cachesOpen && core !== null;
  const agentWorking = (checkout.agent_summary?.working ?? 0) > 0;
  // The review's judgement wins once it has answered; before that only the
  // agent projection the sidebar already carries can say a checkout is busy.
  const inUse = reviewed ? (core.in_use ? inUseText(core.in_use, t) : null) : agentWorking ? t("cleanup.inUse.agent") : null;
  const usable = reviewed && measure === "measured" && inUse === null;
  const cell = (layer: CacheLayer): CacheCell => {
    const source = layers?.[layer] ?? EMPTY_CELL;
    return {
      ...source,
      selectable: usable && source.bytes > 0,
      why: measure === "unavailable" ? t("cleanup.sizeUnknown") : inUse,
    };
  };
  const bucket: Bucket = inUse ? "working" : !isMain && stageOf(checkout) === "done" ? "done" : "resting";
  const worktreeWhy = core ? exclusionText(core, t) : null;
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
    worktree: isMain ? null : { selectable: worktreesOpen && usable && core !== null && core.exclusion_code === null && core.result === null, why: measure === "unavailable" ? t("cleanup.sizeUnknown") : worktreeWhy, panes: core?.pane_count ?? 0 },
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
  worktrees: { path: string; label: string; bytes: number; panes: number }[];
  /** Cache cells to empty, never one whose worktree is going too. */
  cells: { path: string; layer: CacheLayer; bytes: number }[];
  bytes: number;
};

/** What the selection would do to the visible rows now: a ticked cell that stopped being selectable is not part of it. */
export function planOf(rows: readonly SheetRow[], selection: Selection): CleanupPlan {
  const plan: CleanupPlan = { worktrees: [], cells: [], bytes: 0 };
  for (const row of rows) {
    if (row.worktree?.selectable && selection.worktrees.has(row.path)) {
      const bytes = row.total ?? 0;
      plan.worktrees.push({ path: row.path, label: row.label, bytes, panes: row.worktree.panes });
      plan.bytes += bytes;
      continue;
    }
    for (const layer of CACHE_LAYERS) {
      const cell = row.cache[layer];
      if (!cell.selectable || !selection.cells.has(cellKey(row.path, layer))) continue;
      plan.cells.push({ path: row.path, layer, bytes: cell.bytes });
      plan.bytes += cell.bytes;
    }
  }
  return plan;
}

export type Footer = {
  empty: boolean;
  /** `N cells · M worktrees · X`, leaving out a part that is zero (B16); the explaining is the confirm step's. */
  summary: string;
};

/** What the bottom line says while the table cannot be ticked yet or at all; the ready state speaks for itself (B13, B25). */
const WAITING_SUMMARY: Partial<Record<SheetState, MessageKey | null>> = {
  pending: "cleanup.reviewing",
  unreadable: "cleanup.mustCheckUse",
  busy: null,
};

export function footerOf(rows: readonly SheetRow[], selection: Selection, t: TFunction<"translation">, language: InterfaceLanguage, state: SheetState = "ready"): Footer {
  const plan = planOf(rows, selection);
  const anySelectable = rows.some((row) => CACHE_LAYERS.some((layer) => row.cache[layer].selectable) || row.worktree?.selectable);
  const empty = plan.cells.length === 0 && plan.worktrees.length === 0;
  const waiting = WAITING_SUMMARY[state];
  const summary = waiting !== undefined
    ? (waiting === null ? "" : t(waiting))
    : empty
    ? t(anySelectable ? "cleanup.noSelection" : "cleanup.noCache")
    : [
        plan.cells.length > 0 ? t("cleanup.selectedCells", { count: plan.cells.length }) : null,
        plan.worktrees.length > 0 ? t("cleanup.selectedWorktrees", { count: plan.worktrees.length }) : null,
        formatBytes(language, plan.bytes),
      ].filter(Boolean).join(" · ");
  return { empty, summary };
}

/** The confirm step is asked only when a worktree is in the plan (D-15). */
export function needsConfirm(plan: CleanupPlan): boolean {
  return plan.worktrees.length > 0;
}

// --- the entrance (B1, B2, D-29) ----------------------------------------------------------

/** The warning cell shows once the measurement is back and the volume is under the limit (B2, D-20); one checkout that could not be measured does not hide it. */
export function lowFree(workspace: Workspace): number | null {
  const disk = workspace.disk;
  if (!disk || disk.measuring || disk.free_bytes == null) return null;
  return disk.free_bytes < LOW_FREE_BYTES ? disk.free_bytes : null;
}

/** The size the entrance may state: the total when every checkout was measured, else the subtotal of those that were (never a total the system cannot produce). */
export function entranceBytes(workspace: Workspace): { bytes: number; partial: boolean } | null {
  const disk = workspace.disk;
  if (!disk || disk.measuring) return null;
  if (disk.total_bytes != null) return { bytes: disk.total_bytes, partial: false };
  return disk.confirmed_bytes != null ? { bytes: disk.confirmed_bytes, partial: true } : null;
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

/** The tooltip's lines in the PRD's order (B1, D-29); null until any checkout's layers are known. */
export function layerLines(workspace: Workspace, t: TFunction<"translation">): LayerLine[] | null {
  const layers = workspace.disk?.layers;
  if (!layers) return null;
  return [
    { key: "build_cache", label: t("cleanup.layer.build_cache"), bytes: layers.build_cache },
    { key: "dependencies", label: t("cleanup.layer.dependencies"), bytes: layers.dependencies },
    { key: "source", label: t("cleanup.layer.worktreeSource"), bytes: layers.source },
    { key: "other", label: t("cleanup.layer.keptOther"), bytes: layers.other },
    { key: "shared_git", label: t("cleanup.layer.sharedGit"), bytes: layers.shared_git },
  ];
}

// --- results (B22) --------------------------------------------------------------------------

/** What kept a cell's folders, by the core's code; `io` is a move that failed. */
const CELL_REASON_KEY: Record<CleanupCellReason, MessageKey> = {
  in_use: "cleanup.reason.becameInUse",
  changed: "cleanup.reason.changed",
  tracked_files: "cleanup.reason.trackedFiles",
  nested_repository: "cleanup.reason.nestedRepository",
  symlink: "cleanup.reason.symlink",
  not_found: "cleanup.reason.notFound",
  unverified: "cleanup.reason.unverified",
  io: "cleanup.reason.io",
};

export function cellReasonText(code: CleanupCellReason, t: TFunction<"translation">): string {
  return t(CELL_REASON_KEY[code] ?? "cleanup.reason.cannotRemove");
}

export type ResultLine = { key: string; label: string; what: string; outcome: "removed" | "skipped" | "failed"; bytes: number | null; reason: string | null };

/**
 * One line per worktree and per cell the confirm touched, worded from the
 * core's codes alone. A worktree is named by its branch and sized by the bytes
 * the core recorded when it removed it, so a result opened again after the
 * sheet was closed reads the same as one that never left.
 */
export function resultLines(cleanup: DiskCleanup, t: TFunction<"translation">): ResultLine[] {
  const label = (path: string) => cleanup.rows.find((row) => row.path === path)?.branch ?? path.slice(path.lastIndexOf("/") + 1);
  const lines: ResultLine[] = [];
  for (const row of cleanup.rows) {
    if (row.result === null) continue;
    lines.push({
      key: `worktree:${row.path}`,
      label: label(row.path),
      what: t("cleanup.worktreeRemoval"),
      outcome: row.result,
      bytes: row.result === "removed" ? row.bytes : null,
      reason: row.result === "removed" ? null : row.result_code ? reasonText(row.result_code, row.exclusion_count, t) : null,
    });
  }
  const byPath = new Map<string, typeof cleanup.cell_results>();
  for (const cell of cleanup.cell_results) byPath.set(cell.path, [...(byPath.get(cell.path) ?? []), cell]);
  for (const [path, cells] of byPath) {
    for (const outcome of ["removed", "skipped", "failed"] as const) {
      const group = cells.filter((cell) => cell.outcome === outcome);
      if (group.length === 0) continue;
      const code = group.find((cell) => cell.reason_code)?.reason_code ?? null;
      lines.push({
        key: `cells:${path}:${outcome}`,
        label: label(path),
        what: group.map((cell) => t(LAYER_KEY[cell.layer])).join(" · "),
        outcome,
        bytes: outcome === "removed" ? group.reduce((sum, cell) => sum + cell.bytes, 0) : null,
        // A cell of several folders can be removed with the folders it kept named by their reason.
        reason: code ? cellReasonText(code, t) : null,
      });
    }
  }
  return lines;
}
