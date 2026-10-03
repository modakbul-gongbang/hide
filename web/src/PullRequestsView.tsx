import {
  ChevronDownIcon,
  ChevronRightIcon,
  CircleDashedIcon,
  EllipsisIcon,
  ExternalLinkIcon,
  Link2Icon,
  PlayIcon,
  SquareTerminalIcon,
} from "lucide-react";
import { useEffect, useRef, useState, type KeyboardEvent, type MouseEvent, type ReactNode } from "react";
import { lineTone, markTone, rowLine } from "./agentRow";
import { CHECKOUT_KIND_ICON } from "./components/checkout-icon";
import { CheckoutCardHint } from "./components/pr-card";
import { StatusMark } from "./components/status-mark";
import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList, CommandSeparator } from "./components/ui/command";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "./components/ui/dropdown-menu";
import { Popover, PopoverAnchor, PopoverContent } from "./components/ui/popover";
import { Hint } from "./components/ui/tooltip";
import { cn } from "./lib/utils";
import { AgentMessagePopover, IssueChip, type LensHandlers } from "./OverviewLenses";
import type { PrBoard, PrRow } from "./projectBoard";
import { checkoutCard, laneCheckoutCard, pullRequestCard, pullRequestKind, relativeActivity, shownPullRequest } from "./projects";
import type { Workspace } from "./snapshot";
import { CardAgentRow, ChecksMark, PR_TONE, REVIEW, TaskGlyph } from "./TaskBoards";
import { useUiStore, type PrLens } from "./ui";
import { holdsCommandKey, opensExternally } from "./host";

// The PRs tab of a Project's Overview (PRD overview-lenses-prs): the project's
// pull requests grouped by whose move it is, the operator's first. At rest a
// row says what the pull request is, which issue it works on, who is on it and
// where GitHub stands; hovering puts its buttons in the fixed time slot, so
// nothing moves; a half-second rest on a part opens that part's card. Rows
// unfold to the agents on the branch with their ancestors. Hover, focus and
// unfolding are the screen's own state and publish nothing (B24); a GitHub
// write happens only after its dialog's one confirmation (`PrDialogs.tsx`).

const TAKE_HINT = "에이전트에게 맡기기\nPR 브랜치에서 시작 대화상자를 연다. 첫 지시 = 실패한 검사 · 리뷰 코멘트";
const LINK_HINT = "이슈 잇기\n이 PR을 이슈에 잇는다. 이을 이슈가 없으면 PR 제목 · 본문으로 새로 만든다";
const CLEAN_HINT = "정리\n머지됐거나 폴더가 없는 워크트리와 거기서 쉬는 에이전트를 지운다. 확인 대화상자가 먼저 뜬다";
const GITHUB_HINT = "GitHub\n리뷰하고 머지";
const MARKS = 3;

function toggled(values: readonly number[], value: number): number[] {
  return values.includes(value) ? values.filter((row) => row !== value) : [...values, value];
}

