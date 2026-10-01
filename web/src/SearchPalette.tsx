import { CircleDotIcon, FolderIcon, GitBranchIcon, GitPullRequestIcon, LoaderCircleIcon, SearchIcon, ServerIcon, TriangleAlertIcon, ChevronRightIcon } from "lucide-react";
import type { ReactNode } from "react";
import { useEffect, useLayoutEffect, useMemo, useState } from "react";
import type { Actions } from "./actions";
import { AgentMark } from "./AgentMark";
import { DeviceChip } from "./components/device-chip";
import { Command, CommandDialog, CommandGroup, CommandInput, CommandItem, CommandList } from "./components/ui/command";
import { Kbd } from "./components/ui/kbd";
import { frontTarget, relationRows, relationsOf, type Relations } from "./relations";
import { filterEntries, githubEntries, groupEntries, GITHUB_GROUP, RELATED_GROUP, searchEntries, type SearchEntry, type SearchSection, type Tone } from "./search";
import { detailOf, type Detail } from "./searchDetail";
import { githubRow, hasGithubProject, ownAnswer, projectRead, projectToRead, startsSearch, type GithubRow, type ProjectRead } from "./searchGithub";
import { frontCheckout } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { remoteRequestId } from "./remote";
import { keyboardOwner } from "./viewFocus";

// ⌘K (PRD cmdk-navigation): a palette that goes to things. A query finds an
// agent, project, checkout, device, issue or pull request by name or #number;
// an empty one lists what is connected to the thing in front, drawn as a
// Project's Overview groups it; the highlighted row's detail is the right
// half. Nothing here runs a command but `에이전트 시작…`. GitHub is read only
// by the one project read ⌘K asks for when it opens and by the explicit
// `GitHub에서 "…" 검색` row; typing never calls it.

/** The window is too narrow for a list and a detail below this width; the detail is not drawn then (B26). */
const DETAIL_MIN_WIDTH = 640;

/** Whether a dialog of `width` px has room for the detail beside its list; unmeasured is yes. */
function showsDetail(width: number | null): boolean {
  return width === null || width >= DETAIL_MIN_WIDTH;
}

/** The projects ⌘K has asked to be read since the app started: each is asked for once (B10). */
const askedProjects = new Set<string>();

const GITHUB_ROW_ID = "github-search";

const TONE_CLASS: Record<Tone, string> = {
  working: "text-agent-working",
  attention: "text-warning",
  done: "text-success",
  open: "text-pr-open",
  pending: "text-warning",
  failed: "text-destructive",
  muted: "text-muted-foreground",
  merged: "text-pr-merged",
  closed: "text-pr-closed",
};

