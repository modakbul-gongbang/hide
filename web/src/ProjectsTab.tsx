import { AlertTriangleIcon, ArrowRightIcon, ChevronRightIcon, ExternalLinkIcon, FolderGit2Icon, FolderIcon, LoaderCircleIcon, PlusIcon, XIcon } from "lucide-react";
import { useEffect, useMemo } from "react";
import type { Actions } from "./actions";
import { Button } from "./components/ui/button";
import { Hint } from "./components/ui/tooltip";
import { layerLines } from "./diskCleanup";
import { LAYER_DOT, openDiskCleanup } from "./DiskCleanupSheet";
import { readsHere } from "./devices";
import { GithubIcon } from "./components/github-icon";
import { formatBytes } from "./i18n/format";
import { useInterfaceTranslation } from "./i18n/client";
import { requireInterfaceLanguage } from "./i18n/locale";
import { cn } from "./lib/utils";
import { ageWords } from "./overviewLens";
import { diskSummary, orderedRows, projectRows, type NowCounts, type ProjectRowModel } from "./overviewProjects";
import { backlogOf } from "./overviewBacklog";
import type { BoardProject } from "./projectBoard";
import { machineName } from "./screenMachine";
import { settingsTarget } from "./projectSettings";
import { useShellStore } from "./store";
import { useUiStore, type OverviewPanel } from "./ui";
import { primaryCheckout } from "./workspaceManage";

// The Projects tab (PRD overview-device-tasks D-14, D-15, D-24..D-28,
// B47-B57): the device's disk on one line, then a row per project with its
// work now, its backlog, its disk as the sweep measures it while the tab
// shows, and what is left to clean; the quiet ones fold at the bottom.

const LINK = "relative z-1 inline-flex items-center gap-xxs rounded-xs px-xxs outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring";
const COLUMNS = "grid grid-cols-[minmax(0,2fr)_minmax(0,2fr)_minmax(0,0.6fr)_minmax(0,1.4fr)_minmax(0,1fr)_minmax(0,0.8fr)_auto] items-center gap-md";

