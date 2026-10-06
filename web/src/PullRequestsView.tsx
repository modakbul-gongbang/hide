import {
  ChevronDownIcon,
  ChevronRightIcon,
  EllipsisIcon,
  ExternalLinkIcon,
  PlayIcon,
} from "lucide-react";
import { useEffect, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import type { Actions } from "./actions";
import { lineTone, markTone, rowLine } from "./agentRow";
import { CHECKOUT_KIND_ICON } from "./components/checkout-icon";
import { CheckoutCardHint } from "./components/pr-card";
import { StatusMark } from "./components/status-mark";
import { Button } from "./components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "./components/ui/dropdown-menu";
import { Hint } from "./components/ui/tooltip";
import { useInterfaceTranslation } from "./i18n/client";
import { cn } from "./lib/utils";
import { AgentMessagePopover, type LensHandlers } from "./OverviewLenses";
import { PrPanel } from "./PrPanel";
import { PR_GROUP_LABEL, type PrBoard, type PrRow } from "./projectBoard";
import { checkoutCard, pullRequestCard, pullRequestKind, relativeActivity, shownPullRequest } from "./projects";
import type { Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { ChecksMark, PR_TONE } from "./TaskBoards";
import { useUiStore, type PrLens } from "./ui";
import { holdsCommandKey } from "./host";

// The PRs tab of a Project's Overview (PRD overview-lenses-prs, link-graph
// D-08): the project's pull requests grouped by whose move it is, the
// operator's first, one line each: its state, its title, who is on it, CI and
// the time; hovering puts its buttons in the fixed time slot, so nothing
// moves; a half-second rest on a part opens that part's card. A row opens its
// pull request's panel beside the list, which keeps the width left (B2). The
// open panel and the focus are the screen's own state and publish nothing; a
// GitHub write happens only after its dialog's one confirmation
// (`PrDialogs.tsx`).

const MARKS = 3;

/** The row the board draws for `number`, folded groups included. */
function boardRow(board: PrBoard, number: number | null): PrRow | null {
  if (number === null) return null;
  for (const { rows } of board.groups) {
    const row = rows.find((entry) => entry.number === number);
    if (row) return row;
  }
  return null;
}

export function PullRequestsView({ board, project, lens, onLens, handlers, actions, now }: { board: PrBoard; project: Workspace; lens: PrLens; onLens: (prs: Partial<PrLens>) => void; handlers: LensHandlers; actions: Actions; now: number }) {
  const { t } = useInterfaceTranslation();
  const root = useRef<HTMLDivElement>(null);
  const [linking, setLinking] = useState(false);
  const merged = board.groups.find((entry) => entry.group === "merged");
  // A merged row asked for by a chip or open in the panel unfolds its group too (B21).
  const asked = lens.panel ?? lens.focus;
  const mergedOpen = lens.merged || (asked !== null && (merged?.rows.some((row) => row.number === asked) ?? false));
  const panelRow = boardRow(board, lens.panel);
  const record = useShellStore((s) => s.linkPanel);
  // A pull request the board does not list opens from the record (a session's
  // chip to an old one); one the record has not either closes again.
  const matching = record?.workspace_id === project.id && record.target.kind === "pr" && record.target.number === lens.panel ? record : null;
  const unknown = lens.panel !== null && panelRow === null && !board.reading && matching !== null && !matching.loading && matching.pr === null;
  useEffect(() => {
    if (unknown) onLens({ panel: null });
  }, [unknown, onLens]);
  const panelOpen = lens.panel !== null && !unknown;
  const open = (number: number) => {
    setLinking(false);
    onLens({ panel: number, focus: number });
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
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      const rows = [...(root.current?.querySelectorAll<HTMLElement>("[data-pr-row]") ?? [])];
      const next = rows[rows.indexOf(from) + (event.key === "ArrowDown" ? 1 : -1)];
      if (!next) return;
      event.preventDefault();
      next.focus();
      next.scrollIntoView({ block: "nearest" });
      // The panel follows the row the arrows land on (B2).
      if (lens.panel !== null) open(Number(next.dataset.prRow));
    }
  };
  const close = () => {
    const number = lens.panel;
    setLinking(false);
    onLens({ panel: null });
    // The keyboard goes back to the row the panel was for.
    if (number !== null) root.current?.querySelector<HTMLElement>(`[data-pr-row="${number}"]`)?.focus();
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
  if (board.groups.length === 0 && !panelOpen) {
    return (
      <p className="px-lg pb-xl text-caption text-muted-foreground" data-prs-view="empty">
        {t("prList.empty")}
      </p>
    );
  }
  // One tree whether or not the panel is open, so the list keeps its scroll and focus.
  return (
    <div className={panelOpen ? "flex min-h-0 flex-1 items-stretch gap-md pb-lg pr-lg" : "contents"} data-prs-split={panelOpen ? "true" : undefined}>
      <div ref={root} className={cn("flex flex-col gap-md px-lg pb-xl", panelOpen && "min-h-0 min-w-0 flex-1 overflow-auto pr-none")} data-prs-view="board" onKeyDown={keys}>
        {board.groups.map(({ group, rows }) => {
          const label = t(PR_GROUP_LABEL[group]);
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
                    <PullRequestRowView
                      key={row.number}
                      row={row}
                      project={project}
                      selected={panelOpen && lens.panel === row.number}
                      onOpen={() => open(row.number)}
                      onLink={() => {
                        open(row.number);
                        setLinking(true);
                      }}
                      handlers={handlers}
                      now={now}
                    />
                  ))}
                </ul>
              )}
            </section>
          );
        })}
      </div>
      {panelOpen && lens.panel !== null ? (
        <PrPanel key={lens.panel} number={lens.panel} row={panelRow} board={board} project={project} handlers={handlers} actions={actions} now={now} linking={linking} onLinking={setLinking} onClose={close} />
      ) : null}
    </div>
  );
}