export function SearchPalette({ actions }: { actions: Actions }) {
  const rest = useShellStore((s) => s.rest);
  const close = useUiStore((s) => s.closeOverlay);
  const [query, setQuery] = useState("");
  const [highlighted, setHighlighted] = useState("");
  const [asked, setAsked] = useState<{ requestId: string; query: string } | null>(null);
  const [width, setWidth] = useState<number | null>(null);
  const [box, setBox] = useState<HTMLDivElement | null>(null);
  // What was in front when ⌘K opened, read once: the page's own screen, whether the keyboard was in an agent, and the front checkout.
  const [front] = useState(() => {
    const ui = useUiStore.getState();
    const state = useShellStore.getState();
    const owner = keyboardOwner().kind;
    const checkout = frontCheckout(state.rest);
    return {
      target: frontTarget(state.rest, ui.screen?.kind ?? null, ui.searchOver === "settings", state.focusedPaneId, owner === "pane" || owner === "agent", checkout?.id ?? null),
      checkout: ui.screen?.kind === "workspace" && ui.searchOver !== "settings" ? checkout : null,
    };
  });

  // The one read of the project in front, when nothing has read it (B8, B10).
  useEffect(() => {
    const workspace = projectToRead(useShellStore.getState().rest, front.checkout, askedProjects);
    if (!workspace) return;
    askedProjects.add(workspace.id);
    actions.readProjectTasks(workspace.id);
  }, [actions, front]);

  // The dialog's content mounts after this component does, so the box is found when it appears, not on mount.
  useLayoutEffect(() => {
    if (!box) return undefined;
    setWidth(box.getBoundingClientRect().width);
    if (typeof ResizeObserver === "undefined") return undefined;
    const observer = new ResizeObserver(([entry]) => setWidth(entry ? entry.contentRect.width : null));
    observer.observe(box);
    return () => observer.disconnect();
  }, [box]);

  const entries = useMemo(() => searchEntries(rest), [rest]);
  const related = useMemo<Relations | null>(() => (front.target ? relationsOf(rest, front.target, true) : null), [rest, front]);
  const trimmed = query.trim();
  const answer = ownAnswer(rest?.issue_work?.search, asked, trimmed);
  const row = githubRow(trimmed, answer);
  const showGithub = trimmed.length > 0 && hasGithubProject(rest);

  const sections: SearchSection[] = useMemo(() => {
    if (!trimmed) return [];
    const found = groupEntries(filterEntries(entries, trimmed));
    const searched = answer?.phase === "ready" ? githubEntries(answer.results, entries) : [];
    return searched.length > 0 ? [...found, { group: GITHUB_GROUP, entries: searched }] : found;
  }, [entries, trimmed, answer]);

  const relatedRows = useMemo(() => (trimmed || !related ? [] : relationRows(related)), [trimmed, related]);
  const listed: SearchEntry[] = trimmed ? sections.flatMap((section) => section.entries) : relatedRows;
  const ids = [...listed.map((entry) => entry.id), ...(showGithub ? [GITHUB_ROW_ID] : [])];
  // A surviving highlight stays; a retired one moves to the first row (B25).
  const selected = ids.includes(highlighted) ? highlighted : (ids[0] ?? "");
  const selectedEntry = listed.find((entry) => entry.id === selected) ?? null;

  const collapsed = !trimmed && relatedRows.length === 0;
  const detailVisible = !collapsed && showsDetail(width);
  const now = Date.now();

  const activate = (entry: SearchEntry) => {
    // The agent in front is where the operator already is; the detail says so (B21).
    if (entry.tag === "here") return;
    close();
    switch (entry.kind) {
      case "command":
        return actions.openStartPanel();
      case "device":
        return entry.deviceId ? actions.focusDevice(entry.deviceId) : undefined;
      case "agent":
        return entry.paneId ? actions.openAgent(entry.paneId) : undefined;
      case "project":
        return entry.deviceId && entry.workspaceId ? actions.openOverview(entry.deviceId, entry.workspaceId) : undefined;
      case "checkout":
        return entry.deviceId && entry.workspaceId && entry.checkoutId ? actions.openWorkspace(entry.deviceId, entry.workspaceId, entry.checkoutId) : undefined;
      case "issue":
      case "pr":
        if (entry.external && entry.url) return actions.openLink(entry.url, true);
        if (!entry.deviceId || !entry.workspaceId) return undefined;
        if (entry.kind === "issue" && entry.taskKey) return actions.openOverview(entry.deviceId, entry.workspaceId, { issue: entry.taskKey });
        if (entry.kind === "pr" && entry.number !== undefined) return actions.openOverview(entry.deviceId, entry.workspaceId, { pullRequest: entry.number });
        return undefined;
    }
  };

  const search = () => {
    // The same query already running is not started again (B19).
    if (!startsSearch(row)) return;
    const requestId = remoteRequestId();
    setAsked({ requestId, query: trimmed });
    actions.searchGithub(requestId, trimmed);
  };

  const onSelect = (id: string) => {
    if (id === GITHUB_ROW_ID) return search();
    const entry = listed.find((candidate) => candidate.id === id);
    if (entry) activate(entry);
  };

  const reads = (entry: SearchEntry): ProjectRead => (entry.kind === "checkout" && entry.workspace ? projectRead(entry.workspace, askedProjects, now) : null);

  const detail: Detail | null = selectedEntry ? detailOf(rest, selectedEntry, now) : null;

  return (
    <CommandDialog
      open
      title="Search"
      description="Go to an agent, project, checkout, device, issue or pull request"
      className="w-[calc(var(--size-search-sheet-w)*1.5)]"
      onOpenChange={(open) => {
        if (!open) close();
      }}
    >
      <div ref={setBox} className="flex min-w-0 flex-col" data-cmdk={collapsed ? "collapsed" : "open"}>
        <Command shouldFilter={false} label="Search" data-palette="Search" value={selected} onValueChange={setHighlighted}>
          <CommandInput
            value={query}
            placeholder="이름이나 #번호를 입력하세요"
            data-palette-input="true"
            onValueChange={(value) => setQuery(value)}
            wrapperClassName={collapsed ? "border-b-0" : undefined}
            trailing={<Kbd data-palette-esc="true">Esc</Kbd>}
          />
          {collapsed ? null : (
            <div className={`flex min-h-0 ${detailVisible ? "h-(--size-search-sheet-h)" : ""}`}>
              <CommandList className={`${detailVisible ? "max-h-[none] w-[calc(var(--size-search-sheet-w)-var(--spacing-xxs))] shrink-0 border-r border-border" : "min-w-0 flex-1"}`} data-palette-list="true">
                {trimmed ? (
                  <>
                    {sections.length === 0 ? (
                      <div className="px-md py-sm text-caption text-muted-foreground" data-palette-state="no-match">
                        일치하는 항목 없음
                      </div>
                    ) : null}
                    {sections.map((section) => (
                      <CommandGroup key={section.group.id} heading={section.group.label} data-palette-group={section.group.id}>
                        {section.entries.map((entry) => (
                          <SearchRow key={entry.id} entry={entry} read={reads(entry)} onSelect={() => onSelect(entry.id)} />
                        ))}
                      </CommandGroup>
                    ))}
                    {showGithub ? <GithubSearchRow row={row} onSelect={() => onSelect(GITHUB_ROW_ID)} /> : null}
                  </>
                ) : (
                  <CommandGroup heading={RELATED_GROUP.label} data-palette-group={RELATED_GROUP.id}>
                    {relatedRows.map((entry) => (
                      <SearchRow key={entry.id} entry={entry} read={reads(entry)} onSelect={() => onSelect(entry.id)} />
                    ))}
                  </CommandGroup>
                )}
              </CommandList>
              {detailVisible ? <DetailPane detail={detail} entry={selectedEntry} githubRow={selected === GITHUB_ROW_ID ? row : null} query={trimmed} /> : null}
            </div>
          )}
          {collapsed ? null : (
            <div className="flex items-center gap-md border-t border-border px-md py-xxs text-caption text-muted-foreground" data-palette-footer="true">
              <span className="flex items-center gap-xxs"><Kbd>↑</Kbd><Kbd>↓</Kbd> 이동</span>
              <span className="flex items-center gap-xxs"><Kbd>↵</Kbd> 열기</span>
              <span className="flex items-center gap-xxs"><Kbd>Esc</Kbd> 닫기</span>
            </div>
          )}
        </Command>
      </div>
    </CommandDialog>
  );
}