export function OverviewProjects({ projects, deviceId, actions }: { projects: readonly BoardProject[]; deviceId: string; actions: Actions }) {
  const { t, i18n } = useInterfaceTranslation();
  const language = requireInterfaceLanguage(i18n.language);
  const devices = useShellStore((s) => s.rest?.navigator?.devices);
  const bySize = useUiStore((s) => s.overview.bySize);
  const quietOpen = useUiStore((s) => s.overview.open.includes("quiet"));
  const rows = useMemo(() => projectRows(projects), [projects]);
  const { active, quiet } = useMemo(() => orderedRows(rows, bySize), [rows, bySize]);
  const measured = useMemo(() => rows.filter((row) => readsHere(row.project, devices)), [rows, devices]);
  const summary = useMemo(() => diskSummary(measured), [measured]);
  const largest = Math.max(1, ...rows.map((row) => row.disk.bytes ?? 0));
  const dialed = measured.length === 0 && rows.length > 0;
  const name = machineName(devices, deviceId, t);
  // The sweep measures what the tab shows, in its order; leaving the tab stops what has not started (B53).
  const watched = useMemo(() => [...active, ...(quietOpen || bySize ? quiet : [])].filter((row) => readsHere(row.project, devices)).map((row) => row.project.id), [active, quiet, quietOpen, bySize, devices]);
  const order = watched.join("\n");
  useEffect(() => {
    actions.watchDisk(order ? order.split("\n") : []);
  }, [actions, order]);
  useEffect(() => () => actions.watchDisk([]), [actions]);
  const quietBytes = quiet.reduce<number | null>((sum, row) => (row.disk.bytes === null ? sum : (sum ?? 0) + row.disk.bytes), null);
  return (
    <div className="flex flex-col gap-sm px-lg py-md" data-overview-projects="true">
      {dialed ? null : (
        <p className="flex flex-wrap items-center gap-x-sm font-mono text-caption text-subtle-foreground" data-projects-disk-line="true">
          {summary.free !== null && summary.total !== null ? (
            <span>
              {t("overviewProjects.diskFree", { device: name })}{" "}
              <span className={cn(summary.low && "text-warning")} data-projects-free-low={summary.low || undefined}>{formatBytes(language, summary.free)}</span>
              {" / "}
              {formatBytes(language, summary.total)}
            </span>
          ) : null}
          {summary.sum !== null ? <span>· {t("overviewProjects.sum", { size: formatBytes(language, summary.sum) })}</span> : null}
          {summary.reclaimable > 0 ? <span className="text-pr-merged">· {t("overviewProjects.reclaimable", { size: formatBytes(language, summary.reclaimable) })}</span> : null}
          {summary.measuring ? <span className="inline-flex items-center gap-xxs text-muted-foreground" role="status" data-projects-measuring="true"><LoaderCircleIcon aria-hidden="true" className="size-(--size-icon-sm) animate-spin" />{t("overviewProjects.measuring")}</span> : null}
        </p>
      )}
      <div role="table" aria-label={t("overview.projects")} className="flex flex-col">
        <div role="row" className={cn(COLUMNS, "border-b border-border px-sm py-xs text-micro text-muted-foreground")}>
          <span role="columnheader">{t("overviewProjects.colProject")}</span>
          <span role="columnheader">{t("overviewProjects.colNow")}</span>
          <span role="columnheader">{t("overviewProjects.colBacklog")}</span>
          <span role="columnheader">
            <button type="button" aria-pressed={bySize} className={cn(LINK, bySize && "text-foreground")} onClick={() => useUiStore.getState().setOverview({ bySize: !bySize })} data-projects-sort="size">
              {t("overviewProjects.colDisk")}{bySize ? " ↓" : ""}
            </button>
          </span>
          <span role="columnheader">{t("overviewProjects.colCare")}</span>
          <span role="columnheader">{t("overviewProjects.colActivity")}</span>
          <span role="columnheader" className="sr-only">{t("overviewTasks.open")}</span>
        </div>
        {active.map((row) => <ProjectRow key={row.project.id} row={row} largest={largest} actions={actions} />)}
        {quiet.length > 0 ? (
          <>
            <button type="button" aria-expanded={quietOpen} className="flex items-center gap-xxs rounded-sm px-sm py-xs text-left text-caption text-muted-foreground outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring" onClick={() => useUiStore.getState().setOverview({ open: quietOpen ? useUiStore.getState().overview.open.filter((fold) => fold !== "quiet") : [...useUiStore.getState().overview.open, "quiet"] })} data-projects-quiet={quiet.length}>
              {quietBytes !== null ? t("overviewProjects.quiet", { count: quiet.length, size: formatBytes(language, quietBytes) }) : t("overviewProjects.quietNoSize", { count: quiet.length })}
              <ChevronRightIcon aria-hidden="true" className={cn("size-(--size-icon-sm)", quietOpen && "rotate-90")} />
            </button>
            {quietOpen ? quiet.map((row) => <ProjectRow key={row.project.id} row={row} largest={largest} actions={actions} />) : null}
          </>
        ) : null}
      </div>
    </div>
  );
}

/** `Needs You N · 일하는 중 N · 기다림 N · 끝남 N` with its colour bar, `쉬는 중 N` when nothing moves, `일감 없음` with no task (B48). */
function NowCell({ now }: { now: NowCounts }) {
  const { t } = useInterfaceTranslation();
  if (now.total === 0) return <span className="text-muted-foreground">{t("overviewProjects.noTasks")}</span>;
  if (now.total === now.resting) return <span className="text-muted-foreground">{t("overviewProjects.restingCount", { count: now.resting })}</span>;
  const parts = [
    { key: "needs", value: now.needs, tone: "bg-warning" },
    { key: "working", value: now.working, tone: "bg-agent-working" },
    { key: "waiting", value: now.waiting, tone: "bg-muted-foreground" },
    { key: "done", value: now.done, tone: "bg-success" },
  ];
  return (
    <span className="flex min-w-0 flex-col gap-xxs">
      <span aria-hidden="true" className="flex h-(--size-hairline) w-full gap-px overflow-hidden rounded-full bg-muted">
        {parts.filter((part) => part.value > 0).map((part) => <span key={part.key} className={part.tone} style={{ flexGrow: part.value }} />)}
      </span>
      <span className="truncate text-micro text-subtle-foreground">{t("overviewProjects.now", { needs: now.needs, working: now.working, waiting: now.waiting, done: now.done })}</span>
    </span>
  );
}