/** Stops a part's click from also opening the row. */
function own(handler: (event: MouseEvent) => void) {
  return (event: MouseEvent) => {
    event.stopPropagation();
    handler(event);
  };
}

/**
 * One pull request on one line (link-graph B1): its state glyph, its title,
 * the agent marks, CI and the time. The row itself is one button that opens
 * its panel (⌘-click and ⌘↵: GitHub); every part above it is its own
 * destination.
 */
function PullRequestRowView({ row, project, selected, onOpen, onLink, handlers, now }: { row: PrRow; project: Workspace; selected: boolean; onOpen: () => void; onLink: () => void; handlers: LensHandlers; now: number }) {
  const github = (url: string) => handlers.openGitHub(url, project.device_id);
  const { t } = useInterfaceTranslation();
  const Glyph = CHECKOUT_KIND_ICON[pullRequestKind(row.pr)];
  const place = row.checkout?.branch ?? row.branch;
  const card = row.checkout && shownPullRequest(row.checkout)?.number === row.number ? checkoutCard(project, row.checkout, now, t) : pullRequestCard(row.pr, t);
  const age = relativeActivity(row.at, now, t);
  return (
    <li className="flex flex-col" data-pr={row.number} data-pr-group-row={row.group} data-selected={selected ? "true" : undefined}>
      <div className={cn("group/pr-row relative flex h-(--size-control-lg) min-w-0 items-center gap-sm rounded-sm pr-sm pl-xs", selected && "bg-secondary", row.group === "merged" && !selected && "opacity-(--opacity-secondary)")}>
        <button
          type="button"
          aria-label={`PR #${row.number} ${row.title}`}
          aria-current={selected ? "true" : undefined}
          className="absolute inset-0 rounded-sm outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring"
          onClick={(event) => {
            if (holdsCommandKey(event)) github(row.url);
            else onOpen();
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter" && holdsCommandKey(event)) {
              event.preventDefault();
              github(row.url);
            }
          }}
          data-pr-row={row.number}
        />
        <CheckoutCardHint card={card} description={`PR #${row.number} · ${row.title}`} onOpenPullRequest={(url) => github(url)}>
          <span className="relative inline-flex shrink-0" data-pr-glyph={row.number}>
            <Glyph aria-hidden="true" className={cn("size-(--size-pr-icon)", PR_TONE[row.tone])} data-pr-state={row.tone} />
          </span>
        </CheckoutCardHint>
        <Hint label={row.title} reveals>
          <span className={cn("relative min-w-0 cursor-pointer truncate text-subhead", row.group === "merged" ? "text-subtle-foreground" : "text-foreground")} onClick={(event) => (holdsCommandKey(event) ? github(row.url) : onOpen())} data-pr-title={row.number}>
            {row.title}
          </span>
        </Hint>
        <span className="flex-1" />
        <AgentMarks row={row} place={place} onOpen={onOpen} handlers={handlers} />
        <span className="relative flex w-(--size-icon-sm) shrink-0 justify-center">
          {row.checks ? (
            <Hint label={t(row.checks === "failed" ? "prList.openFailedChecks" : "prList.openChecks")}>
              <button type="button" className="inline-flex rounded-xs outline-none focus-visible:ring-1 focus-visible:ring-ring" onClick={own(() => github(`${row.url}/checks`))} data-pr-checks-open={row.checks}>
                <ChecksMark checks={row.checks} />
              </button>
            </Hint>
          ) : null}
        </span>
        <span className="relative flex w-(--size-pr-slot) shrink-0 items-center justify-end">
          <span className="font-mono text-caption text-muted-foreground group-focus-within/pr-row:invisible group-hover/pr-row:invisible group-has-data-[state=open]/pr-row:invisible" data-pr-age={row.number}>
            {age}
          </span>
          <RowActions row={row} project={project} onLink={onLink} handlers={handlers} />
        </span>
      </div>
    </li>
  );
}

