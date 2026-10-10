import { CheckIcon, ChevronDownIcon, ChevronRightIcon } from "lucide-react";
import { memo, useEffect, useMemo, useState, type KeyboardEvent } from "react";
import type { Actions } from "./actions";
import { AgentMark } from "./AgentMark";
import { markTone } from "./agentRow";
import { sessionsModel, type SessionsModel } from "./sessionPanel";
import { AskLine, DescendantMark, TreeChevron, TreeRails, type TreePlace } from "./components/agent-tree";
import { AgentPrMark, useAgentStaleness } from "./components/pr-mark";
import { childrenOf } from "./components/agent-tree-popover";
import { Elapsed } from "./components/elapsed";
import { StatusMark } from "./components/status-mark";
import { Hint } from "./components/ui/tooltip";
import { statusText } from "./agentStatus";
import { useInterfaceTranslation } from "./i18n/client";
import type { MessageKey } from "./i18n/catalogs";
import { cn } from "./lib/utils";
import type { LensAgent } from "./overviewLens";
import { checkoutPathOfPane, childBranch, sessionTreeLines } from "./sessionTree";
import { sidebarFocusPane } from "./sidebarFocus";
import type { SidebarTreeLine } from "./sidebarTree";
import type { AgentRow, SessionGroup } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";

const GROUP_LABEL: Record<SessionGroup, MessageKey> = {
  needs_you: "agents.group.needs_you", working: "agents.group.working", done: "agents.group.done",
  idle: "agentSessions.group.idle", resolved: "agentSessions.group.resolved",
};
const NO_FOLDS: string[] = [];
const NO_PANES: ReadonlySet<string> = new Set();

/**
 * Sessions (PRD agent-hierarchy-screens D-16, D-17; B26 to B33): the core
 * groups and orders the roots; a row opens its children at any depth, kept
 * apart from the sidebar's folds, and the row standing for the focused pane
 * is selected as in the sidebar.
 */
export function AgentSessions({ actions }: { actions: Actions }) {
  const rest = useShellStore((state) => state.rest);
  const agents = useShellStore((state) => state.agents);
  const live = useShellStore((state) => state.connection === "live");
  const folds = useShellStore((state) => state.rest?.ui_state?.sessions_expanded_agent_pane_ids ?? NO_FOLDS);
  const [onlyCheckout, setOnlyCheckout] = useState<string | null>(null);
  // The sidebar's rule: a Workspace in front selects the row of its focused pane.
  const inWorkspace = useUiStore((state) => state.screen?.kind === "workspace" && !state.overviewOpen);
  const focusedPane = useShellStore((state) => state.focusedPaneId);
  const model = useMemo(() => sessionsModel(rest, agents, onlyCheckout), [rest, agents, onlyCheckout]);
  // Share the existing visible-board demand. No new polling clock or reader.
  useEffect(() => {
    if (!live) return;
    const report = () => actions.observeRequestView(document.visibilityState === "visible");
    report();
    document.addEventListener("visibilitychange", report);
    return () => {
      document.removeEventListener("visibilitychange", report);
      actions.observeRequestView(false);
    };
  }, [actions, live]);
  return <SessionsContent model={model} actions={actions} folds={folds} focusedPane={inWorkspace ? focusedPane : null} onlyCheckout={onlyCheckout} onOnlyCheckout={setOnlyCheckout} />;
}