export function PullRequestsView({ board, project, lens, onLens, handlers, now }: { board: PrBoard; project: Workspace; lens: PrLens; onLens: (prs: Partial<PrLens>) => void; handlers: LensHandlers; now: number }) {
  const root = useRef<HTMLDivElement>(null);
  const merged = board.groups.find((entry) => entry.group === "merged");
  // A merged row asked for by a chip unfolds its group too (B21).
  const mergedOpen = lens.merged || (lens.focus !== null && (merged?.rows.some((row) => row.number === lens.focus) ?? false));
  const setOpen = (number: number, open: boolean) => {
    if (lens.open.includes(number) !== open) onLens({ open: toggled(lens.open, number), focus: number });
  };
  // A chip's row is brought into view and given the keyboard once drawn.
  const drawn = board.groups.length > 0;
  useEffect(() => {
    if (lens.focus === null || !drawn) return;
    const row = root.current?.querySelector<HTMLElement>(`[data-pr-row="${lens.focus}"]`);
    row?.scrollIntoView({ block: "nearest" });
    row?.focus({ preventScroll: true });
  }, [lens.focus, drawn]);
  const keys = (event: KeyboardEvent<HTMLDivElement>) => {
    const from = (event.target as HTMLElement).closest<HTMLElement>("[data-pr-row]");
    if (!from || event.target !== from) return;
    const number = Number(from.dataset.prRow);
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      const rows = [...(root.current?.querySelectorAll<HTMLElement>("[data-pr-row]") ?? [])];
      const next = rows[rows.indexOf(from) + (event.key === "ArrowDown" ? 1 : -1)];
      if (!next) return;
      event.preventDefault();
      next.focus();
      next.scrollIntoView({ block: "nearest" });
    } else if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
      event.preventDefault();
      setOpen(number, event.key === "ArrowRight");
    }
  };
  if (board.reading && board.groups.length === 0) {
    return (
      <div className="flex flex-col gap-xs px-lg pb-xl" data-prs-view="reading">
        {[0, 1, 2].map((index) => (
          <span key={index} className="block h-(--size-control) rounded-sm bg-muted" data-pr-skeleton={index} />
        ))}
      </div>
    );
  }
  if (board.groups.length === 0) {
    return (
      <p className="px-lg pb-xl text-caption text-muted-foreground" data-prs-view="empty">
        열린 PR이 없습니다.
      </p>
    );
  }
  return (
    <div ref={root} className="flex flex-col gap-md px-lg pb-xl" data-prs-view="board" onKeyDown={keys}>
      {board.groups.map(({ group, label, rows }) => {
        const folded = group === "merged" && !mergedOpen;
        return (
          <section key={group} className="flex flex-col" data-pr-group={group} data-folded={folded ? "true" : undefined}>
            <h2 className="flex h-(--size-control) items-center gap-xs text-subhead font-semibold">
              {group === "merged" ? (
                <button type="button" aria-expanded={!folded} onClick={() => onLens({ merged: folded, focus: null })} className="inline-flex items-center gap-xs rounded-xs text-foreground outline-none focus-visible:ring-1 focus-visible:ring-ring" data-pr-group-toggle="merged">
                  {folded ? <ChevronRightIcon aria-hidden="true" className="size-(--size-icon) text-muted-foreground" /> : <ChevronDownIcon aria-hidden="true" className="size-(--size-icon) text-muted-foreground" />}
                  {label}
                  <span className="font-normal text-muted-foreground">{rows.length}</span>
                </button>
              ) : (
                <span className={cn("inline-flex items-center gap-xs", group === "turn" ? "text-warning" : "text-foreground")}>
                  {label}
                  <span className="font-normal text-muted-foreground">{rows.length}</span>
                </span>
              )}
            </h2>
            {folded ? null : (
              <ul className="flex flex-col" role="list">
                {rows.map((row) => (
                  <PullRequestRowView key={row.number} row={row} project={project} open={lens.open.includes(row.number)} onToggle={() => setOpen(row.number, !lens.open.includes(row.number))} onUnfold={() => setOpen(row.number, true)} handlers={handlers} now={now} />
                ))}
              </ul>
            )}
          </section>
        );
      })}
    </div>
  );
}


/** Stops a part's click from also unfolding the row. */
function own(handler: (event: MouseEvent) => void) {
  return (event: MouseEvent) => {
    event.stopPropagation();
    handler(event);
  };
}

/**
 * One pull request (B3-B8): `▸`, its state glyph, number and title, the issue
 * cell, the yellow `확인` when an agent there finished unseen, then the agent
 * marks, the branch, CI, the review word and the time. The row itself is one
 * button that unfolds it (⌘-click: GitHub); every part above it is its own
 * destination.
 */