/** The disk cell (B51, B53-B56): a bar against the device's largest project, its purple share what a cleanup frees, and the size that opens the cleanup sheet. */
function DiskCell({ row, largest }: { row: ProjectRowModel; largest: number }) {
  const { t, i18n } = useInterfaceTranslation();
  const language = requireInterfaceLanguage(i18n.language);
  const devices = useShellStore((s) => s.rest?.navigator?.devices);
  if (!readsHere(row.project, devices)) return <span data-projects-disk="dialed" />;
  const { disk } = row;
  const busy = disk.measuring || disk.queued;
  const size = disk.bytes === null ? "-" : formatBytes(language, disk.bytes);
  const lines = row.folder ? null : layerLines(row.project, t);
  const hint = disk.failure
    ? t("overviewProjects.failed", { reason: disk.failure })
    : row.folder
      ? t("overviewProjects.folderTotal")
      : lines
        ? [...lines.map((line) => `${line.label} ${formatBytes(language, line.bytes)}`), row.reclaimable > 0 ? t("overviewProjects.reclaimable", { size: formatBytes(language, row.reclaimable) }) : null].filter(Boolean).join(" · ")
        : t("overviewProjects.notMeasured");
  const width = disk.bytes === null ? 0 : Math.max(2, (disk.bytes / largest) * 100);
  const freed = disk.bytes ? Math.min(100, (row.reclaimable / disk.bytes) * 100) : 0;
  return (
    <span className={cn("flex min-w-0 items-center gap-xs", busy && "opacity-(--opacity-dimmed)")} data-projects-disk={busy ? "measuring" : disk.failure ? "failed" : disk.bytes === null ? "none" : "measured"}>
      {row.folder ? null : (
        <span aria-hidden="true" className="flex h-(--size-hairline) min-w-0 flex-1 overflow-hidden rounded-full bg-muted">
          <span className="flex h-full bg-muted-foreground" style={{ width: `${width}%` }}>
            <span className="ml-auto h-full bg-pr-merged" style={{ width: `${freed}%` }} />
          </span>
        </span>
      )}
      <Hint label={hint}>
        <button type="button" className={cn(LINK, "shrink-0 font-mono text-caption")} disabled={row.folder || disk.bytes === null} onClick={() => openDiskCleanup(row.project.id, "all")} data-projects-disk-open={row.project.id}>
          {size}
          {disk.failure ? <AlertTriangleIcon aria-label={t("overviewProjects.failed", { reason: disk.failure })} className="size-(--size-icon-sm) text-warning" /> : null}
        </button>
      </Hint>
      {busy ? <LoaderCircleIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0 animate-spin text-muted-foreground" /> : null}
    </span>
  );
}

function ProjectRow({ row, largest, actions }: { row: ProjectRowModel; largest: number; actions: Actions }) {
  const { t, i18n } = useInterfaceTranslation();
  const language = requireInterfaceLanguage(i18n.language);
  const { project } = row;
  const selected = useUiStore((s) => s.overview.panel?.kind === "project" && s.overview.panel.workspaceId === project.id);
  // A device the core dials is cleaned up on that device (B49): the count stays, with nothing to press.
  const cleans = useShellStore((s) => readsHere(project, s.rest?.navigator?.devices));
  const activity = project.last_activity_unix_ms ?? null;
  return (
    <div role="row" className={cn(COLUMNS, "relative rounded-sm px-sm py-xs text-caption hover:bg-accent", selected && "bg-secondary")} data-projects-row={project.id}>
      <button type="button" aria-label={project.label} aria-pressed={selected} className="absolute inset-0 rounded-sm outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring" onClick={() => useUiStore.getState().setOverview({ panel: { kind: "project", workspaceId: project.id } })} />
      <span role="cell" className="pointer-events-none flex min-w-0 items-center gap-xs">
        {row.folder ? <FolderIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0 text-muted-foreground" /> : project.tasks?.source?.kind === "github" ? <GithubIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0 text-muted-foreground" data-projects-github="true" /> : <FolderGit2Icon aria-hidden="true" className="size-(--size-icon-sm) shrink-0 text-muted-foreground" />}
        <span className="min-w-0 truncate font-medium text-foreground">{project.label}</span>
        {row.folder ? <span className="shrink-0 text-micro text-muted-foreground">{t("overviewProjects.folder")}</span> : null}
      </span>
      <span role="cell" className="pointer-events-none min-w-0"><NowCell now={row.now} /></span>
      <span role="cell">
        {row.backlog > 0 ? (
          <button type="button" className={cn(LINK, "font-mono text-subtle-foreground")} onClick={() => { useUiStore.getState().setMainView("backlog"); useUiStore.getState().setOverview({ projectId: project.id, panel: null }); }} data-projects-backlog={row.backlog}>
            {row.backlog}<ChevronRightIcon aria-hidden="true" className="size-(--size-icon-sm)" />
          </button>
        ) : null}
      </span>
      <span role="cell" className="min-w-0"><DiskCell row={row} largest={largest} /></span>
      <span role="cell">
        {row.merged > 0 && cleans ? (
          <button type="button" className={cn(LINK, "text-pr-merged")} onClick={() => openDiskCleanup(project.id, "done")} data-projects-merged={row.merged}>
            {t("overviewProjects.merged", { count: row.merged })}
          </button>
        ) : row.merged > 0 ? <span className="pointer-events-none text-pr-merged" data-projects-merged={row.merged}>{t("overviewProjects.mergedThere", { count: row.merged })}</span> : null}
      </span>
      <span role="cell" className="pointer-events-none truncate font-mono text-micro text-muted-foreground">{activity !== null ? ageWords(language, Date.now() - activity, t) : ""}</span>
      <span role="cell">
        <Hint label={t("overviewTasks.openHint", { project: project.label })}>
          <button type="button" className={cn(LINK, "text-subtle-foreground")} aria-label={t("overviewTasks.openHint", { project: project.label })} onClick={() => actions.openProjectSessions(project.device_id, project.id)} data-projects-open={project.id}>
            <ExternalLinkIcon aria-hidden="true" className="size-(--size-icon-sm)" />
          </button>
        </Hint>
      </span>
    </div>
  );
}

/** One project beside the tab (B57). */
export function ProjectPanel({ panel, projects, actions }: { panel: Extract<OverviewPanel, { kind: "project" }>; projects: readonly BoardProject[]; deviceId: string; actions: Actions }) {
  const { t, i18n } = useInterfaceTranslation();
  const language = requireInterfaceLanguage(i18n.language);
  const devices = useShellStore((s) => s.rest?.navigator?.devices);
  const row = useMemo(() => projectRows(projects.filter(({ workspace }) => workspace.id === panel.workspaceId))[0] ?? null, [projects, panel.workspaceId]);
  // The panel states worktrees, merged ones and how far main is behind (B57): it reads the project's worktrees again when it opens.
  useEffect(() => actions.refreshProjectOverview(panel.workspaceId), [actions, panel.workspaceId]);
  const close = () => useUiStore.getState().setOverview({ panel: null });
  const head = (title: string) => (
    <div className="flex shrink-0 items-center gap-xs px-sm py-xs">
      <h2 className="min-w-0 flex-1 truncate px-xs text-subhead font-semibold">{title}</h2>
      <Hint label={t("record.close")}><Button variant="ghost" size="icon-sm" aria-label={t("record.close")} onClick={close}><XIcon /></Button></Hint>
    </div>
  );
  if (!row) return <div className="flex min-h-0 flex-1 flex-col">{head("")}<p className="px-md text-caption text-muted-foreground">{t("overview.projectMissing")}</p></div>;
  const { project } = row;
  const backlog = backlogOf(project);
  const lines = row.folder ? null : layerLines(project, t);
  const worktrees = project.checkouts.filter((checkout) => checkout.is_worktree && checkout.is_primary !== true).length;
  const behind = primaryCheckout(project)?.worktree?.behind_upstream ?? null;
  const branch = primaryCheckout(project)?.branch ?? project.default_branch ?? "main";
  const cleans = readsHere(project, devices);
  const activity = project.last_activity_unix_ms ?? null;
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-project-panel={project.id}>
      {head(project.label)}
      <div className="min-h-0 flex-1 space-y-md overflow-y-auto px-md pb-md text-caption">
        <p className="flex flex-wrap items-center gap-x-sm text-micro text-muted-foreground">
          <span className="truncate font-mono">{project.path}</span>
          {row.folder ? <span>{t("overviewProjects.folder")}</span> : null}
          {activity !== null ? <span>· {ageWords(language, Date.now() - activity, t)}</span> : null}
        </p>
        <div className="flex flex-wrap gap-xs">
          <Button size="sm" onClick={() => actions.openProjectSessions(project.device_id, project.id)}><ExternalLinkIcon />{t("overviewTasks.open")}</Button>
          <Button size="sm" variant="outline" onClick={() => actions.openStartPanel()}><PlusIcon />{t("overviewTasks.newAgent")}</Button>
        </div>
        <section className="space-y-xs">
          <h3 className="font-medium text-subtle-foreground">{t("overviewProjects.colNow")}</h3>
          <NowCell now={row.now} />
          <button type="button" className={cn(LINK, "text-subtle-foreground")} onClick={() => { useUiStore.getState().setMainView("tasks"); useUiStore.getState().setOverview({ projectId: project.id, panel: null }); }}>
            {t("overviewProjects.panel.tasksOnly")}<ArrowRightIcon aria-hidden="true" className="size-(--size-icon-sm)" />
          </button>
        </section>
        <section className="space-y-xs">
          <h3 className="font-medium text-subtle-foreground">{t("overviewProjects.colBacklog")}</h3>
          <p className="flex items-center gap-sm">
            <span>{project.tasks?.source?.label ?? ""}</span>
            <span className="text-muted-foreground">{t("backlog.openCount", { count: backlog.rows.length })}</span>
            <button type="button" className={cn(LINK, "text-subtle-foreground")} onClick={() => { useUiStore.getState().setMainView("backlog"); useUiStore.getState().setOverview({ projectId: project.id, panel: null }); }}>
              {t("overviewProjects.panel.view")}<ArrowRightIcon aria-hidden="true" className="size-(--size-icon-sm)" />
            </button>
          </p>
        </section>
        {cleans ? (
          <section className="space-y-xs" data-project-panel-storage="true">
            <h3 className="font-medium text-subtle-foreground">{t("overviewProjects.panel.storage")} {row.disk.bytes !== null ? <span className="font-mono text-muted-foreground">{formatBytes(language, row.disk.bytes)}</span> : null}</h3>
            {lines ? (
              <>
                <span aria-hidden="true" className="flex h-(--lens-bar-height) w-full overflow-hidden rounded-xs bg-muted">
                  {lines.map((line) => <span key={line.key} className={LAYER_DOT[line.key]} style={{ flexGrow: line.bytes }} />)}
                </span>
                <ul className="space-y-xxs">
                  {lines.map((line) => (
                    <li key={line.key} className="flex items-center justify-between gap-md">
                      <span className="inline-flex items-center gap-xs"><span aria-hidden="true" className={cn("size-(--size-status-mark) rounded-full", LAYER_DOT[line.key])} />{line.label}</span>
                      <span className="font-mono">{formatBytes(language, line.bytes)}</span>
                    </li>
                  ))}
                </ul>
              </>
            ) : row.folder ? <p className="text-muted-foreground">{t("overviewProjects.folderTotal")}</p> : null}
          </section>
        ) : null}
        {!row.folder ? (
          <section className="flex items-center gap-sm">
            <span>{t("overviewProjects.panel.worktrees", { count: worktrees })}</span>
            {cleans ? <Button size="sm" variant="ghost" onClick={() => openDiskCleanup(project.id, "all")}>{t("overviewProjects.panel.cleanup")}</Button> : null}
          </section>
        ) : null}
        {behind !== null && behind > 0 ? <p className="text-warning">{t("overviewProjects.panel.behind", { branch, count: behind })}</p> : null}
        <div className="flex flex-wrap gap-xs border-t border-border pt-md">
          <Button size="sm" variant="outline" onClick={(event) => useUiStore.getState().openProjectSettings(settingsTarget(project), event.currentTarget)} data-project-panel-settings={project.id}>{t("overviewProjects.panel.settings")}</Button>
          <Button size="sm" variant="ghost" className="text-destructive" onClick={() => useUiStore.getState().setWorkspaceDialog({ kind: "remove_project", workspaceId: project.id })}>{t("overviewProjects.panel.unregister")}</Button>
        </div>
      </div>
    </div>
  );
}