const SessionsContent = memo(function SessionsContent({ model, actions, folds, focusedPane, onlyCheckout, onOnlyCheckout }: {
  model: SessionsModel; actions: Actions; folds: string[]; focusedPane: string | null; onlyCheckout: string | null; onOnlyCheckout: (id: string | null) => void;
}) {
  const { t } = useInterfaceTranslation();
  const [resolvedOpen, setResolvedOpen] = useState(false);
  const [idleOpen, setIdleOpen] = useState(false);
  // Siblings past five wait behind "N more" until asked, for this panel only, as in the sidebar.
  const [shownAll, setShownAll] = useState<ReadonlySet<string>>(NO_PANES);
  const groups = model.scope?.sessions.groups ?? [];
  const filtered = model.front !== null && onlyCheckout === model.front.id;
  const opened = useMemo(() => new Set(folds), [folds]);
  const byPane = useMemo(() => new Map(model.agents.map((row) => [row.pane_id, row])), [model.agents]);
  // Each shown root's drawn tree, so the selection knows every row the panel draws.
  const sections = useMemo(() => groups.map(({ group, members, more }) => {
    const shown = group === "resolved" && !resolvedOpen ? [] : [...members, ...(idleOpen ? more : [])];
    return { group, count: members.length + more.length, more: more.length, trees: shown.map((member) => {
      const row = sessionMember(model, member);
      return { row, lines: sessionTreeLines(row.agent, byPane, opened, shownAll) };
    }) };
  }), [groups, resolvedOpen, idleOpen, model, byPane, opened, shownAll]);
  const selected = useMemo(() => {
    const drawn = new Set(sections.flatMap(({ trees }) => trees.flatMap(({ lines }) => lines.flatMap((line) => (line.kind === "agent" ? [line.row.agent.pane_id] : [])))));
    return sidebarFocusPane(focusedPane, model.agents, drawn);
  }, [sections, focusedPane, model.agents]);
  const showAll = (parent: string) => setShownAll((before) => new Set(before).add(parent));
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col" data-session-panel="true">
      <header className="flex shrink-0 flex-col gap-sm border-b border-border px-md py-sm">
        <span className="truncate text-caption font-medium" title={model.project?.label}>{model.project?.label ?? t("overview.allProjects")}</span>
        {model.project && model.front ? <div className="flex min-w-0 gap-xs">
          <button type="button" aria-pressed={!filtered} className="rounded-xs bg-secondary px-xs py-xxs text-micro outline-none focus-visible:ring-1 focus-visible:ring-ring" onClick={() => onOnlyCheckout(null)}>{t("agentSessions.allCheckouts")}</button>
          <button type="button" aria-pressed={filtered} className="min-w-0 truncate rounded-xs px-xs py-xxs text-micro outline-none aria-pressed:bg-secondary focus-visible:ring-1 focus-visible:ring-ring" title={model.front.label} onClick={() => onOnlyCheckout(filtered ? null : model.front!.id)}>{t("agentSessions.onlyCheckout", { checkout: model.front.label })}</button>
        </div> : null}
      </header>
      {!model.available ? <p className="px-md py-sm text-caption text-muted-foreground" role="status">{model.reason ?? t("devices.rail.notConnected")}</p> : null}
      <div className="min-h-0 flex-1 overflow-y-auto px-sm pb-md" onKeyDown={moveFocus}>
        {groups.length === 0 ? <p className="px-xs py-md text-caption text-muted-foreground" data-sessions-empty="true">{t("requests.nothingToDo")}</p> : null}
        {sections.map(({ group, count, more, trees }) => {
          const collapsible = group === "resolved";
          const expanded = collapsible ? resolvedOpen : true;
          return <section key={group} data-session-group={group} aria-label={t(GROUP_LABEL[group])}>
            <button type="button" data-session-focus="group" className={cn("flex w-full items-center gap-xs px-xs py-sm text-left text-caption font-medium text-subtle-foreground outline-none focus-visible:ring-1 focus-visible:ring-ring", group === "needs_you" && "text-warning")}
              aria-expanded={collapsible ? expanded : undefined} onClick={() => collapsible ? setResolvedOpen(!resolvedOpen) : undefined}>
              {collapsible ? expanded ? <ChevronDownIcon className="size-(--size-icon-sm)" /> : <ChevronRightIcon className="size-(--size-icon-sm)" /> : null}
              <span>{t(GROUP_LABEL[group])}</span><span className="tabular-nums text-muted-foreground">{count}</span>
            </button>
            {expanded ? <ul className="flex min-w-0 flex-col">
              {trees.map(({ row, lines }) => <SessionTree key={`${row.project.id}:${row.agent.pane_id}`} row={row} lines={lines} model={model} actions={actions} byPane={byPane} opened={opened} selected={selected} onShowAll={showAll} />)}
              {more > 0 && !idleOpen ? <li>
                <button type="button" data-session-focus="more" data-session-more={more} onClick={() => setIdleOpen(true)} className="flex w-full items-center gap-xs rounded-xs px-xs py-sm text-left text-caption text-subtle-foreground outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring">
                  <ChevronRightIcon className="size-(--size-icon-sm)" /><span>{t("agentSessions.foldedMore", { count: more })}</span>
                </button>
              </li> : null}
            </ul> : null}
          </section>;
        })}
      </div>
    </div>
  );
}, (before, after) => before.actions === after.actions && before.folds === after.folds && before.focusedPane === after.focusedPane
  && before.onlyCheckout === after.onlyCheckout && before.onOnlyCheckout === after.onOnlyCheckout
  && sameModel(before.model, after.model));