function PullRequestRowView({ row, project, open, onToggle, onUnfold, handlers, now }: { row: PrRow; project: Workspace; open: boolean; onToggle: () => void; onUnfold: () => void; handlers: LensHandlers; now: number }) {
  const [picking, setPicking] = useState(false);
  const github = (url: string) => handlers.openGitHub(url, project.device_id);
  const Glyph = CHECKOUT_KIND_ICON[pullRequestKind(row.pr)];
  const place = row.checkout?.branch ?? row.branch;
  const checkoutHere = row.checkout !== null && row.checkout.exists;
  const numberCard = row.checkout && shownPullRequest(row.checkout)?.number === row.number ? checkoutCard(project, row.checkout, now) : pullRequestCard(row.pr);
  const age = relativeActivity(row.at, now);
  return (
    <li className="flex flex-col" data-pr={row.number} data-pr-group-row={row.group} data-open={open ? "true" : undefined}>
      <div className={cn("group/pr-row relative flex h-(--size-control-lg) min-w-0 items-center gap-sm rounded-sm pr-sm pl-xs", open && "bg-secondary", row.group === "merged" && "opacity-(--opacity-secondary)")}>
        <button
          type="button"
          aria-expanded={open}
          aria-label={`PR #${row.number} ${row.title}`}
          className="absolute inset-0 rounded-sm outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring"
          onClick={(event) => {
            if (opensExternally(event)) github(row.url);
            else onToggle();
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter" && holdsCommandKey(event)) {
              event.preventDefault();
              github(row.url);
            }
          }}
          data-pr-row={row.number}
        />
        <span className="pointer-events-none relative flex w-(--size-icon-sm) shrink-0 justify-center text-muted-foreground" aria-hidden="true">
          {open ? <ChevronDownIcon className="size-(--size-icon-sm)" /> : <ChevronRightIcon className="size-(--size-icon-sm)" />}
        </span>
        <Glyph aria-hidden="true" className={cn("pointer-events-none relative size-(--size-pr-icon) shrink-0", PR_TONE[row.tone])} data-pr-state={row.tone} />
        <CheckoutCardHint card={numberCard} description={`PR #${row.number} · ${row.title}`} onOpenPullRequest={(url) => github(url)}>
          <span className="relative w-(--size-pr-number) shrink-0 font-mono text-caption text-muted-foreground" data-pr-number={row.number}>
            #{row.number}
          </span>
        </CheckoutCardHint>
        <span className={cn("pointer-events-none relative min-w-0 truncate text-subhead", row.group === "merged" ? "text-subtle-foreground" : "text-foreground")}>{row.title}</span>
        <IssueCell row={row} project={project} picking={picking} onPicking={setPicking} handlers={handlers} now={now} />
        {row.needsLook ? (
          <Badge variant="outline" className="pointer-events-none relative shrink-0 border-warning text-warning" data-pr-look="true">
            확인
          </Badge>
        ) : null}
        <span className="flex-1" />
        <AgentMarks row={row} place={place} onUnfold={onUnfold} handlers={handlers} />
        <BranchCell row={row} project={project} checkoutHere={checkoutHere} handlers={handlers} now={now} />
        <span className="relative flex w-(--size-icon-sm) shrink-0 justify-center">
          {row.checks ? (
            <Hint label={`${row.checks === "failed" ? "실패한 검사" : "검사"} GitHub에서 보기`}>
              <button type="button" className="inline-flex rounded-xs outline-none focus-visible:ring-1 focus-visible:ring-ring" onClick={own(() => github(`${row.url}/checks`))} data-pr-checks-open={row.checks}>
                <ChecksMark checks={row.checks} />
              </button>
            </Hint>
          ) : null}
        </span>
        <span className="relative flex w-(--size-pr-review) shrink-0 justify-end">
          {row.review ? (
            <button type="button" className={cn("rounded-xs text-caption outline-none hover:underline focus-visible:ring-1 focus-visible:ring-ring", REVIEW[row.review].tone)} onClick={own(() => github(`${row.url}/files`))} data-pr-review={row.review}>
              {REVIEW[row.review].label}
            </button>
          ) : null}
        </span>
        <span className="relative flex w-(--size-pr-slot) shrink-0 items-center justify-end">
          <span className="font-mono text-caption text-muted-foreground group-focus-within/pr-row:invisible group-hover/pr-row:invisible group-has-data-[state=open]/pr-row:invisible" data-pr-age={row.number}>
            {age}
          </span>
          <RowActions row={row} project={project} onLink={() => setPicking(true)} handlers={handlers} />
        </span>
      </div>
      {open ? <UnfoldedRow row={row} project={project} place={place} checkoutHere={checkoutHere} onLink={() => setPicking(true)} handlers={handlers} /> : null}
    </li>
  );
}