function EntryIcon({ entry }: { entry: SearchEntry }) {
  if (entry.kind === "agent") return <AgentMark kind={entry.agentKind} />;
  const Icon =
    entry.kind === "project" ? FolderIcon : entry.kind === "checkout" ? GitBranchIcon : entry.kind === "device" ? ServerIcon : entry.kind === "issue" ? CircleDotIcon : entry.kind === "pr" ? GitPullRequestIcon : ChevronRightIcon;
  const tone = entry.kind === "issue" || entry.kind === "pr" ? TONE_CLASS[entry.status?.tone ?? "muted"] : "";
  return (
    <span className={`flex w-(--size-agent-badge-compact) shrink-0 justify-center ${tone}`} aria-hidden="true">
      <Icon />
    </span>
  );
}

function StatusText({ status }: { status: { tone: Tone; label: string } }) {
  return <span className={`shrink-0 whitespace-nowrap text-caption ${TONE_CLASS[status.tone]}`}>{status.label}</span>;
}

/** One result or relation row: its mark, the title and the line under it, and the state at its end. */
function SearchRow({ entry, read, onSelect }: { entry: SearchEntry; read: ProjectRead; onSelect: () => void }) {
  const depth = entry.depth ?? 0;
  return (
    <CommandItem asChild value={entry.id} onSelect={onSelect}>
      <button type="button" data-palette-row={entry.id} data-palette-depth={depth} className="group/palette-row w-full text-left" style={depth > 0 ? { paddingLeft: `calc(var(--spacing-sm) + ${depth} * var(--spacing-lg))` } : undefined}>
        <EntryIcon entry={entry} />
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="flex min-w-0 items-center gap-xs">
            {entry.tag === "parent" ? <span className="shrink-0 text-caption text-muted-foreground">↑ 부모</span> : null}
            <span className="min-w-0 truncate font-medium">{entry.title}</span>
            {entry.chip ? <DeviceChip label={entry.chip.label} local={entry.chip.local} className="max-w-2/5" /> : null}
          </span>
          {entry.subtitle ? <span data-palette-detail="true" className="truncate text-caption text-muted-foreground">{entry.subtitle}</span> : null}
        </span>
        {entry.tag === "here" ? (
          <span className="shrink-0 rounded-full border border-border px-xs text-caption text-subtle-foreground" data-palette-here="true">
            여기
          </span>
        ) : null}
        {read?.state === "reading" ? <LoaderCircleIcon aria-label="GitHub 읽는 중" className="size-(--size-icon-sm) shrink-0 animate-spin text-muted-foreground" data-palette-read="reading" /> : null}
        {read?.state === "failed" ? (
          <span title={read.tooltip} aria-label={read.tooltip} className="shrink-0 text-warning" data-palette-read="failed">
            <TriangleAlertIcon className="size-(--size-icon-sm)" />
          </span>
        ) : null}
        {entry.ci ? <StatusText status={entry.ci} /> : entry.status ? <StatusText status={entry.status} /> : null}
        <span aria-hidden="true" data-palette-enter="true" className="invisible shrink-0 text-caption text-muted-foreground group-data-[selected=true]/palette-row:visible">
          ↵
        </span>
      </button>
    </CommandItem>
  );
}