function sameModel(a: SessionsModel, b: SessionsModel): boolean {
  return a.scope === b.scope && a.project === b.project && a.front === b.front
    && a.agents.length === b.agents.length && a.agents.every((row, index) => row === b.agents[index])
    && a.available === b.available && a.reason === b.reason && a.deviceId === b.deviceId
    && a.members.length === b.members.length && a.members.every((row, index) => {
      const next = b.members[index]!;
      return row.agent === next.agent && row.project === next.project && row.checkout === next.checkout;
    });
}

function sessionMember(model: SessionsModel, member: number): LensAgent {
  const row = model.members[member];
  if (!row) throw new Error("Sessions group references a missing member");
  return row;
}

/** A Sessions root and, through every opened row, its descendants as tree rows. */
function SessionTree({ row, lines, model, actions, byPane, opened, selected, onShowAll }: { row: LensAgent; lines: SidebarTreeLine[]; model: SessionsModel; actions: Actions; byPane: ReadonlyMap<string, AgentRow>; opened: ReadonlySet<string>; selected: string | null; onShowAll: (parent: string) => void }) {
  const { agent } = row;
  const openOf = (of: AgentRow) => (childrenOf(of, byPane).length > 0 ? opened.has(of.pane_id) : null);
  return <>
    <SessionRow row={row} model={model} actions={actions} open={openOf(agent)} selected={selected === agent.pane_id} />
    {lines.slice(1).map((line) => {
      if (line.kind === "more") return <TreeMoreRow key={`more:${line.parent}`} line={line} onShowAll={onShowAll} />;
      const child = line.row.agent;
      const parent = (child.lineage_parent_pane_id ? byPane.get(child.lineage_parent_pane_id) : undefined) ?? agent;
      return <ChildRow key={child.pane_id} parent={parent} child={child} depth={line.row.depth} model={model} actions={actions} place={line.place} open={openOf(child)} selected={selected === child.pane_id} workspace={row.project.id} />;
    })}
  </>;
}