/**
 * The issue cell (B7-B9): the issue's chip, its card after a half-second
 * rest and its panel on a click; empty, a dotted circle that turns into the
 * 이슈 잇기 icon under the pointer where the pull request can be linked, and
 * opens the project's open issues to choose from, searchable, with `새 이슈
 * 만들기` last.
 */
function IssueCell({ row, project, picking, onPicking, handlers, now }: { row: PrRow; project: Workspace; picking: boolean; onPicking: (open: boolean) => void; handlers: LensHandlers; now: number }) {
  const issue = row.issue;
  if (issue?.task) {
    return (
      <span className="relative shrink-0" data-pr-issue={issue.key}>
        <IssueChip project={project} task={issue.task} handlers={handlers} now={now} />
      </span>
    );
  }
  if (issue) {
    // A reference the source does not list (closed, or another repository's): GitHub has it.
    return (
      <Hint label={`${issue.label} GitHub에서 열기`}>
        <button type="button" className="relative shrink-0 rounded-xs font-mono text-caption text-muted-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring" onClick={own(() => issue.url && handlers.openGitHub(issue.url, project.device_id))} data-pr-issue={issue.key}>
          {issue.label}
        </button>
      </Hint>
    );
  }
  const empty = <CircleDashedIcon aria-hidden="true" className="size-(--size-icon-sm) text-muted-foreground" />;
  if (!row.linkable) {
    return (
      <span className="pointer-events-none relative inline-flex w-(--size-icon) shrink-0 justify-center" data-pr-issue="none">
        {empty}
      </span>
    );
  }
  return (
    <IssuePicker row={row} project={project} open={picking} onOpenChange={onPicking}>
      <button type="button" aria-label="이슈 잇기" className="group/pr-issue relative inline-flex w-(--size-icon) shrink-0 justify-center rounded-xs outline-none focus-visible:ring-1 focus-visible:ring-ring" onClick={own(() => onPicking(true))} data-pr-issue="none" data-pr-link-open={row.number}>
        <span className={cn("group-hover/pr-issue:hidden group-focus-visible/pr-issue:hidden", picking && "hidden")}>{empty}</span>
        <Link2Icon aria-hidden="true" className={cn("hidden size-(--size-icon-sm) text-foreground group-hover/pr-issue:block group-focus-visible/pr-issue:block", picking && "block")} data-pr-link-icon={row.number} />
      </button>
    </IssuePicker>
  );
}

