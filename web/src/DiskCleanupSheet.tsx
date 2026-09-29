import { AlertTriangleIcon, ChevronRightIcon, GitBranchIcon, HomeIcon, Loader2Icon, LockIcon, RefreshCwIcon, Trash2Icon } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { Actions } from "./actions";
import { AlertDialog, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from "./components/ui/alert-dialog";
import { Button } from "./components/ui/button";
import { Checkbox } from "./components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "./components/ui/dialog";
import { ToggleGroup, ToggleGroupItem } from "./components/ui/toggle-group";
import { Hint } from "./components/ui/tooltip";
import {
  EMPTY_SELECTION,
  FILTERS,
  FILTER_LABEL,
  LAYER_LABEL,
  BUSY_TEXT,
  allocatedTotal,
  cleanupElsewhere,
  bundleRefs,
  bundleState,
  filterCounts,
  footerOf,
  freedFree,
  gigabytes,
  isChecked,
  isIncluded,
  entranceBytes,
  layoutRows,
  needsConfirm,
  planOf,
  pruneSelection,
  resultLines,
  sheetModel,
  signedBytes,
  toggleBundle,
  toggleCell,
  visibleRows,
  type BundleState,
  type CacheCell,
  type CellRef,
  type Column,
  type Filter,
  type ResultLine,
  type Selection,
  type SheetModel,
  type SheetRow,
  type SheetState,
} from "./diskCleanup";
import { cn } from "./lib/utils";
import { formatBytes, prChip } from "./projectBoard";
import type { CacheLayer, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { PR_TONE } from "./TaskBoards";

// The disk cleanup sheet of a local Git project (PRD disk-layers): one row per
// checkout, one cell per layer a build tool remakes, and the checkboxes that
// pick them. Every rule it draws lives in `diskCleanup.ts`; this file only
// lays the model out and turns a press into one core event. The table is a
// screen-local grid (D-31): no System / Table part exists for it.

/** The sheet's width: the `--size-disk-sheet` token. */
const SHEET_WIDTH = "w-(--size-disk-sheet)";
/** Checkbox, checkout, the two cache layers, the worktree, other and the row total. */
const GRID = "var(--spacing-xl) minmax(0, 2.6fr) repeat(3, minmax(0, 1.2fr)) minmax(0, 0.9fr) minmax(0, 0.9fr)";

/** The dot each layer wears in the usage bar, the tooltip and the legend. */
export const LAYER_DOT = {
  build_cache: "bg-accent-choice-amber",
  dependencies: "bg-file-blue",
  other: "bg-file-yellow",
  source: "bg-file-neutral",
  shared_git: "bg-file-neutral",
} as const;

type Step = "table" | "confirm";

/** How long a confirm may go unanswered before the sheet takes it as dropped; the core answers in one snapshot. */
const CONFIRM_ANSWER_MS = 5000;

export function DiskCleanupSheet({ actions, workspace, filter: initialFilter, onClose }: { actions: Actions; workspace: Workspace; filter: Filter; onClose: () => void }) {
  const workspaces = useShellStore((s) => s.rest?.navigator?.workspaces);
  const elsewhere = cleanupElsewhere(workspaces ?? [], workspace.id);
  const model = useMemo(() => sheetModel(workspace, elsewhere), [workspace, elsewhere]);
  const cleanup = workspace.cleanup ?? null;
  const [filter, setFilter] = useState<Filter>(initialFilter);
  const [selection, setSelection] = useState<Selection>(EMPTY_SELECTION);
  const [foldOpen, setFoldOpen] = useState(false);
  const [step, setStep] = useState<Step>("table");
  // A press on `정리` is one event; until the core answers with `removing` a second press is not another (B23).
  const [sent, setSent] = useState(false);
  const cleanupId = cleanup?.id ?? null;
  const phase = cleanup?.phase ?? null;

  // Opening reviews the project, unless a cleanup that already ran or is running is what there is to show (B20).
  // Another project's review or cleanup that ends while the sheet is open leaves this project unreviewed: review when it does.
  useEffect(() => {
    if (elsewhere || phase === "removing" || phase === "complete") return;
    actions.reviewDiskCleanup(workspace.id);
    // On opening and when the daemon's other cleanup ends; a later `다시 검토` is its own press.
  }, [elsewhere === null]);
  // A new review starts from nothing ticked, and a settled press can be made again.
  useEffect(() => {
    setSelection(EMPTY_SELECTION);
    setStep("table");
    setSent(false);
  }, [cleanupId]);
  useEffect(() => {
    if (phase !== "review") setSent(phase === "removing");
  }, [phase]);
  // The core drops a confirm it cannot take (stale id, review gone) with no phase change: after its answer window
  // the press is released, so `정리` never stays disabled with nothing said.
  useEffect(() => {
    if (!sent || phase !== "review") return;
    const timer = window.setTimeout(() => setSent(false), CONFIRM_ANSWER_MS);
    return () => window.clearTimeout(timer);
  }, [sent, phase]);

  const shown = visibleRows(model.rows, filter);
  const counts = filterCounts(model.rows);
  const layout = layoutRows(shown);
  const footer = footerOf(shown, selection, model.state);
  const plan = planOf(shown, selection);

  const close = () => {
    if (cleanup && phase !== "removing") actions.dismissDiskCleanup();
    onClose();
  };
  const changeFilter = (next: Filter) => {
    setFilter(next);
    setSelection((current) => pruneSelection(current, visibleRows(model.rows, next)));
  };
  const run = () => {
    if (!cleanup || sent) return;
    setSent(true);
    setStep("table");
    actions.confirmDiskCleanup(
      cleanup.id,
      plan.worktrees.map((worktree) => worktree.path),
      plan.cells.map((cell) => ({ path: cell.path, layer: cell.layer })),
    );
  };
  const press = () => (needsConfirm(plan) ? setStep("confirm") : run());
  // The one worktree the confirmation named can stop being removable while it is open (a row turned in use):
  // with none left the sheet is back on the table, where that row says why.
  const confirming = step === "confirm" && plan.worktrees.length > 0;
  useEffect(() => {
    if (step === "confirm" && plan.worktrees.length === 0) setStep("table");
  }, [step, plan.worktrees.length]);

  const title = "디스크 정리";
  const body =
    model.state === "removing" ? (
      <RunningView progress={cleanup?.progress ?? null} />
    ) : model.state === "complete" && cleanup ? (
      <ResultView workspace={workspace} onReview={() => actions.reviewDiskCleanup(workspace.id)} onClose={close} />
    ) : (
      <TableView
        workspace={workspace}
        model={model}
        filter={filter}
        counts={counts}
        layout={layout}
        shown={shown}
        selection={selection}
        onSelection={setSelection}
        onFilter={changeFilter}
        foldOpen={foldOpen}
        onFold={() => setFoldOpen((open) => !open)}
        onRetry={() => actions.reviewDiskCleanup(workspace.id)}
      />
    );
  const listing = model.state !== "removing" && model.state !== "complete";
  return (
    <>
      <Dialog open onOpenChange={(next) => { if (!next) close(); }}>
        <DialogContent className={listing ? SHEET_WIDTH : "w-(--size-overview-cleanup)"} showCloseButton aria-label={title} data-disk-sheet={workspace.id} data-disk-state={model.state} data-disk-filter={filter}>
          <DialogHeader>
            <DialogTitle>{title}</DialogTitle>
            <DialogDescription className="text-caption text-muted-foreground">
              {workspace.label} · {model.rows.length} 체크아웃 · 크기는 할당된 블록이고 실제로 비워지는 양은 정리 후 디스크에서 잰다
            </DialogDescription>
          </DialogHeader>
          <div className="flex min-h-0 flex-1 flex-col overflow-auto px-lg py-md">{body}</div>
          {listing ? (
            <DialogFooter className="items-start justify-between border-t border-border pt-md" data-disk-footer={footer.empty ? "empty" : "ready"}>
              <div className="flex min-w-0 flex-col gap-xxs">
                <span className="text-subhead font-semibold tabular-nums text-foreground" data-disk-summary="true">
                  {footer.summary}
                </span>
                {footer.destructive ? (
                  <span className="flex items-center gap-xs text-body text-destructive" data-disk-warning="worktree">
                    <AlertTriangleIcon aria-hidden="true" className="size-(--size-icon)" />
                    {footer.destructive}
                  </span>
                ) : null}
                {footer.note ? (
                  <span className="text-body text-muted-foreground" data-disk-note="dependencies">
                    {footer.note}
                  </span>
                ) : null}
              </div>
              <div className="flex items-center gap-sm">
                <Button variant="ghost" onClick={close} data-disk-cancel="true">
                  취소
                </Button>
                <Button onClick={press} disabled={footer.empty || model.state !== "ready" || sent} data-disk-clean="true">
                  <Trash2Icon aria-hidden="true" />
                  {model.state === "busy" && elsewhere ? BUSY_TEXT[elsewhere] : "정리"}
                </Button>
              </div>
            </DialogFooter>
          ) : null}
        </DialogContent>
      </Dialog>
      <AlertDialog open={confirming} onOpenChange={(next) => { if (!next) setStep("table"); }}>
        <AlertDialogContent data-disk-confirm="true">
          <AlertDialogHeader>
            <AlertDialogTitle>워크트리 {plan.worktrees.length}개를 폴더째 지운다</AlertDialogTitle>
            <AlertDialogDescription>되돌릴 수 없다. 브랜치와 Git 기록은 남는다.</AlertDialogDescription>
          </AlertDialogHeader>
          <ul className="flex flex-col gap-xxs font-mono text-body text-foreground" data-disk-confirm-list="true">
            {plan.worktrees.map((worktree) => (
              <li key={worktree.path} className="flex min-w-0 items-baseline justify-between gap-md">
                <span className="min-w-0 truncate">{worktree.label}</span>
                <span className="shrink-0 text-muted-foreground">{formatBytes(worktree.bytes)}</span>
              </li>
            ))}
          </ul>
          {plan.cells.length > 0 ? (
            <p className="text-body text-subtle-foreground" data-disk-confirm-cells="true">
              다른 체크아웃의 캐시 칸 {plan.cells.length}개 · {formatBytes(plan.cells.reduce((sum, cell) => sum + cell.bytes, 0))}도 함께 비운다
            </p>
          ) : null}
          <AlertDialogFooter>
            {/* Plain buttons, not Action and Cancel: neither is the default and neither closes the sheet behind. */}
            <Button variant="secondary" onClick={() => setStep("table")} data-disk-confirm-back="true">
              돌아가기
            </Button>
            <Button variant="destructive" onClick={run} data-disk-confirm-run="true">
              워크트리 {plan.worktrees.length}개와 캐시 정리
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}

// --- the table ----------------------------------------------------------------------

function TableView({
  workspace,
  model,
  filter,
  counts,
  layout,
  shown,
  selection,
  onSelection,
  onFilter,
  foldOpen,
  onFold,
  onRetry,
}: {
  workspace: Workspace;
  model: SheetModel;
  filter: Filter;
  counts: Record<Filter, number>;
  layout: ReturnType<typeof layoutRows>;
  shown: SheetRow[];
  selection: Selection;
  onSelection: (next: Selection) => void;
  onFilter: (next: Filter) => void;
  foldOpen: boolean;
  onFold: () => void;
  onRetry: () => void;
}) {
  const all = bundleRefs(shown, selection, ["build_cache", "dependencies"]);
  const columnRefs = (column: Column) => bundleRefs(shown, selection, [column]);
  const measured = model.rows.some((row) => row.measure !== "pending");
  return (
    <div className="flex min-w-0 flex-col gap-md">
      <UsageBar workspace={workspace} />
      {model.state === "unreadable" ? (
        <p className="flex items-center gap-sm text-body text-warning" data-disk-unreadable="true">
          지금 쓰는 중인지 확인할 수 없다
          <Button variant="ghost" size="sm" onClick={onRetry} data-disk-retry="true">
            <RefreshCwIcon aria-hidden="true" />
            다시
          </Button>
        </p>
      ) : null}
      <div className="flex flex-wrap items-center justify-between gap-md">
        <ToggleGroup type="single" value={filter} onValueChange={(value) => value && onFilter(value as Filter)} aria-label="체크아웃 필터" data-disk-filters="true">
          {FILTERS.map((name) => (
            <ToggleGroupItem key={name} value={name} data-disk-filter-item={name}>
              {FILTER_LABEL[name]}
              <span className="font-mono text-caption text-muted-foreground">{counts[name]}</span>
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
        <span className="text-caption text-muted-foreground">체크박스는 보이는 행에만 닿는다</span>
      </div>
      {shown.length === 0 && measured ? (
        <div className="flex flex-col items-center gap-sm py-xl text-body text-muted-foreground" data-disk-empty="true">
          이 필터에 맞는 체크아웃이 없다
          <Button variant="secondary" size="sm" onClick={() => onFilter("all")} data-disk-show-all="true">
            전체 보기
          </Button>
        </div>
      ) : (
        <div role="table" aria-label="체크아웃별 디스크" className="flex min-w-0 flex-col" data-disk-table="true">
          <div role="row" className="grid items-end gap-x-sm border-b border-border pb-sm" style={{ gridTemplateColumns: GRID }}>
            <BundleBox state={bundleState(selection, all)} label="보이는 행의 빌드 캐시와 의존성 전부" onToggle={() => onSelection(toggleBundle(selection, all))} data="all" />
            <span role="columnheader" className="text-caption text-subtle-foreground">
              체크아웃 · 크기순
            </span>
            {(["build_cache", "dependencies", "worktree"] as const).map((column) => (
              <ColumnHead key={column} column={column} state={model.state} rows={shown} selection={selection} refs={columnRefs(column)} onToggle={() => onSelection(toggleBundle(selection, columnRefs(column)))} />
            ))}
            <span role="columnheader" className="flex flex-col items-end gap-xxs text-caption text-subtle-foreground">
              기타
              <span className="font-mono text-muted-foreground">{formatBytes(shown.reduce((sum, row) => sum + (row.other?.bytes ?? 0), 0))}</span>
              <span className="text-muted-foreground">지우지 않음</span>
            </span>
            <span role="columnheader" className="flex flex-col items-end gap-xxs text-caption text-subtle-foreground" data-disk-column="total">
              합계
              <span className="font-mono text-muted-foreground" data-disk-total-head="true">{formatBytes(shown.reduce((sum, row) => sum + (row.total ?? 0), 0))}</span>
            </span>
          </div>
          {layout.main ? <Row row={layout.main} selection={selection} onSelection={onSelection} /> : null}
          {layout.big.map((row) => (
            <Row key={row.path} row={row} selection={selection} onSelection={onSelection} />
          ))}
          {layout.small.length > 0 ? <FoldedRows layout={layout} open={foldOpen} onToggle={onFold} selection={selection} onSelection={onSelection} /> : null}
        </div>
      )}
    </div>
  );
}

function UsageBar({ workspace }: { workspace: Workspace }) {
  const disk = workspace.disk;
  const layers = disk?.layers;
  const size = entranceBytes(workspace);
  if (!disk || !size || !layers) return null;
  const parts = [
    { key: "build_cache", label: "빌드 캐시", bytes: layers.build_cache },
    { key: "dependencies", label: "의존성", bytes: layers.dependencies },
    { key: "other", label: "기타", bytes: layers.other },
    { key: "source", label: "소스 · .git", bytes: layers.source + layers.shared_git },
  ] as const;
  return (
    <div className="flex flex-col gap-xs" data-disk-usage="true">
      <div className="flex items-baseline justify-between gap-md text-caption text-subtle-foreground">
        <span>
          이 프로젝트 <span className="font-mono text-foreground">{size.partial ? "≥ " : ""}{formatBytes(size.bytes)}</span>
        </span>
        {disk.free_bytes != null ? <span className="font-mono" data-disk-free="true">디스크 여유 {formatBytes(disk.free_bytes)}</span> : null}
      </div>
      <div className="flex h-(--lens-bar-height) w-full gap-px overflow-hidden rounded-xs bg-muted" role="img" aria-label={parts.map((part) => `${part.label} ${formatBytes(part.bytes)}`).join(", ")}>
        {parts.map((part) => (
          <span key={part.key} className={LAYER_DOT[part.key]} style={{ flexGrow: part.bytes, flexBasis: 0 }} />
        ))}
      </div>
      <div className="flex flex-wrap gap-md text-caption text-muted-foreground">
        {parts.map((part) => (
          <span key={part.key} className="inline-flex items-center gap-xs">
            <span aria-hidden="true" className={cn("size-(--size-status-mark) rounded-full", LAYER_DOT[part.key])} />
            {part.label}
          </span>
        ))}
      </div>
    </div>
  );
}

function BundleBox({ state, label, onToggle, data }: { state: BundleState; label: string; onToggle: () => void; data: string }) {
  return (
    <Checkbox
      checked={state === "indeterminate" ? "indeterminate" : state === "checked"}
      disabled={state === "none"}
      onCheckedChange={onToggle}
      aria-label={label}
      data-disk-bundle={data}
      data-disk-bundle-state={state}
    />
  );
}

function ColumnHead({ column, state: sheet, rows, selection, refs, onToggle }: { column: Column; state: SheetState; rows: SheetRow[]; selection: Selection; refs: CellRef[]; onToggle: () => void }) {
  const state = bundleState(selection, refs);
  const selected = refs.filter((ref) => isChecked(selection, ref)).length;
  const bytes = rows.reduce((sum, row) => sum + (column === "worktree" ? (row.total ?? 0) * (row.isMain ? 0 : 1) : row.cache[column].bytes), 0);
  return (
    <div role="columnheader" className="flex flex-col gap-xxs" data-disk-column={column}>
      <span className="flex items-center gap-xs text-caption font-medium text-foreground">
        <BundleBox state={state} label={`보이는 행의 ${LAYER_LABEL[column]} 전부`} onToggle={onToggle} data={column} />
        {LAYER_LABEL[column]}
      </span>
      <span className="pl-lg font-mono text-caption text-muted-foreground">{formatBytes(bytes)}</span>
      <span className="pl-lg text-caption text-muted-foreground">{sheet === "pending" && refs.length === 0 ? "확인 중…" : selected > 0 ? `${selected}곳 선택됨` : refs.length > 0 ? `고를 수 있는 ${refs.length}곳` : "고를 수 있는 곳 없음"}</span>
    </div>
  );
}

function Row({ row, selection, onSelection, nested = false }: { row: SheetRow; selection: Selection; onSelection: (next: Selection) => void; nested?: boolean }) {
  const caches = bundleRefs([row], selection, ["build_cache", "dependencies"]);
  const state = bundleState(selection, caches);
  const included = selection.worktrees.has(row.path);
  const pr = row.checkout.pull_request ? prChip(row.checkout.pull_request) : null;
  const dimmed = row.measure === "unavailable";
  return (
    <div
      role="row"
      className={cn("group grid items-center gap-x-sm border-b border-border py-sm hover:bg-muted", dimmed && "opacity-(--opacity-dimmed)", nested && "bg-card")}
      style={{ gridTemplateColumns: GRID }}
      data-disk-row={row.path}
      data-disk-bucket={row.bucket}
      data-disk-measure={row.measure}
    >
      <span role="cell">
        {included && state === "none" ? (
          <Checkbox checked disabled aria-label={`${row.label} 캐시는 워크트리에 포함됨`} data-disk-bundle="row" data-disk-bundle-state="included" />
        ) : (
          <BundleBox state={state} label={`${row.label} 빌드 캐시와 의존성`} onToggle={() => onSelection(toggleBundle(selection, caches))} data="row" />
        )}
      </span>
      <span role="rowheader" className="flex min-w-0 items-center gap-xs text-body text-foreground">
        {row.isMain ? <HomeIcon aria-hidden="true" className="size-(--size-icon) shrink-0 text-muted-foreground" /> : <GitBranchIcon aria-hidden="true" className="size-(--size-icon) shrink-0 text-muted-foreground" />}
        <Hint label={row.measure === "unavailable" ? "크기를 재지 못함" : row.label} reveals>
          <span className="min-w-0 truncate" data-disk-label="true">
            {row.label}
          </span>
        </Hint>
        {pr ? (
          <span className={cn("shrink-0 font-mono text-caption", PR_TONE[pr.tone])} data-disk-pr={pr.number}>
            #{pr.number}
          </span>
        ) : null}
        {row.inUse ? (
          <span className="shrink-0 text-caption text-warning" data-disk-in-use="true">
            {row.inUse}
          </span>
        ) : null}
      </span>
      {(["build_cache", "dependencies"] as const).map((layer) => (
        <CacheCellView key={layer} row={row} layer={layer} selection={selection} onSelection={onSelection} />
      ))}
      <WorktreeCellView row={row} selection={selection} onSelection={onSelection} />
      <OtherCellView row={row} />
      <span role="cell" className="text-right font-mono text-body text-subtle-foreground" data-disk-total={row.path}>
        {row.measure === "measured" && row.total !== null ? formatBytes(row.total) : row.measure === "pending" ? <Skeleton /> : ""}
      </span>
    </div>
  );
}

function Skeleton() {
  return <span aria-hidden="true" data-disk-skeleton="true" className="inline-block h-(--size-badge-height) w-(--spacing-xxxl) animate-pulse rounded-xs bg-muted" />;
}

function CacheCellView({ row, layer, selection, onSelection }: { row: SheetRow; layer: CacheLayer; selection: Selection; onSelection: (next: Selection) => void }) {
  const cell: CacheCell = row.cache[layer];
  const ref: CellRef = { path: row.path, column: layer };
  const included = isIncluded(selection, ref);
  if (row.measure === "pending") {
    return (
      <span role="cell" className="flex items-center gap-xs" data-disk-cell={`${row.path}:${layer}`} data-disk-cell-state="pending">
        <Checkbox disabled aria-label={`${row.label} ${LAYER_LABEL[layer]}`} />
        <Skeleton />
      </span>
    );
  }
  if (row.measure === "unavailable") return <span role="cell" data-disk-cell={`${row.path}:${layer}`} data-disk-cell-state="unavailable" />;
  if (cell.bytes === 0) {
    return (
      <span role="cell" className="text-muted-foreground" data-disk-cell={`${row.path}:${layer}`} data-disk-cell-state="empty">
        -
      </span>
    );
  }
  const detail = `폴더 ${cell.folders}개${cell.largest_name ? ` · 가장 큰 폴더 ${cell.largest_name}` : ""}`;
  const name = `${row.label} ${LAYER_LABEL[layer]} ${formatBytes(cell.bytes)}`;
  const stateName = included ? "included" : cell.selectable ? "selectable" : "blocked";
  const box = (
    <Checkbox
      checked={included || isChecked(selection, ref)}
      disabled={!cell.selectable || included}
      onCheckedChange={() => onSelection(toggleCell(selection, ref))}
      aria-label={name}
      data-disk-check={`${row.path}:${layer}`}
    />
  );
  return (
    <span role="cell" className={cn("flex min-w-0 items-center gap-xs", (included || !cell.selectable) && "opacity-(--opacity-secondary)")} data-disk-cell={`${row.path}:${layer}`} data-disk-cell-state={stateName}>
      {cell.selectable || included ? (
        box
      ) : (
        <Hint label={cell.why ?? detail} reveals>
          <span className="inline-flex">{box}</span>
        </Hint>
      )}
      <Hint label={cell.why && !cell.selectable ? `${cell.why}\n${detail}` : detail} reveals>
        <span className="font-mono text-body text-foreground" data-disk-bytes="true">
          {formatBytes(cell.bytes)}
        </span>
      </Hint>
    </span>
  );
}

function WorktreeCellView({ row, selection, onSelection }: { row: SheetRow; selection: Selection; onSelection: (next: Selection) => void }) {
  const key = `${row.path}:worktree`;
  if (row.isMain || !row.worktree) return <span role="cell" data-disk-cell={key} data-disk-cell-state="none" />;
  if (row.measure === "pending") {
    return (
      <span role="cell" className="flex items-center gap-xs" data-disk-cell={key} data-disk-cell-state="pending">
        <Checkbox disabled aria-label={`${row.label} 워크트리`} />
        <Skeleton />
      </span>
    );
  }
  if (row.measure === "unavailable" || row.total === null) return <span role="cell" data-disk-cell={key} data-disk-cell-state="unavailable" />;
  const ref: CellRef = { path: row.path, column: "worktree" };
  if (!row.worktree.selectable) {
    return (
      <span role="cell" className="flex min-w-0 items-center gap-xs text-muted-foreground" data-disk-cell={key} data-disk-cell-state="blocked">
        <Hint label={row.worktree.why ?? "지울 수 없음"}>
          <LockIcon aria-hidden="true" className="size-(--size-icon)" data-disk-lock="true" />
        </Hint>
      </span>
    );
  }
  return (
    <span role="cell" className="flex min-w-0 items-center gap-xs" data-disk-cell={key} data-disk-cell-state="selectable">
      <Checkbox
        checked={isChecked(selection, ref)}
        onCheckedChange={() => onSelection(toggleCell(selection, ref))}
        aria-label={`${row.label} 워크트리 ${formatBytes(row.total)}`}
        data-disk-check={key}
      />
      <span className={cn("font-mono text-body", isChecked(selection, ref) ? "text-destructive" : "text-foreground")} data-disk-bytes="true">
        {formatBytes(row.total)}
      </span>
    </span>
  );
}

function OtherCellView({ row }: { row: SheetRow }) {
  const other = row.other;
  if (row.measure !== "measured" || !other || other.bytes === 0) return <span role="cell" data-disk-cell={`${row.path}:other`} />;
  const hint = `${other.largest_name ? `가장 큰 폴더 ${other.largest_name}\n` : ""}hide가 모르는 폴더라 지우지 않는다`;
  return (
    <span role="cell" className="text-right" data-disk-cell={`${row.path}:other`}>
      <Hint label={hint} reveals>
        <span className="font-mono text-body text-subtle-foreground">{formatBytes(other.bytes)}</span>
      </Hint>
    </span>
  );
}

function FoldedRows({ layout, open, onToggle, selection, onSelection }: { layout: ReturnType<typeof layoutRows>; open: boolean; onToggle: () => void; selection: Selection; onSelection: (next: Selection) => void }) {
  const refs = bundleRefs(layout.small, selection, ["build_cache", "dependencies"]);
  return (
    <>
      <div role="row" className="flex items-center gap-sm border-b border-border py-sm" data-disk-fold={open ? "open" : "closed"}>
        <BundleBox state={bundleState(selection, refs)} label={`작은 체크아웃 ${layout.small.length}곳의 빌드 캐시와 의존성`} onToggle={() => onSelection(toggleBundle(selection, refs))} data="fold" />
        <button type="button" aria-expanded={open} className="flex items-center gap-xs rounded-xs text-body text-subtle-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring" onClick={onToggle} data-disk-fold-toggle="true">
          <ChevronRightIcon aria-hidden="true" className={cn("size-(--size-icon) transition-transform", open && "rotate-90")} />
          작은 체크아웃 {layout.small.length} · <span className="font-mono">{formatBytes(layout.smallBytes)}</span>
        </button>
      </div>
      {open ? layout.small.map((row) => <Row key={row.path} row={row} selection={selection} onSelection={onSelection} nested />) : null}
    </>
  );
}

// --- running and result ---------------------------------------------------------------

function RunningView({ progress }: { progress: { done: number; total: number } | null }) {
  return (
    <div className="flex flex-col gap-sm py-lg" data-disk-running="true">
      <span className="flex items-center gap-sm text-title font-semibold text-foreground">
        <Loader2Icon aria-hidden="true" className="size-(--size-icon) animate-spin text-muted-foreground" />
        비우는 중{progress ? ` · ${progress.done}/${progress.total}` : ""}
      </span>
      <span className="text-body text-muted-foreground">닫아도 정리는 계속된다. 다시 열면 진행이나 결과가 보인다.</span>
    </div>
  );
}

const OUTCOME_TEXT: Record<ResultLine["outcome"], string> = { removed: "지움", skipped: "건너뜀", failed: "실패" };

function ResultView({ workspace, onReview, onClose }: { workspace: Workspace; onReview: () => void; onClose: () => void }) {
  const cleanup = workspace.cleanup!;
  const lines = resultLines(cleanup);
  const freed = freedFree(cleanup);
  return (
    <div className="flex flex-col gap-md" data-disk-result="true">
      <div className="flex flex-wrap items-baseline justify-between gap-md">
        <div className="flex flex-col gap-xxs">
          <span className="text-caption text-muted-foreground">디스크에서 잰 여유</span>
          <span className="flex items-baseline gap-sm font-mono text-headline font-semibold text-foreground" data-disk-free-change="true">
            {cleanup.free_before != null ? gigabytes(cleanup.free_before) : "-"}
            <span aria-hidden="true">→</span>
            {cleanup.free_after != null ? gigabytes(cleanup.free_after) : "…"}
          </span>
        </div>
        <span className="font-mono text-caption text-subtle-foreground" data-disk-allocated="true">
          할당 합계 {formatBytes(allocatedTotal(lines))}
          {freed !== null ? ` · 실제로 늘어난 여유 ${signedBytes(freed)}` : ""}
        </span>
      </div>
      <ul className="flex flex-col divide-y divide-border border-y border-border" data-disk-result-lines="true">
        {lines.map((line) => (
          <li key={line.key} className="grid items-baseline gap-x-md py-sm text-body" style={{ gridTemplateColumns: "minmax(0, 1.4fr) minmax(0, 1fr) minmax(0, 1.6fr) minmax(0, 0.5fr)" }} data-disk-result-line={line.outcome}>
            <span className="truncate text-foreground">{line.label}</span>
            <span className="truncate text-muted-foreground">{line.what}</span>
            <span className={cn("truncate text-caption", line.outcome === "removed" ? "text-success" : line.outcome === "failed" ? "text-destructive" : "text-warning")}>
              {OUTCOME_TEXT[line.outcome]}
              {line.reason ? ` · ${line.outcome === "removed" ? "남긴 폴더: " : ""}${line.reason}` : ""}
            </span>
            <span className="text-right font-mono text-caption text-subtle-foreground">{line.bytes !== null ? formatBytes(line.bytes) : ""}</span>
          </li>
        ))}
        {lines.length === 0 ? <li className="py-sm text-body text-muted-foreground">정리한 칸이 없다</li> : null}
      </ul>
      <div className="flex justify-end gap-sm">
        <Button variant="ghost" onClick={onReview} data-disk-review-again="true">
          <RefreshCwIcon aria-hidden="true" />
          다시 검토
        </Button>
        <Button variant="secondary" onClick={onClose} data-disk-close="true">
          닫기
        </Button>
      </div>
    </div>
  );
}
