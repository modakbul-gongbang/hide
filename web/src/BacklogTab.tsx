import { ChevronDownIcon, ChevronRightIcon, CircleDotIcon, ExternalLinkIcon, FileTextIcon, FlagIcon, ListFilterIcon, LoaderCircleIcon, PlayIcon, TriangleAlertIcon, XIcon } from "lucide-react";
import { useEffect, useMemo, useState, type KeyboardEvent } from "react";
import type { Actions } from "./actions";
import { Button } from "./components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from "./components/ui/dropdown-menu";
import { Input } from "./components/ui/input";
import { Popover, PopoverContent, PopoverTrigger } from "./components/ui/popover";
import { ToggleGroup, ToggleGroupItem } from "./components/ui/toggle-group";
import { Hint } from "./components/ui/tooltip";
import { readsHere } from "./devices";
import { useInterfaceTranslation } from "./i18n/client";
import { requireInterfaceLanguage } from "./i18n/locale";
import { cn } from "./lib/utils";
import { ageWords } from "./overviewLens";
import { useBacklogOrigin } from "./backlogOrigin";
import { backlogLabels, backlogOf, BACKLOG_SHOWN, backlogTitle, destinationOf, filterRows, pendingItems, startedKeys, titleLeft, type BacklogRow } from "./overviewBacklog";
import { isMemoKey, itemToken, taskSections } from "./overviewTasks";
import { issueDate, type BoardProject } from "./projectBoard";
import { issueSourceChoices } from "./settings";
import { projectIssueSource, settingsTarget } from "./projectSettings";
import type { BacklogItem, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { IssueLabelDot, IssueLabelView } from "./issueMarks";
import { toggledFold, useUiStore, type OverviewPanel } from "./ui";

// The 백로그 tab (PRD overview-device-tasks D-09, D-10, D-21..D-23, B25-B36):
// one section per project in Tasks' order, each with its source chip, an
// input that makes an item at once, the items the core is making or failed
// to make, and the unstarted items newest first. A device the core dials
// keeps no backlog, so each of its sections says so in one line (B36).

const LINK = "inline-flex items-center gap-xxs rounded-xs px-xxs text-caption outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring";

export function OverviewBacklog({ projects, rows, actions }: { projects: readonly BoardProject[]; rows: ReadonlyMap<string, import("./snapshot").AgentRow>; deviceId: string; actions: Actions }) {
  const only = useUiStore((s) => s.overview.projectId);
  const devices = useShellStore((s) => s.rest?.navigator?.devices);
  // The sections stand in Tasks' order (B25).
  const order = useMemo(() => taskSections(projects, rows, "all", "", only).map((section) => section.project), [projects, rows, only]);
  return (
    <div className="flex flex-col gap-lg px-lg py-md" data-overview-backlog="true">
      {order.map((project) => (readsHere(project, devices) ? <BacklogSection key={project.id} project={project} actions={actions} /> : <DialedSection key={project.id} project={project} />))}
    </div>
  );
}

function DialedSection({ project }: { project: Workspace }) {
  const { t } = useInterfaceTranslation();
  return (
    <section aria-label={project.label} data-backlog-section={project.id} data-backlog-dialed="true">
      <h2 className="pb-xs text-subhead font-semibold text-foreground">{project.label}</h2>
      <p className="text-caption text-muted-foreground">{t("backlog.dialed", { project: project.label })}</p>
    </section>
  );
}

/** The project's Issue source menu, as the sidebar's (B34). */
function SourceChip({ project, actions }: { project: Workspace; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const stored = useShellStore((s) => projectIssueSource(s.rest, project));
  const { value, options } = issueSourceChoices(project, stored, t);
  const github = project.tasks?.source?.kind === "github";
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button type="button" className={cn(LINK, "border border-border text-subtle-foreground")} data-backlog-source={project.tasks?.source?.kind ?? "none"}>
          {github ? <CircleDotIcon aria-hidden="true" className="size-(--size-icon-sm)" /> : <FileTextIcon aria-hidden="true" className="size-(--size-icon-sm)" />}
          {t(github ? "backlog.sourceGithub" : "backlog.sourceLocal")}
          <ChevronDownIcon aria-hidden="true" className="size-(--size-icon-sm)" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start">
        <DropdownMenuRadioGroup value={value} onValueChange={(choice) => actions.setProjectSettings(settingsTarget(project), { issue_source: choice as "auto" | "github" | "local" }, crypto.randomUUID())}>
          {options.map((option) => <DropdownMenuRadioItem key={option.id} value={option.id} data-backlog-source-choice={option.id}>{option.label}</DropdownMenuRadioItem>)}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function BacklogSection({ project, actions }: { project: Workspace; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const query = useUiStore((s) => s.overview.backlogQuery);
  const labels = useUiStore((s) => s.overview.backlogLabels);
  const more = useUiStore((s) => s.overview.open.includes(`more:${project.id}`));
  const made = useShellStore((s) => s.rest?.issue_work?.backlog);
  const backlog = useMemo(() => backlogOf(project), [project]);
  const pending = useMemo(() => pendingItems(made, project), [made, project]);
  const rows = useMemo(() => filterRows(backlog.rows, query, labels), [backlog.rows, query, labels]);
  const source = backlog.source;
  const local = source?.kind !== "github";
  const filtering = query.trim() !== "" || labels.length > 0;
  const shown = more || filtering ? rows : rows.slice(0, BACKLOG_SHOWN);
  const hidden = rows.length - shown.length;
  const neverRead = source?.failure && source.last_read_at_unix_ms === null;
  return (
    <section aria-label={project.label} data-backlog-section={project.id}>
      <h2 className="flex min-w-0 flex-wrap items-center gap-sm pb-xs">
        <span className="min-w-0 truncate text-subhead font-semibold text-foreground">{project.label}</span>
        <SourceChip project={project} actions={actions} />
        <span className="text-caption text-muted-foreground" data-backlog-open={backlog.rows.length}>{t("backlog.openCount", { count: backlog.rows.length })}</span>
        {source?.failure ? (
          <Hint label={source.failure}>
            <span className="inline-flex items-center gap-xxs text-caption text-warning" data-backlog-read-failed="true">
              <TriangleAlertIcon aria-hidden="true" className="size-(--size-icon-sm)" />
              {t("backlog.readFailed", { reason: source.failure })}
              <button type="button" className={cn(LINK, "text-warning underline")} onClick={() => actions.openSettings("github")}>{t("backlog.readFix")}</button>
            </span>
          </Hint>
        ) : null}
        <span className="flex-1" />
        {backlog.started > 0 ? (
          <button type="button" className={cn(LINK, "text-subtle-foreground")} data-backlog-started={backlog.started} onClick={() => { useUiStore.getState().setMainView("tasks"); useUiStore.getState().setOverview({ projectId: project.id, panel: null }); }}>
            {t("backlog.started", { count: backlog.started })}
          </button>
        ) : null}
      </h2>
      <BacklogInput project={project} actions={actions} />
      <ul className="flex flex-col" role="list">
        {pending.map((item) => <PendingRow key={item.intent} item={item} project={project} actions={actions} />)}
        {neverRead ? null : shown.map((row) => <ItemRow key={row.key} row={row} project={project} />)}
      </ul>
      {!neverRead && backlog.rows.length === 0 && pending.length === 0 ? <p className="px-sm py-xs text-caption text-muted-foreground">{t("backlog.empty")}</p> : null}
      {!neverRead && filtering && rows.length === 0 && backlog.rows.length > 0 ? <p className="px-sm py-xs text-caption text-muted-foreground">{t("backlog.noMatch")}</p> : null}
      {hidden > 0 ? (
        <button type="button" className={cn(LINK, "mt-xxs text-muted-foreground")} data-backlog-more={project.id} onClick={() => useUiStore.getState().setOverview({ open: toggledFold(useUiStore.getState().overview.open, `more:${project.id}`) })}>
          {t(local ? "backlog.moreMemos" : "backlog.moreIssues", { count: hidden })}
          <ChevronRightIcon aria-hidden="true" className="size-(--size-icon-sm)" />
        </button>
      ) : more && rows.length > BACKLOG_SHOWN && !filtering ? (
        <button type="button" className={cn(LINK, "mt-xxs text-muted-foreground")} onClick={() => useUiStore.getState().setOverview({ open: toggledFold(useUiStore.getState().overview.open, `more:${project.id}`) })}>
          {t("overviewTasks.fold")}
        </button>
      ) : null}
    </section>
  );
}

/**
 * The section's input (B26, B29): Enter makes the item and empties the field,
 * keeping the focus for the next one; the hint always says where it goes.
 */
function BacklogInput({ project, actions }: { project: Workspace; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const [title, setTitle] = useState("");
  const destination = destinationOf(project.tasks?.source ?? null);
  const left = titleLeft(title);
  const submit = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== "Enter" || event.nativeEvent.isComposing) return;
    event.preventDefault();
    const text = title.trim();
    if (!text) return;
    actions.addBacklog(project.id, text);
    setTitle("");
  };
  return (
    <div className="flex items-center gap-sm pb-xs">
      <Input
        value={title}
        placeholder={t(destination === "local" ? "backlog.placeholderLocal" : "backlog.placeholderGithub", { project: project.label })}
        aria-label={t(destination === "local" ? "backlog.placeholderLocal" : "backlog.placeholderGithub", { project: project.label })}
        className="min-w-0 flex-1"
        onChange={(event) => setTitle(backlogTitle(event.target.value))}
        onKeyDown={submit}
        data-backlog-input={project.id}
      />
      {title ? <span className="shrink-0 font-mono text-micro text-muted-foreground" data-backlog-left={left}>{t("backlog.remaining", { count: left })}</span> : null}
      <span className="shrink-0 text-micro text-muted-foreground" data-backlog-destination={destination}>
        {t(destination === "local" ? "backlog.enterLocal" : destination === "public" ? "backlog.enterGithubPublic" : destination === "unknown" ? "backlog.enterGithubUnknown" : "backlog.enterGithub")}
      </span>
    </div>
  );
}

/** An item the core is making, made but not listed yet, or failed to make (B29, B30). */
function PendingRow({ item, project, actions }: { item: BacklogItem; project: Workspace; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const token = item.task_key ? itemToken(item.task_key, project) : null;
  return (
    <li className={cn("flex min-w-0 items-center gap-sm rounded-sm px-sm py-xs text-caption", item.phase !== "failed" && "opacity-(--opacity-dimmed)")} data-backlog-pending={item.phase} data-backlog-intent={item.intent}>
      <span className="min-w-(--size-control-lg) shrink-0 font-mono text-muted-foreground">{token ?? ""}</span>
      <span className="min-w-0 flex-1 truncate text-foreground">{item.title}</span>
      {item.phase === "failed" ? (
        <>
          <span className="shrink-0 text-destructive" data-backlog-failure={item.failure ?? "refused"}>{t(`backlog.failure.${item.failure ?? "refused"}`)}</span>
          {item.retry ? <Button size="sm" variant="ghost" onClick={() => actions.retryBacklog(item.intent)} data-backlog-retry={item.intent}>{t("common.retry")}</Button> : null}
          <Button size="sm" variant="ghost" onClick={() => actions.dismissBacklog(item.intent)} data-backlog-dismiss={item.intent}>{t("backlog.remove")}</Button>
        </>
      ) : item.phase === "created" ? null : (
        <span className="flex shrink-0 items-center gap-xxs text-muted-foreground"><LoaderCircleIcon aria-hidden="true" className="size-(--size-icon-sm) animate-spin" />{t("backlog.creating")}</span>
      )}
    </li>
  );
}

function ItemRow({ row, project }: { row: BacklogRow; project: Workspace }) {
  const { t, i18n } = useInterfaceTranslation();
  const selected = useUiStore((s) => s.overview.panel?.kind === "item" && s.overview.panel.taskKey === row.key && s.overview.panel.workspaceId === project.id);
  const fromMissed = useShellStore((s) => s.rest?.issue_work?.missed_keys?.includes(row.key) ?? false);
  const language = requireInterfaceLanguage(i18n.language);
  const open = () => useUiStore.getState().setOverview({ panel: { kind: "item", workspaceId: project.id, taskKey: row.key } });
  return (
    <li className={cn("relative flex min-w-0 items-center gap-sm rounded-sm px-sm py-xs text-caption hover:bg-accent", selected && "bg-secondary")} data-backlog-row={row.key}>
      <button type="button" aria-label={[row.token, row.title].filter(Boolean).join(" ")} aria-pressed={selected} className="absolute inset-0 rounded-sm outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring" onClick={open} />
      <span className="pointer-events-none min-w-(--size-control-lg) shrink-0 font-mono text-muted-foreground">{row.token ?? ""}</span>
      <span className="pointer-events-none min-w-0 flex-1 truncate text-foreground">{row.title}</span>
      {fromMissed ? <Hint label={t("backlog.fromMissed")}><FlagIcon aria-label={t("backlog.fromMissed")} className="relative size-(--size-icon-sm) shrink-0 text-warning" data-backlog-missed="true" /></Hint> : null}
      {row.labels.slice(0, 3).map((label) => <span key={label.name} className="pointer-events-none inline-flex shrink-0 overview-narrow:hidden"><IssueLabelView label={label} /></span>)}
      {row.at !== null ? <span className="pointer-events-none shrink-0 font-mono text-micro text-muted-foreground" title={issueDate(row.at, language)}>{ageWords(language, Date.now() - row.at, t)}</span> : null}
      {row.url && !row.local ? (
        <Hint label={t("backlog.openOnGithub")}>
          <a href={row.url} target="_blank" rel="noreferrer" aria-label={t("backlog.openOnGithub")} className={cn(LINK, "relative z-1 text-muted-foreground")} data-backlog-github={row.key}>
            <ExternalLinkIcon aria-hidden="true" className="size-(--size-icon-sm)" />
          </a>
        </Hint>
      ) : null}
      <Button size="sm" variant="ghost" className="relative z-1" onClick={() => useUiStore.getState().setWorkspaceDialog({ kind: "start_issue", workspaceId: project.id, taskKey: row.key })} data-backlog-start={row.key}>
        <PlayIcon aria-hidden="true" />
        {t("backlog.start")}
      </Button>
    </li>
  );
}

/** The backlog's label chips (B27): any one picked label keeps a row. */
export function BacklogLabelFilter({ projects }: { projects: readonly BoardProject[] }) {
  const { t } = useInterfaceTranslation();
  const picked = useUiStore((s) => s.overview.backlogLabels);
  const labels = useMemo(() => backlogLabels(projects), [projects]);
  if (labels.length === 0) return null;
  return (
    <Popover>
      <Hint label={t("backlog.labels")}>
        <PopoverTrigger asChild>
          <Button variant={picked.length ? "secondary" : "ghost"} size="icon-sm" aria-label={t("backlog.labels")} data-backlog-labels={picked.length ? "active" : "none"}>
            <ListFilterIcon aria-hidden="true" />
          </Button>
        </PopoverTrigger>
      </Hint>
      <PopoverContent align="end" className="flex max-h-(--radix-popover-content-available-height) flex-col gap-sm overflow-y-auto">
        <ToggleGroup type="multiple" value={[...picked]} onValueChange={(names) => useUiStore.getState().setOverview({ backlogLabels: names })} aria-label={t("backlog.labels")} className="w-full flex-wrap">
          {labels.map((label) => (
            <ToggleGroupItem key={label.name} value={label.name} size="sm" className="min-w-0 max-w-full" data-backlog-label={label.name}>
              <IssueLabelDot color={label.color} />
              <span className="min-w-0 truncate">{label.name}</span>
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      </PopoverContent>
    </Popover>
  );
}

/**
 * One backlog item beside the tab (B31, B32): its title, id, source, when it
 * was made, its labels, where it came from, Start, the GitHub link, and
 * `되돌리기` while no checkout works on it.
 */
export function BacklogItemPanel({ panel, projects, actions }: { panel: Extract<OverviewPanel, { kind: "item" }>; projects: readonly BoardProject[]; actions: Actions }) {
  const { t, i18n } = useInterfaceTranslation();
  const project = projects.find(({ workspace }) => workspace.id === panel.workspaceId)?.workspace ?? null;
  const task = project?.tasks?.tasks.find((row) => row.key === panel.taskKey) ?? null;
  const origin = useShellStore((s) => (s.rest?.issue_work?.origin?.task_key === panel.taskKey ? s.rest.issue_work.origin : null));
  const undo = useShellStore((s) => (s.rest?.issue_work?.undo?.task_key === panel.taskKey ? s.rest.issue_work.undo : null));
  const madeByHide = useShellStore((s) => s.rest?.issue_work?.made_keys?.some((key) => key.toLowerCase() === panel.taskKey.toLowerCase()) ?? false);
  const language = requireInterfaceLanguage(i18n.language);
  useBacklogOrigin(actions, panel.workspaceId, panel.taskKey);
  // An undone item has left the backlog; so does the panel.
  useEffect(() => {
    if (undo?.phase === "done") useUiStore.getState().setOverview({ panel: null });
  }, [undo?.phase]);
  const close = () => useUiStore.getState().setOverview({ panel: null });
  const head = (
    <div className="flex shrink-0 items-center px-sm py-xs">
      <Hint label={t("record.close")}><Button variant="ghost" size="icon-sm" aria-label={t("record.close")} onClick={close} data-item-close="true"><XIcon /></Button></Hint>
    </div>
  );
  if (!project || !task || !task.open) {
    return <div className="flex min-h-0 flex-1 flex-col" data-backlog-item="missing">{head}<p role="status" className="px-md py-sm text-caption text-muted-foreground">{t("backlog.panel.missing")}</p></div>;
  }
  const started = startedKeys(project).has(task.key);
  const local = isMemoKey(task.key);
  // Only an item Hide made can be undone: every memo, and a GitHub issue
  // the core's create record names (D-21); someone else's issue never is.
  const undoable = !started && (local || madeByHide);
  const at = task.created_at_unix_ms ?? task.updated_at_unix_ms ?? null;
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-backlog-item={task.key}>
      {head}
      <div className="min-h-0 flex-1 overflow-y-auto px-md pb-md text-caption [word-break:keep-all]">
        <h2 className="whitespace-normal pb-xs text-subhead font-semibold [overflow-wrap:anywhere]">{task.title}</h2>
        <p className="flex flex-wrap items-center gap-x-sm gap-y-xxs pb-sm text-micro text-muted-foreground">
          {task.id ? <span className="font-mono">{task.id}</span> : null}
          <span>{t("backlog.panel.source")} · {project.tasks?.source?.label ?? ""}</span>
          {at !== null ? <span>{t("backlog.panel.created")} · {issueDate(at, language)}</span> : null}
        </p>
        {task.labels?.length ? <p className="flex flex-wrap gap-sm pb-sm">{task.labels.map((label) => <IssueLabelView key={label.name} label={label} />)}</p> : null}
        <div className="flex flex-wrap items-center gap-xs pb-sm">
          <Button size="sm" onClick={() => useUiStore.getState().setWorkspaceDialog({ kind: "start_issue", workspaceId: project.id, taskKey: task.key })} data-item-start="true"><PlayIcon />{t("backlog.start")}</Button>
          {task.url && !local ? <Button size="sm" variant="outline" asChild><a href={task.url} target="_blank" rel="noreferrer" data-item-github="true"><ExternalLinkIcon />{t("backlog.openOnGithub")}</a></Button> : null}
          {undoable ? (
            <Button size="sm" variant="ghost" disabled={undo?.phase === "working"} onClick={() => actions.undoBacklog(project.id, task.key)} data-item-undo="true">
              {t(undo?.phase === "working" ? "backlog.panel.undoing" : "backlog.panel.undo")}
            </Button>
          ) : null}
        </div>
        {undo?.phase === "failed" ? (
          <p role="status" className="flex items-center gap-xs pb-sm text-destructive" data-item-undo-failed={undo.failure ?? "refused"}>
            {/* A refusal sent nothing anywhere and a retry would be refused again. */}
            {!undo.failure || undo.failure === "refused" ? <span className="min-w-0 flex-1">{t("backlog.panel.undoRefused")}</span> : <>
              <span className="min-w-0 flex-1">{t("backlog.panel.undoFailed", { reason: t(`backlog.failure.${undo.failure}`) })}</span>
              <Button size="sm" variant="ghost" onClick={() => actions.undoBacklog(project.id, task.key)}>{t("common.retry")}</Button>
            </>}
          </p>
        ) : null}
        {origin && (origin.phase !== "ready" || origin.session) ? (
          <section className="pt-sm" data-item-origin={origin.phase}>
            <h3 className="mb-xs font-medium text-subtle-foreground">{t("backlog.panel.origin")}</h3>
            {origin.phase === "reading" ? <p className="text-muted-foreground">{t("backlog.panel.originReading")}</p>
              : origin.phase === "failed" ? <p className="text-muted-foreground">{t("backlog.panel.originFailed")}</p>
              : origin.session ? (
                <button type="button" className="flex w-full min-w-0 flex-col items-start gap-xxs rounded-sm px-xs py-xs text-left outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring" onClick={() => { const session = origin.session!; actions.closeOverview(); actions.openSessionRecord(session); }} data-item-origin-session="true">
                  <span className="font-medium text-foreground">{origin.session_title ?? ""}</span>
                  {origin.task_name ? <span className="text-subtle-foreground">{origin.task_name}</span> : null}
                  {origin.missed_text ? <span className="flex items-start gap-xxs text-warning"><FlagIcon aria-hidden="true" className="mt-xxs size-(--size-icon-sm) shrink-0" />{origin.missed_text}</span> : null}
                </button>
              ) : null}
          </section>
        ) : null}
      </div>
    </div>
  );
}