/** The last row of a search: the explicit GitHub search, never run while typing (B16-B19). */
function GithubSearchRow({ row, onSelect }: { row: GithubRow; onSelect: () => void }) {
  return (
    <CommandGroup data-palette-group="github-search">
      <CommandItem asChild value={GITHUB_ROW_ID} onSelect={onSelect}>
        <button type="button" data-palette-row={GITHUB_ROW_ID} data-github-state={row.state} className="group/palette-row w-full text-left">
          <span className="flex w-(--size-agent-badge-compact) shrink-0 justify-center" aria-hidden="true">
            {row.state === "failed" ? <TriangleAlertIcon className="text-warning" /> : <SearchIcon />}
          </span>
          <span className={`min-w-0 flex-1 truncate ${row.state === "none" ? "text-muted-foreground" : "font-medium"}`}>{row.label}</span>
          {row.state === "working" ? <LoaderCircleIcon aria-label="GitHub 검색 중" className="size-(--size-icon-sm) shrink-0 animate-spin text-muted-foreground" data-palette-read="searching" /> : null}
          <span aria-hidden="true" data-palette-enter="true" className="invisible shrink-0 text-caption text-muted-foreground group-data-[selected=true]/palette-row:visible">
            ↵
          </span>
        </button>
      </CommandItem>
    </CommandGroup>
  );
}

