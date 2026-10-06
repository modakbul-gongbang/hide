import { CircleDashedIcon, CircleDotIcon, ExternalLinkIcon, FolderGit2Icon, GitBranchIcon, Link2Icon, PlayIcon, SquareTerminalIcon, TriangleAlertIcon, XIcon } from "lucide-react";
import { useEffect, useRef, type ReactNode } from "react";
import type { Actions } from "./actions";
import { CHECKOUT_KIND_ICON } from "./components/checkout-icon";
import { Button } from "./components/ui/button";
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList, CommandSeparator } from "./components/ui/command";
import { Kbd } from "./components/ui/kbd";
import { useEscapeLayer } from "./components/ui/layer";
import { Popover, PopoverAnchor, PopoverContent } from "./components/ui/popover";
import { Hint } from "./components/ui/tooltip";
import { useInterfaceTranslation } from "./i18n/client";
import type { MessageKey } from "./i18n/catalogs";
import { cn } from "./lib/utils";
import { folderName, sameIssue } from "./linkPanel";
import { LinkSessions } from "./LinkSessions";
import type { LensHandlers } from "./OverviewLenses";
import { issueDate, readFailureText, type PrBoard, type PrRow } from "./projectBoard";
import { pullRequestKind, relativeActivity, type PullRequestKind } from "./projects";
import type { LinkIssueSource, LinkPanel, Task, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { ChecksMark, PR_TONE, REVIEW, TaskGlyph } from "./TaskBoards";
import { useUiStore } from "./ui";

// The PR panel beside the PRs list (PRD link-graph D-08, B2-B5, B23-B26): the
// head, the title, the facts line, what the pull request is linked to and the
// sessions that made it or worked on it. What is true now (the PR, CI, the
// review, whether a worktree is still here, whether a pane is live) is drawn
// from the snapshot and wins over the record (B5); the links and the sessions
// are the record's, read when the panel opens and on its own revision.

const STATE_WORD: Record<PullRequestKind, { key: MessageKey; tone: string }> = {
  pr_open: { key: "links.state.open", tone: PR_TONE.open },
  pr_draft: { key: "links.state.draft", tone: PR_TONE.draft },
  pr_merged: { key: "links.state.merged", tone: PR_TONE.merged },
  pr_closed: { key: "links.state.closed", tone: PR_TONE.closed },
};

const SOURCE_WORD: Record<LinkIssueSource, MessageKey> = { closes: "links.source.closes", hide: "links.source.hide" };

/** The pull request as the panel draws it: the board's row when it lists the PR, else the record's copy. */
type Shown = {
  number: number;
  title: string;
  url: string;
  branch: string;
  kind: PullRequestKind;
  checks: PrRow["checks"];
  review: PrRow["review"];
  at: number | null;
  mergedAt: number | null;
  closedAt: number | null;
};

function shownPr(row: PrRow | null, record: LinkPanel["pr"]): Shown | null {
  if (row) {
    return { number: row.number, title: row.title, url: row.url, branch: row.branch, kind: pullRequestKind(row.pr), checks: row.checks, review: row.review, at: row.at, mergedAt: row.pr.merged_at_unix_ms ?? null, closedAt: record?.closed_at_unix_ms ?? null };
  }
  if (!record) return null;
  const kind: PullRequestKind = record.merged_at_unix_ms !== null ? "pr_merged" : record.closed_at_unix_ms !== null ? "pr_closed" : "pr_open";
  return { number: record.number, title: record.title, url: record.url, branch: record.branch, kind, checks: null, review: null, at: null, mergedAt: record.merged_at_unix_ms, closedAt: record.closed_at_unix_ms };
}

/** A GitHub task key's web address, for an issue the project's list no longer holds. */
function issueUrl(key: string): string | null {
  const match = /^github:([^/]+\/[^#]+)#(\d+)$/i.exec(key);
  return match ? `https://github.com/${match[1]}/issues/${match[2]}` : null;
}

function issueLabel(key: string): string {
  return `#${key.split("#").pop() ?? key}`;
}

export function PrPanel({
  number,
  row,
  board,
  project,
  handlers,
  actions,
  now,
  linking,
  onLinking,
  onClose,
}: {
  number: number;
  /** The board's row for the pull request; null when the board does not list it (an old merged one a chip asked for). */
  row: PrRow | null;
  board: PrBoard;
  project: Workspace;
  handlers: LensHandlers;
  actions: Actions;
  now: number;
  /** The Link issue picker is open (the row's `⋯` asked for it). */
  linking: boolean;
  onLinking: (open: boolean) => void;
  onClose: () => void;
}) {
  const { t, i18n } = useInterfaceTranslation();
  const held = useShellStore((s) => s.linkPanel);
  const record = held && held.workspace_id === project.id && held.target.kind === "pr" && held.target.number === number ? held : null;
  // One read each time the panel opens on a pull request; the core drops an answer for one no longer shown.
  useEffect(() => {
    actions.openLinks(project.id, { kind: "pr", number });
  }, [actions, project.id, number]);
  useEffect(() => () => actions.closeLinks(), [actions]);
  // Escape closes the panel first, then leaves the Overview (B2).
  useEscapeLayer(true, onClose);
  const pr = shownPr(row, record?.pr ?? null);
  if (!pr) return null;
  const Glyph = CHECKOUT_KIND_ICON[pr.kind];
  const state = STATE_WORD[pr.kind];
  const checkout = row?.checkout && row.checkout.exists ? row.checkout : null;
  const github = (url: string) => handlers.openGitHub(url, project.device_id);
  const when =
    pr.kind === "pr_merged" && pr.mergedAt !== null
      ? t("links.mergedOn", { date: issueDate(pr.mergedAt, i18n.language) })
      : pr.kind === "pr_closed" && pr.closedAt !== null
        ? t("links.closedOn", { date: issueDate(pr.closedAt, i18n.language) })
        : pr.at !== null
          ? t("links.updated", { age: relativeActivity(pr.at, now, t) ?? "" })
          : null;
  const delegate = row && pr.kind !== "pr_merged" && pr.kind !== "pr_closed" ? () => useUiStore.getState().setWorkspaceDialog({ kind: "pr_delegate", workspaceId: project.id, prNumber: pr.number }) : null;
  const panel: LinkPanel = record ?? { workspace_id: project.id, target: { kind: "pr", number }, loading: true, failure: null, pr: null, prs: [], sessions: [], total: 0 };
  return (
    <aside
      aria-label={t("links.panel", { number: pr.number })}
      className="flex min-h-0 w-2/5 min-w-(--issue-panel-min-width) shrink-0 flex-col gap-md overflow-y-auto rounded-md border border-border bg-card p-lg"
      data-pr-panel={pr.number}
      tabIndex={-1}
    >
      <header className="flex min-w-0 items-center gap-xs text-caption">
        <Glyph aria-hidden="true" className={cn("size-(--size-pr-icon) shrink-0", state.tone)} />
        <span className="font-mono text-muted-foreground">#{pr.number}</span>
        <span className={cn("font-medium", state.tone)} data-pr-panel-state={pr.kind}>
          {t(state.key)}
        </span>
        <span className="flex-1" />
        {checkout ? (
          <Hint label={t("issue.openWorkspace")}>
            <Button variant="ghost" size="icon-sm" onClick={() => handlers.openCheckout(project, checkout)} data-pr-panel-workspace={pr.number}>
              <SquareTerminalIcon aria-hidden="true" />
            </Button>
          </Hint>
        ) : null}
        <Hint label={t("prList.gitHubHint")} shortcut={<Kbd>⌘↵</Kbd>}>
          <Button variant="ghost" size="icon-sm" onClick={() => github(pr.url)} data-pr-panel-github={pr.number}>
            <ExternalLinkIcon aria-hidden="true" />
          </Button>
        </Hint>
        <Hint label={t("common.close")} shortcut={<Kbd>Esc</Kbd>}>
          <Button variant="ghost" size="icon-sm" onClick={onClose} data-pr-panel-close="true">
            <XIcon aria-hidden="true" />
          </Button>
        </Hint>
      </header>
      <Hint label={pr.title} reveals>
        <h2 className="line-clamp-2 break-words break-keep text-headline font-semibold text-foreground" data-pr-panel-title="true">
          {pr.title}
        </h2>
      </Hint>
      <p className="flex min-w-0 flex-wrap items-center gap-x-xs gap-y-xxs text-caption text-muted-foreground" data-pr-panel-facts="true">
        <span className="inline-flex min-w-0 max-w-full items-center gap-xxs">
          <GitBranchIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
          <Hint label={pr.branch} reveals>
            <span className="min-w-0 truncate font-mono" data-pr-panel-branch={pr.branch}>
              {pr.branch}
            </span>
          </Hint>
        </span>
        {pr.checks ? (
          <>
            <span aria-hidden="true">·</span>
            <button type="button" className="inline-flex rounded-xs outline-none focus-visible:ring-1 focus-visible:ring-ring" onClick={() => github(`${pr.url}/checks`)} data-pr-panel-checks={pr.checks}>
              <ChecksMark checks={pr.checks} />
            </button>
          </>
        ) : null}
        {pr.review ? (
          <button type="button" className={cn("rounded-xs outline-none hover:underline focus-visible:ring-1 focus-visible:ring-ring", REVIEW[pr.review].tone)} onClick={() => github(`${pr.url}/files`)} data-pr-panel-review={pr.review}>
            {t(REVIEW[pr.review].label)}
          </button>
        ) : null}
        {when ? (
          <>
            <span aria-hidden="true">·</span>
            <span data-pr-panel-when="true">{when}</span>
          </>
        ) : null}
        {board.failure ? (
          <Hint label={readFailureText(board.failure, t)}>
            <span role="img" className="inline-flex text-warning" data-pr-panel-stale="true">
              <TriangleAlertIcon aria-hidden="true" className="size-(--size-icon-sm)" />
            </span>
          </Hint>
        ) : null}
      </p>
      <hr className="border-border" />
      <Connections row={row} record={record} project={project} handlers={handlers} linking={linking} onLinking={onLinking} />
      <LinkSessions
        panel={panel}
        project={project}
        branchOf={() => pr.branch}
        onRetry={() => actions.openLinks(project.id, { kind: "pr", number })}
        empty={
          <p className="flex items-center gap-sm text-caption text-muted-foreground" data-link-none="true">
            {t("links.noSessions")}
            {delegate ? (
              <Hint label={t("prList.takeHint")}>
                <Button variant="secondary" size="sm" onClick={delegate} data-pr-panel-delegate={pr.number}>
                  <PlayIcon aria-hidden="true" />
                  {t("prList.delegate")}
                </Button>
              </Hint>
            ) : null}
          </p>
        }
        actions={actions}
      />
    </aside>
  );
}

/**
 * The links section (B4, B5): each issue with where the link came from, then
 * the worktrees of its branch, a gone one dim with `Cleaned up`; with no issue,
 * `No linked issue` and Link issue.
 */
function Connections({ row, record, project, handlers, linking, onLinking }: { row: PrRow | null; record: LinkPanel | null; project: Workspace; handlers: LensHandlers; linking: boolean; onLinking: (open: boolean) => void }) {
  const { t } = useInterfaceTranslation();
  const tasks = project.tasks?.tasks ?? [];
  const issues: { key: string; source: LinkIssueSource | null }[] = [...(record?.pr?.issues ?? [])];
  // The board's issue is GitHub's or the branch's link now; the record may not have it yet (B5).
  if (row?.issue && !issues.some((issue) => sameIssue(issue.key, row.issue!.key))) issues.push({ key: row.issue.key, source: null });
  const paths = [...(record?.pr?.worktrees ?? [])];
  if (row?.checkout && !paths.includes(row.checkout.path)) paths.unshift(row.checkout.path);
  const checkouts = project.checkouts;
  return (
    <section className="flex flex-col gap-xs" aria-label={t("links.connections")} data-pr-panel-links="true">
      <h3 className="text-caption text-muted-foreground">{t("links.connections")}</h3>
      {issues.length === 0 ? (
        <span className="flex min-h-(--size-control) items-center gap-sm text-body text-muted-foreground" data-pr-panel-issue="none">
          <CircleDashedIcon aria-hidden="true" className="size-(--size-icon) shrink-0" />
          <span className="flex-1">{t("links.noIssue")}</span>
          {row?.linkable ? (
            <IssuePicker row={row} project={project} open={linking} onOpenChange={onLinking}>
              <Button variant="secondary" size="sm" onClick={() => onLinking(true)} data-pr-link-open={row.number}>
                <Link2Icon aria-hidden="true" />
                {t("prList.linkIssue")}
              </Button>
            </IssuePicker>
          ) : null}
        </span>
      ) : (
        issues.map((issue) => {
          const task = tasks.find((candidate) => sameIssue(candidate.key, issue.key)) ?? null;
          return <IssueLine key={issue.key} issueKey={issue.key} source={issue.source} task={task} project={project} handlers={handlers} />;
        })
      )}
      {paths.map((path) => {
        const here = checkouts.find((checkout) => checkout.path === path && checkout.exists) ?? null;
        return (
          <span key={path} className="flex min-h-(--size-control) min-w-0 items-center gap-sm text-body" data-pr-panel-worktree={path} data-removed={here ? undefined : "true"}>
            <FolderGit2Icon aria-hidden="true" className={cn("size-(--size-icon) shrink-0", here ? "text-subtle-foreground" : "text-muted-foreground")} />
            {here ? (
              <Hint label={path}>
                <button type="button" className="min-w-0 truncate rounded-xs text-left text-foreground outline-none hover:underline focus-visible:ring-1 focus-visible:ring-ring" onClick={() => handlers.openCheckout(project, here)}>
                  {folderName(path)}
                </button>
              </Hint>
            ) : (
              <>
                <Hint label={path} reveals>
                  <span className="min-w-0 truncate text-muted-foreground">{folderName(path)}</span>
                </Hint>
                <span className="shrink-0 text-caption text-muted-foreground" data-pr-panel-cleaned="true">
                  {t("links.cleaned")}
                </span>
              </>
            )}
          </span>
        );
      })}
    </section>
  );
}

/** One linked issue (B4): its panel on the Issues tab when the project lists it, else GitHub. */
function IssueLine({ issueKey, source, task, project, handlers }: { issueKey: string; source: LinkIssueSource | null; task: Task | null; project: Workspace; handlers: LensHandlers }) {
  const { t } = useInterfaceTranslation();
  const label = task ? `${task.id ?? issueLabel(issueKey)} ${task.title}` : issueLabel(issueKey);
  const url = task?.url ?? issueUrl(issueKey);
  const open = task ? () => handlers.openIssue(project, task) : url ? () => handlers.openGitHub(url, project.device_id) : null;
  return (
    <span className="flex min-h-(--size-control) min-w-0 items-center gap-sm text-body" data-pr-panel-issue={issueKey} data-pr-panel-source={source ?? undefined}>
      {task ? <TaskGlyph task={task} className="size-(--size-icon) text-subtle-foreground" /> : <CircleDotIcon aria-hidden="true" className="size-(--size-icon) shrink-0 text-subtle-foreground" />}
      {open ? (
        <Hint label={task ? t("links.openIssue", { issue: label }) : t("prList.openIssue", { issue: label })} reveals>
          <button type="button" className="min-w-0 flex-1 truncate rounded-xs text-left text-foreground outline-none hover:underline focus-visible:ring-1 focus-visible:ring-ring" onClick={open}>
            {label}
          </button>
        </Hint>
      ) : (
        <span className="min-w-0 flex-1 truncate">{label}</span>
      )}
      {source ? <span className="shrink-0 text-caption text-muted-foreground">{t(SOURCE_WORD[source])}</span> : null}
    </span>
  );
}

/** The project's open issues and `Create new issue` (overview-lenses-prs B9) under Link issue, `children`; choosing one opens its confirmation, or for a Local issue links it. */
export function IssuePicker({ row, project, open, onOpenChange, children }: { row: PrRow; project: Workspace; open: boolean; onOpenChange: (open: boolean) => void; children: ReactNode }) {
  const { t } = useInterfaceTranslation();
  const issues = (project.tasks?.tasks ?? []).filter((task) => task.open);
  // A choice hands the keyboard to the dialog it opens, not back to the button.
  const chosen = useRef(false);
  const choose = (dialog: Parameters<ReturnType<typeof useUiStore.getState>["setWorkspaceDialog"]>[0]) => {
    chosen.current = true;
    onOpenChange(false);
    useUiStore.getState().setWorkspaceDialog(dialog);
  };
  return (
    <Popover open={open} onOpenChange={onOpenChange}>
      <Hint label={t("prList.linkHint")} reveals>
        <PopoverAnchor asChild>{children}</PopoverAnchor>
      </Hint>
      <PopoverContent
        align="end"
        className="w-(--size-pr-popover) p-none"
        data-pr-link-picker={row.number}
        onOpenAutoFocus={() => {
          chosen.current = false;
        }}
        onCloseAutoFocus={(event) => {
          if (chosen.current) event.preventDefault();
        }}
      >
        <Command>
          <CommandInput placeholder={t("prList.searchIssues")} autoFocus />
          <CommandList>
            <CommandEmpty>{t("board.empty")}</CommandEmpty>
            <CommandGroup heading={project.tasks?.source?.label ?? t("issue.label")}>
              {issues.map((task) => (
                <CommandItem key={task.key} value={`${task.id ?? ""} ${task.title} ${task.key}`} onSelect={() => choose({ kind: "pr_link", workspaceId: project.id, prNumber: row.number, issueKey: task.key })} data-pr-link-choice={task.key}>
                  <TaskGlyph task={task} />
                  <span className="shrink-0 font-mono text-caption text-muted-foreground">{task.id}</span>
                  <span className="min-w-0 truncate">{task.title}</span>
                </CommandItem>
              ))}
            </CommandGroup>
            <CommandSeparator />
            <CommandGroup>
              <CommandItem value={t("prList.newIssue")} onSelect={() => choose({ kind: "pr_new_issue", workspaceId: project.id, prNumber: row.number })} data-pr-link-new="true">
                {t("prList.newIssue")}
              </CommandItem>
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}