const SessionRow = memo(function SessionRow({ row, model, actions, open, selected }: { row: LensAgent; model: SessionsModel; actions: Actions; open: boolean | null; selected: boolean }) {
  const { t } = useInterfaceTranslation();
  const { agent, project } = row;
  const staleness = useAgentStaleness(agent);
  const session = agent.state.session;
  const needsYou = session.group === "needs_you";
  const open_ = () => { if (model.available) actions.openAgent(agent.pane_id); };
  // The chevron lane spans the row so an opened row's rail runs on to its children.
  return <li className={cn("group/session relative flex min-w-0 items-stretch rounded-xs pr-xs", selected ? "bg-secondary" : "hover:bg-accent focus-within:bg-accent")}
    data-session-row={agent.pane_id} data-selected={selected ? "true" : undefined} onClick={open_}>
    <TreeChevron name={agent.identity_label} open={open} onToggle={() => actions.toggleSessionTree(agent.pane_id)} hangs={false} disabled={!model.available} data-session-chevron={agent.pane_id} />
    <div className="flex min-w-0 flex-1 flex-col gap-xxs py-xs pl-xs">
      <div className="flex min-h-(--size-sidebar-line) min-w-0 items-center gap-xs">
        <button type="button" data-session-focus="row" disabled={!model.available} onClick={(event) => { event.stopPropagation(); open_(); }}
          onKeyDown={(event) => treeKey(event, open, () => actions.toggleSessionTree(agent.pane_id))}
          aria-current={selected ? "true" : undefined}
          aria-expanded={open ?? undefined} aria-label={[agent.identity_label, agent.agent_kind, statusText(t, agent.status_code)].join(", ")} title={agent.identity_label}
          className="flex min-w-0 flex-1 items-center gap-xs text-left outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50">
          {/* Unfinished Idle work wears ◐ in the mark's place (B31); the line says what is left. */}
          {session.unfinished ? <StatusMark symbol="◐" className="text-warning" data-session-unfinished="true" /> : <StatusMark symbol={agent.symbol} className={markTone(agent)} />}<AgentMark kind={agent.agent_kind} />
          <span className="min-w-0 truncate text-caption font-medium">{agent.identity_label}</span>
        </button>
        {needsYou ? null : <AgentPrMark agent={agent} staleness={staleness} disabled={!model.available} onOpen={(pull) => actions.openSessionPullRequest({ workspace_id: project.id, url: pull.url, number: pull.number })} />}
        {needsYou || open ? null : <DescendantMark agent={agent} />}
        <Elapsed since={agent.state.request_since} className="shrink-0 text-micro text-muted-foreground" />
        {!agent.resolved ? <Hint label={t("agentSessions.resolve")}><button type="button" disabled={!model.available} aria-label={t("agentSessions.resolve")} data-session-resolve={agent.pane_id} className="shrink-0 rounded-xs p-xxs text-muted-foreground opacity-0 outline-none hover:bg-secondary group-hover/session:opacity-100 group-focus-within/session:opacity-100 focus-visible:ring-1 focus-visible:ring-ring hoverless:opacity-100" onClick={(event) => {event.stopPropagation(); actions.resolveSession(agent.pane_id);}}><CheckIcon className="size-(--size-icon-sm)" /></button></Hint> : null}
      </div>
      <SecondLine agent={agent} />
    </div>
  </li>;
});

/**
 * An opened row's descendant at any depth: the same tree row with its own
 * chevron, PR and, while folded, its descendant mark; the second line starts
 * with the branch when it works in another checkout than its parent.
 */
function ChildRow({ parent, child, depth, model, actions, place, open, selected, workspace }: { parent: AgentRow; child: AgentRow; depth: number; model: SessionsModel; actions: Actions; place: TreePlace; open: boolean | null; selected: boolean; workspace: string }) {
  const { t } = useInterfaceTranslation();
  const staleness = useAgentStaleness(child);
  const go = () => { if (model.available) actions.followRelation(parent.pane_id, child.pane_id, child.identity_label); };
  const toggle = () => actions.toggleSessionTree(child.pane_id);
  // The sidebar's title rule: a delegated title is quiet unless it asks or is selected.
  const titleTone = child.state.title_emphasized || (selected && child.state.selection_emphasizes_title) ? "text-foreground" : "text-subtle-foreground";
  return <li className={cn("flex min-w-0 items-stretch rounded-xs pr-xs", selected ? "bg-secondary" : "hover:bg-accent focus-within:bg-accent")}
    data-session-child={child.pane_id} data-depth={depth} data-selected={selected ? "true" : undefined} onClick={go}>
    <TreeRails place={place} />
    <TreeChevron name={child.identity_label} open={open} onToggle={toggle} hangs chain={place.chain} disabled={!model.available} data-session-chevron={child.pane_id} />
    <div className="flex min-w-0 flex-1 flex-col gap-xxs py-xs pl-xs">
      <div className="flex min-h-(--size-sidebar-line) min-w-0 items-center gap-xs">
        <button type="button" data-session-focus="row" disabled={!model.available} onClick={(event) => { event.stopPropagation(); go(); }}
          onKeyDown={(event) => treeKey(event, open, toggle)}
          aria-current={selected ? "true" : undefined} aria-expanded={open ?? undefined}
          aria-label={[child.identity_label, child.agent_kind, statusText(t, child.status_code), child.state.branch_badge].filter(Boolean).join(", ")} title={child.identity_label}
          className="flex min-w-0 flex-1 items-center gap-xs text-left outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50">
          <StatusMark symbol={child.symbol} className={markTone(child)} /><AgentMark kind={child.agent_kind} />
          <span className={cn("min-w-0 truncate text-caption", titleTone)}>{child.identity_label}</span>
        </button>
        <AgentPrMark agent={child} staleness={staleness} disabled={!model.available} onOpen={(pull) => actions.openSessionPullRequest({ workspace_id: workspace, url: pull.url, number: pull.number })} />
        {open ? null : <DescendantMark agent={child} />}
        <Elapsed since={child.state.request_since} className="shrink-0 text-micro text-muted-foreground" />
      </div>
      <SecondLine agent={child} branch />
    </div>
  </li>;
}