function RelationBlock({ relations, on }: { relations: Relations; on: string }) {
  const line = (entry: SearchEntry, depth: number) => (
    <div key={entry.id} data-relation-row={entry.id} data-relation-on={entry.id === on ? "true" : undefined} className={`flex items-center gap-xs py-xxs text-caption ${entry.id === on ? "font-semibold text-foreground" : "text-subtle-foreground"}`} style={depth > 0 ? { paddingLeft: `calc(${depth} * var(--spacing-md))` } : undefined}>
      <span className="flex w-(--size-icon-sm) shrink-0 justify-center [&_svg]:size-(--size-icon-sm)"><EntryIcon entry={entry} /></span>
      <span className="min-w-0 truncate">{entry.tag === "parent" ? "↑ 부모 " : ""}{entry.title}</span>
    </div>
  );
  return (
    <div className="mt-md border-t border-border pt-md" data-detail-relations="true">
      <h4 className="mb-xs text-caption font-semibold text-muted-foreground">관계</h4>
      {relations.issues.map((entry) => line(entry, 0))}
      {relations.groups.map((group) => (
        <div key={group.head.id}>
          {line(group.head, 0)}
          {group.rows.map((entry) => line(entry, 1 + (entry.depth ?? 0)))}
        </div>
      ))}
    </div>
  );
}

/** The highlighted row, said in full: its state, its facts and the relations it belongs to. */
function DetailPane({ detail, entry, githubRow: row, query }: { detail: Detail | null; entry: SearchEntry | null; githubRow: GithubRow | null; query: string }) {
  if (row) {
    return (
      <aside className="flex min-w-0 flex-1 flex-col overflow-y-auto p-md" data-palette-detail-pane="github">
        <div className="text-caption font-semibold text-muted-foreground">GitHub</div>
        <div className="mt-xs break-words text-subhead font-semibold">{`"${query}"`}</div>
        <p className="mt-sm text-caption text-muted-foreground">이 Mac의 GitHub 프로젝트 저장소에서 PR과 이슈를 한 번 검색합니다. 입력하는 동안에는 GitHub를 부르지 않습니다.</p>
        <ActionLine text={row.state === "failed" ? "다시 시도" : row.state === "none" ? "찾은 결과가 없습니다" : "GitHub에서 검색"} key_={row.state !== "none"} />
      </aside>
    );
  }
  if (!detail || !entry) return <aside className="min-w-0 flex-1" data-palette-detail-pane="empty" />;
  const here = entry.tag === "here";
  return (
    <aside className="flex min-w-0 flex-1 flex-col" data-palette-detail-pane={entry.kind}>
      <div className="min-h-0 flex-1 overflow-y-auto p-md">
        <div className="text-caption font-semibold text-muted-foreground">{detail.kind}</div>
        <div className="mt-xs break-words text-subhead font-semibold">{detail.title}</div>
        {detail.pills.length + detail.tags.length > 0 ? (
          <div className="mt-sm flex flex-wrap gap-xs">
            {detail.pills.map((pill) => (
              <span key={pill.label} className={`rounded-sm bg-secondary px-sm text-caption ${TONE_CLASS[pill.tone]}`}>{pill.label}</span>
            ))}
            {detail.tags.map((tag) => (
              <span key={tag} className="rounded-sm bg-secondary px-sm text-caption text-subtle-foreground">{tag}</span>
            ))}
          </div>
        ) : null}
        {detail.facts.length > 0 ? (
          <dl className="mt-md grid grid-cols-[auto_minmax(0,1fr)] gap-x-md gap-y-xs border-t border-border pt-md text-caption" data-detail-facts="true">
            {detail.facts.map(([label, value]) => (
              <div key={label} className="contents">
                <dt className="text-muted-foreground">{label}</dt>
                <dd className="min-w-0 break-words">{value}</dd>
              </div>
            ))}
          </dl>
        ) : null}
        {detail.relations ? <RelationBlock relations={detail.relations} on={entry.id} /> : null}
      </div>
      <ActionLine text={here ? "지금 보고 있는 에이전트" : detail.action} key_={!here} />
    </aside>
  );
}

function ActionLine({ text, key_ }: { text: ReactNode; key_: boolean }) {
  return (
    <div className="mt-auto flex items-center gap-xs border-t border-border px-md py-sm text-caption text-muted-foreground" data-palette-action="true">
      {key_ ? <Kbd>↵</Kbd> : null}
      <span>{text}</span>
    </div>
  );
}