/** The project's open issues and `새 이슈 만들기` (B9) under the issue cell, `children`, which carries the 이슈 잇기 hint; choosing one opens its confirmation, or for a Local issue links it. */
function IssuePicker({ row, project, open, onOpenChange, children }: { row: PrRow; project: Workspace; open: boolean; onOpenChange: (open: boolean) => void; children: ReactNode }) {
  const issues = (project.tasks?.tasks ?? []).filter((task) => task.open);
  // A choice hands the keyboard to the dialog it opens, not back to the cell.
  const chosen = useRef(false);
  const choose = (dialog: Parameters<ReturnType<typeof useUiStore.getState>["setWorkspaceDialog"]>[0]) => {
    chosen.current = true;
    onOpenChange(false);
    useUiStore.getState().setWorkspaceDialog(dialog);
  };
  return (
    <Popover open={open} onOpenChange={onOpenChange}>
      <Hint label={LINK_HINT}>
        <PopoverAnchor asChild>{children}</PopoverAnchor>
      </Hint>
      <PopoverContent
        align="start"
        className="w-(--size-pr-popover) p-none"
        data-pr-link-picker={row.number}
        onClick={(event) => event.stopPropagation()}
        onOpenAutoFocus={() => {
          chosen.current = false;
        }}
        onCloseAutoFocus={(event) => {
          if (chosen.current) event.preventDefault();
        }}
      >
        <Command>
          <CommandInput placeholder="이슈 검색" autoFocus />
          <CommandList>
            <CommandEmpty>열린 이슈가 없습니다</CommandEmpty>
            <CommandGroup heading={project.tasks?.source?.label ?? "이슈"}>
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
              <CommandItem value="새 이슈 만들기" onSelect={() => choose({ kind: "pr_new_issue", workspaceId: project.id, prNumber: row.number })} data-pr-link-new="true">
                새 이슈 만들기
              </CommandItem>
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}

/**
 * The agents on the branch's checkout, up to three marks and `+N` (B4): one
 * agent's mark opens its pane, several unfold the row; each mark's
 * half-second card is everything that agent last said (B7).
 */
function AgentMarks({ row, place, onUnfold, handlers }: { row: PrRow; place: string; onUnfold: () => void; handlers: LensHandlers }) {
  if (row.agents.length === 0) return null;
  const one = row.agents.length === 1;
  return (
    <span className="relative flex shrink-0 items-center gap-xxs" data-pr-agents={row.agents.length}>
      {row.agents.slice(0, MARKS).map((agent) => {
        const said = rowLine(agent);
        return (
          <AgentMessagePopover key={agent.pane_id} agent={agent} place={place} fallback={said?.text ?? agent.identity_label} tone={said ? lineTone(said, agent.demand) : "text-muted-foreground"} onOpen={() => handlers.openAgent(agent.pane_id)}>
            <button type="button" aria-label={agent.identity_label} className="inline-flex rounded-xs outline-none focus-visible:ring-1 focus-visible:ring-ring" onClick={own(() => (one ? handlers.openAgent(agent.pane_id) : onUnfold()))} data-pr-agent={agent.pane_id}>
              <StatusMark symbol={agent.symbol} className={markTone(agent)} />
            </button>
          </AgentMessagePopover>
        );
      })}
      {row.agents.length > MARKS ? <span className="font-mono text-caption text-muted-foreground">+{row.agents.length - MARKS}</span> : null}
    </span>
  );
}

/** The branch in mono (B7, B8): its checkout's Workspace and checkout card where it has one here, else only its name. */
function BranchCell({ row, project, checkoutHere, handlers, now }: { row: PrRow; project: Workspace; checkoutHere: boolean; handlers: LensHandlers; now: number }) {
  const checkout = row.checkout;
  const text = <span className="truncate">{row.branch}</span>;
  const shape = "relative min-w-0 max-w-(--size-pr-branch-max) shrink truncate rounded-xs font-mono text-caption text-muted-foreground";
  if (!checkout || !checkoutHere) {
    return (
      <Hint label={row.branch}>
        <span className={shape} data-pr-branch={row.branch}>
          {text}
        </span>
      </Hint>
    );
  }
  return (
    <CheckoutCardHint card={laneCheckoutCard(project, checkout, now)} description={`${row.branch} · ${checkout.path}`} onOpenPullRequest={(url) => handlers.openGitHub(url, project.device_id)} onOpenWorkspace={() => handlers.openCheckout(project, checkout)}>
      <button type="button" className={cn(shape, "inline-flex outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring")} onClick={own(() => handlers.openCheckout(project, checkout))} data-pr-branch={row.branch}>
        {text}
      </button>
    </CheckoutCardHint>
  );
}

/**
 * The buttons that stand in the time slot under the pointer or the keyboard
 * (B6): `▷ 맡기기` where a failing or change-requested pull request has no
 * agent, `정리` on a merged one whose worktree is still here, else the GitHub
 * icon and `⋯` with 맡기기, 이슈 잇기 and 브랜치 이름 복사.
 */
function RowActions({ row, project, onLink, handlers }: { row: PrRow; project: Workspace; onLink: () => void; handlers: LensHandlers }) {
  const reveal = "invisible absolute inset-y-0 right-0 flex items-center gap-xxs group-focus-within/pr-row:visible group-hover/pr-row:visible has-data-[state=open]:visible";
  const delegate = () => useUiStore.getState().setWorkspaceDialog({ kind: "pr_delegate", workspaceId: project.id, prNumber: row.number });
  if (row.delegate) {
    return (
      <span className={reveal} data-pr-actions="delegate">
        <Hint label={TAKE_HINT}>
          <Button variant="secondary" size="sm" onClick={own(delegate)} data-pr-delegate-open={row.number}>
            <PlayIcon aria-hidden="true" />
            맡기기
          </Button>
        </Hint>
      </span>
    );
  }
  if (row.cleanup && row.checkout) {
    const checkout = row.checkout;
    return (
      <span className={reveal} data-pr-actions="cleanup">
        <Hint label={CLEAN_HINT}>
          <Button variant="ghost" size="sm" onClick={own(() => handlers.cleanup(project, checkout))} data-pr-cleanup={row.cleanup}>
            정리
          </Button>
        </Hint>
      </span>
    );
  }
  const open = row.group !== "merged";
  return (
    <span className={reveal} data-pr-actions="default">
      <Hint label={GITHUB_HINT}>
        <Button variant="ghost" size="icon-sm" aria-label="GitHub" onClick={own(() => handlers.openGitHub(row.url, project.device_id))} data-pr-github={row.number}>
          <ExternalLinkIcon aria-hidden="true" />
        </Button>
      </Hint>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="icon-sm" aria-label="PR 동작" onClick={(event) => event.stopPropagation()} data-pr-menu={row.number}>
            <EllipsisIcon aria-hidden="true" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" onClick={(event) => event.stopPropagation()}>
          {open ? <DropdownMenuItem onSelect={delegate} data-pr-menu-delegate="true">맡기기</DropdownMenuItem> : null}
          {row.linkable ? <DropdownMenuItem onSelect={onLink} data-pr-menu-link="true">이슈 잇기</DropdownMenuItem> : null}
          <DropdownMenuItem onSelect={() => void navigator.clipboard?.writeText(row.branch)} data-pr-menu-copy="true">
            브랜치 이름 복사
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </span>
  );
}

/**
 * The unfolded row (B5): the branch's agents under their ancestors, root
 * first, the operator's turn on its yellow line, then GitHub, Workspace and,
 * with no issue, 이슈 잇기 as icon buttons.
 */
function UnfoldedRow({ row, project, place, checkoutHere, onLink, handlers }: { row: PrRow; project: Workspace; place: string; checkoutHere: boolean; onLink: () => void; handlers: LensHandlers }) {
  const checkout = row.checkout;
  return (
    <div className="flex flex-col gap-xxs pb-sm pl-(--size-pr-indent)" data-pr-unfolded={row.number}>
      {row.lineage.length > 0 ? (
        <ul className="flex flex-col" role="list" data-pr-lineage={row.lineage.length}>
          {row.lineage.map(({ agent, depth }) => (
            <CardAgentRow key={agent.pane_id} agent={agent} depth={depth} place={place} selected={false} onOpen={handlers.openAgent} />
          ))}
        </ul>
      ) : null}
      <span className="flex items-center gap-xs">
        <Hint label={GITHUB_HINT}>
          <Button variant="ghost" size="icon-sm" aria-label="GitHub" onClick={() => handlers.openGitHub(row.url, project.device_id)} data-pr-unfolded-github={row.number}>
            <ExternalLinkIcon aria-hidden="true" />
          </Button>
        </Hint>
        {checkout && checkoutHere ? (
          <Hint label="Workspace 열기">
            <Button variant="ghost" size="icon-sm" aria-label="Workspace" onClick={() => handlers.openCheckout(project, checkout)} data-pr-unfolded-workspace={row.number}>
              <SquareTerminalIcon aria-hidden="true" />
            </Button>
          </Hint>
        ) : null}
        {row.linkable ? (
          <Hint label={LINK_HINT}>
            <Button variant="ghost" size="icon-sm" aria-label="이슈 잇기" onClick={onLink} data-pr-unfolded-link={row.number}>
              <Link2Icon aria-hidden="true" />
            </Button>
          </Hint>
        ) : null}
      </span>
    </div>
  );
}