/**
 * The agents on the branch's checkout and the one that made it, up to three marks and `+N` (B4): one
 * agent's mark opens its pane, several open the panel with their sessions; each mark's
 * half-second card is everything that agent last said (B7).
 */
function AgentMarks({ row, place, onOpen, handlers }: { row: PrRow; place: string; onOpen: () => void; handlers: LensHandlers }) {
  if (row.agents.length === 0) return null;
  const one = row.agents.length === 1;
  return (
    <span className="relative flex shrink-0 items-center gap-xxs" data-pr-agents={row.agents.length}>
      {row.agents.slice(0, MARKS).map((agent) => {
        const said = rowLine(agent);
        return (
          <AgentMessagePopover key={agent.pane_id} agent={agent} place={place} fallback={said?.text ?? agent.identity_label} tone={said ? lineTone(said, agent) : "text-muted-foreground"} onOpen={() => handlers.openAgent(agent.pane_id)}>
            <button type="button" aria-label={agent.identity_label} className="inline-flex rounded-xs outline-none focus-visible:ring-1 focus-visible:ring-ring" onClick={own(() => (one ? handlers.openAgent(agent.pane_id) : onOpen()))} data-pr-agent={agent.pane_id}>
              <StatusMark symbol={agent.symbol} className={markTone(agent)} />
            </button>
          </AgentMessagePopover>
        );
      })}
      {row.agents.length > MARKS ? <span className="font-mono text-caption text-muted-foreground">+{row.agents.length - MARKS}</span> : null}
    </span>
  );
}

/**
 * The buttons that stand in the time slot under the pointer or the keyboard
 * (B6): `▷ Assign` where a failing or change-requested pull request has no
 * agent, `Clean up` on a merged one whose worktree is still here, else the GitHub
 * icon and `⋯` with Assign, Link issue and Copy branch name.
 */
function RowActions({ row, project, onLink, handlers }: { row: PrRow; project: Workspace; onLink: () => void; handlers: LensHandlers }) {
  const { t } = useInterfaceTranslation();
  const reveal = "invisible absolute inset-y-0 right-0 flex items-center gap-xxs group-focus-within/pr-row:visible group-hover/pr-row:visible has-data-[state=open]:visible";
  const delegate = () => useUiStore.getState().setWorkspaceDialog({ kind: "pr_delegate", workspaceId: project.id, prNumber: row.number });
  if (row.delegate) {
    return (
      <span className={reveal} data-pr-actions="delegate">
        <Hint label={t("prList.takeHint")}>
          <Button variant="secondary" size="sm" onClick={own(delegate)} data-pr-delegate-open={row.number}>
            <PlayIcon aria-hidden="true" />
            {t("prList.delegate")}
          </Button>
        </Hint>
      </span>
    );
  }
  if (row.cleanup && row.checkout) {
    const checkout = row.checkout;
    return (
      <span className={reveal} data-pr-actions="cleanup">
        <Hint label={t("prList.cleanHint")}>
          <Button variant="ghost" size="sm" onClick={own(() => handlers.cleanup(project, checkout))} data-pr-cleanup={row.cleanup}>
            {t("prList.cleanup")}
          </Button>
        </Hint>
      </span>
    );
  }
  const open = row.group !== "merged";
  return (
    <span className={reveal} data-pr-actions="default">
      <Hint label={t("prList.gitHubHint")}>
        <Button variant="ghost" size="icon-sm" aria-label="GitHub" onClick={own(() => handlers.openGitHub(row.url, project.device_id))} data-pr-github={row.number}>
          <ExternalLinkIcon aria-hidden="true" />
        </Button>
      </Hint>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="icon-sm" aria-label={t("prList.actions")} onClick={(event) => event.stopPropagation()} data-pr-menu={row.number}>
            <EllipsisIcon aria-hidden="true" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" onClick={(event) => event.stopPropagation()}>
          {open ? <DropdownMenuItem onSelect={delegate} data-pr-menu-delegate="true">{t("prList.delegate")}</DropdownMenuItem> : null}
          {row.linkable ? <DropdownMenuItem onSelect={onLink} data-pr-menu-link="true">{t("prList.linkIssue")}</DropdownMenuItem> : null}
          <DropdownMenuItem onSelect={() => void navigator.clipboard?.writeText(row.branch)} data-pr-menu-copy="true">
            {t("prList.copyBranch")}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </span>
  );
}
