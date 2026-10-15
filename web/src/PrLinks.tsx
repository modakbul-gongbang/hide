import { CircleDashedIcon, CircleDotIcon, FolderGit2Icon, Link2Icon } from "lucide-react";
import { useRef, type ReactNode } from "react";
import { Button } from "./components/ui/button";
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList, CommandSeparator } from "./components/ui/command";
import { Popover, PopoverAnchor, PopoverContent } from "./components/ui/popover";
import { Hint } from "./components/ui/tooltip";
import { useInterfaceTranslation } from "./i18n/client";
import type { MessageKey } from "./i18n/catalogs";
import { cn } from "./lib/utils";
import { folderName, sameIssue } from "./linkPanel";
import type { Checkout, LinkIssueSource, LinkPanel, Task, Workspace } from "./snapshot";
import { TaskGlyph } from "./issueMarks";
import { useUiStore } from "./ui";
import type { Actions } from "./actions";

/** Where the parts of a pull request's links go. */
export type LinkHandlers = {
  /** A checkout: its Workspace. */
  openCheckout: (project: Workspace, checkout: Checkout) => void;
  /** An issue of the project's source: its card or backlog item (PRD overview-device-tasks B60). */
  openIssue: (project: Workspace, task: Task) => void;
  /** A link out: the page on GitHub. */
  openGitHub: (url: string, deviceId: string) => void;
};

export function linkHandlers(actions: Actions): LinkHandlers {
  return {
    openCheckout: (project, checkout) => actions.openWorkspace(project.device_id, checkout.workspace_id, checkout.id),
    openIssue: (project, task) => actions.openBacklogItem(project.device_id, project.id, task.key),
    openGitHub: (url, deviceId) => actions.openPullRequest(url, deviceId, true),
  };
}

// What a pull request is linked to (PRD link-graph B4, B5; work-tab-pr-lens
// D-06): its issues with where each link came from, the worktrees of its
// branch, and the Link issue picker the card and a list row's menu open. What
// is true now (the board's issue, the checkout) wins over the record.

const SOURCE_WORD: Record<LinkIssueSource, MessageKey> = { closes: "links.source.closes", hide: "links.source.hide" };

/** A GitHub task key's web address, for an issue the project's list no longer holds. */
function issueUrl(key: string): string | null {
  const match = /^github:([\w.-]+\/[\w.-]+)#(\d+)$/i.exec(key);
  return match ? `https://github.com/${match[1]}/issues/${match[2]}` : null;
}

function issueLabel(key: string): string {
  return `#${key.split("#").pop() ?? key}`;
}

/**
 * The links section (B4, B5): each issue with where the link came from, then
 * the worktrees of its branch, a gone one dim with `Cleaned up`; with no issue,
 * `No linked issue` and Link issue.
 */
export function PrLinks({ number, issue: boardIssue, checkout: boardCheckout, linkable, record, project, handlers, linking, onLinking }: { number: number; issue: { key: string } | null; checkout: Checkout | null; linkable: boolean; record: LinkPanel | null; project: Workspace; handlers: LinkHandlers; linking: boolean; onLinking: (open: boolean) => void }) {
  const { t } = useInterfaceTranslation();
  const tasks = project.tasks?.tasks ?? [];
  const issues: { key: string; source: LinkIssueSource | null }[] = [...(record?.pr?.issues ?? [])];
  // The board's issue is GitHub's or the branch's link now; the record may not have it yet (B5).
  if (boardIssue && !issues.some((issue) => sameIssue(issue.key, boardIssue.key))) issues.push({ key: boardIssue.key, source: null });
  const paths = [...(record?.pr?.worktrees ?? [])];
  if (boardCheckout && !paths.includes(boardCheckout.path)) paths.unshift(boardCheckout.path);
  const checkouts = project.checkouts;
  return (
    <section className="flex flex-col gap-xs" aria-label={t("links.connections")} data-pr-panel-links="true">
      <h3 className="text-caption text-muted-foreground">{t("links.connections")}</h3>
      {issues.length === 0 ? (
        <span className="flex min-h-(--size-control) items-center gap-sm text-body text-muted-foreground" data-pr-panel-issue="none">
          <CircleDashedIcon aria-hidden="true" className="size-(--size-icon) shrink-0" />
          <span className="flex-1">{t("links.noIssue")}</span>
          {linkable ? (
            <IssuePicker number={number} project={project} open={linking} onOpenChange={onLinking}>
              <Button variant="secondary" size="sm" onClick={() => onLinking(true)} data-pr-link-open={number}>
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
function IssueLine({ issueKey, source, task, project, handlers }: { issueKey: string; source: LinkIssueSource | null; task: Task | null; project: Workspace; handlers: LinkHandlers }) {
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
export function IssuePicker({ number, project, open, onOpenChange, children }: { number: number; project: Workspace; open: boolean; onOpenChange: (open: boolean) => void; children: ReactNode }) {
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
        data-pr-link-picker={number}
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
                <CommandItem key={task.key} value={`${task.id ?? ""} ${task.title} ${task.key}`} onSelect={() => choose({ kind: "pr_link", workspaceId: project.id, prNumber: number, issueKey: task.key })} data-pr-link-choice={task.key}>
                  <TaskGlyph task={task} />
                  <span className="shrink-0 font-mono text-caption text-muted-foreground">{task.id}</span>
                  <span className="min-w-0 truncate">{task.title}</span>
                </CommandItem>
              ))}
            </CommandGroup>
            <CommandSeparator />
            <CommandGroup>
              <CommandItem value={t("prList.newIssue")} onSelect={() => choose({ kind: "pr_new_issue", workspaceId: project.id, prNumber: number })} data-pr-link-new="true">
                {t("prList.newIssue")}
              </CommandItem>
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}