/** Past five siblings, the rest wait behind one line at their depth, as in the sidebar (B12). */
function TreeMoreRow({ line, onShowAll }: { line: Extract<SidebarTreeLine, { kind: "more" }>; onShowAll: (parent: string) => void }) {
  const { t } = useInterfaceTranslation();
  return <li className="flex min-w-0 items-stretch rounded-xs pr-xs hover:bg-accent">
    <TreeRails place={line.place} />
    <TreeChevron name="" open={null} hangs chain={line.place.chain} />
    <button type="button" data-session-focus="tree-more" data-tree-more={line.count} onClick={() => onShowAll(line.parent)}
      className="flex min-w-0 flex-1 items-center gap-xs py-xs pl-xs text-left text-caption text-subtle-foreground outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring">
      <ChevronRightIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0 text-muted-foreground" />
      <span className="min-w-0 truncate">{t("agentSessions.tree.more", { count: line.count })}</span>
    </button>
  </li>;
}

/**
 * B31: the ask for Needs You, else the core's line; a block's line is its
 * cause. A child in another checkout than its parent starts the line with
 * that branch, which keeps at most two fifths of the line for the label.
 */
function SecondLine({ agent, branch: withBranch = false }: { agent: AgentRow; branch?: boolean }) {
  const session = agent.state.session;
  const ask = session.group === "needs_you" ? agent.state.ask : null;
  const branch = withBranch ? childBranch(agent) : null;
  if (ask && !branch) return <div className="flex min-w-0 text-micro" data-session-line="ask"><AskLine ask={ask} className="flex-1" /></div>;
  if (!session.line && !ask && !branch) return null;
  const blocked = agent.status_code === "error";
  return <div className={cn("flex min-w-0 gap-xs text-micro", blocked ? "text-warning" : "text-subtle-foreground")} data-session-line={ask ? "ask" : blocked ? "blocked" : session.unfinished ? "unfinished" : session.line ? "label" : "branch"}>
    {branch ? <BranchText agent={agent} branch={branch} /> : null}
    {ask ? <AskLine ask={ask} className="min-w-0 flex-1" /> : session.line ? <span className="min-w-0 truncate" title={session.line}>{session.line}</span> : null}
  </div>;
}

/** The child's branch, dim mono, cut with … past two fifths of the line; the whole branch and path in its tooltip. */
function BranchText({ agent, branch }: { agent: AgentRow; branch: string }) {
  const path = useShellStore((state) => checkoutPathOfPane(state.rest, agent.pane_id));
  return <span className="max-w-2/5 shrink-0 truncate font-mono text-muted-foreground" data-session-branch={agent.pane_id}
    title={[agent.state.branch_badge, path].filter(Boolean).join("\n")}>{branch}</span>;
}

/** Right opens a row's children and Left folds them (B15); Up and Down move through the list. */
function treeKey(event: KeyboardEvent<HTMLElement>, open: boolean | null, toggle: () => void) {
  if (open === null || event.metaKey || event.ctrlKey || event.altKey) return;
  if ((event.key === "ArrowRight" && !open) || (event.key === "ArrowLeft" && open)) {
    event.preventDefault();
    toggle();
  }
}

function moveFocus(event: KeyboardEvent<HTMLElement>) {
  if (event.metaKey || event.ctrlKey || event.altKey) return;
  const step = event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : null;
  if (step === null && event.key !== "Home" && event.key !== "End") return;
  const items = [...event.currentTarget.querySelectorAll<HTMLButtonElement>("[data-session-focus]:not(:disabled)")];
  const current = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-session-focus]");
  const index = current ? items.indexOf(current) : -1;
  const next = event.key === "Home" ? items[0] : event.key === "End" ? items.at(-1) : items[Math.max(0, Math.min(items.length - 1, index + (step ?? 0)))];
  if (next) { event.preventDefault(); next.focus(); }
}
