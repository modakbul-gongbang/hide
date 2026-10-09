import { CheckIcon, ChevronDownIcon, ChevronRightIcon } from "lucide-react";
import { memo, useEffect, useMemo, useState, type KeyboardEvent } from "react";
import type { Actions } from "./actions";
import { AgentMark } from "./AgentMark";
import { markTone } from "./agentRow";
import { sessionsModel, type SessionsModel } from "./sessionPanel";
import { AskLine, DescendantMark, PrChip, TreeChevron, TreeRails, treePlaces, type PrStaleness } from "./components/agent-tree";
import { childrenOf } from "./components/agent-tree-popover";
import { Elapsed } from "./components/elapsed";
import { StatusMark } from "./components/status-mark";
import { Hint } from "./components/ui/tooltip";
import { statusText } from "./agentStatus";
import { useInterfaceTranslation } from "./i18n/client";
import type { MessageKey } from "./i18n/catalogs";
import { cn } from "./lib/utils";
import type { LensAgent } from "./overviewLens";
import type { AgentRow, SessionGroup } from "./snapshot";
import { useShellStore } from "./store";

const GROUP_LABEL: Record<SessionGroup, MessageKey> = {
  needs_you: "agents.group.needs_you", working: "agents.group.working", done: "agents.group.done",
  idle: "agentSessions.group.idle", resolved: "agentSessions.group.resolved",
};
const NO_FOLDS: string[] = [];

/**
 * Sessions (PRD agent-hierarchy-screens D-16, D-17; B26 to B33): the core
 * groups and orders the roots; a row opens one level of its children, kept
 * apart from the sidebar's folds, and grandchildren show only as marks.
 */
export function AgentSessions({ actions }: { actions: Actions }) {
  const rest = useShellStore((state) => state.rest);
  const agents = useShellStore((state) => state.agents);
  const live = useShellStore((state) => state.connection === "live");
  const folds = useShellStore((state) => state.rest?.ui_state?.sessions_expanded_agent_pane_ids ?? NO_FOLDS);
  const [onlyCheckout, setOnlyCheckout] = useState<string | null>(null);
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
  return <SessionsContent model={model} actions={actions} folds={folds} onlyCheckout={onlyCheckout} onOnlyCheckout={setOnlyCheckout} />;
}

const SessionsContent = memo(function SessionsContent({ model, actions, folds, onlyCheckout, onOnlyCheckout }: {
  model: SessionsModel; actions: Actions; folds: string[]; onlyCheckout: string | null; onOnlyCheckout: (id: string | null) => void;
}) {
  const { t } = useInterfaceTranslation();
  const [resolvedOpen, setResolvedOpen] = useState(false);
  const [idleOpen, setIdleOpen] = useState(false);
  const groups = model.scope?.sessions.groups ?? [];
  const filtered = model.front !== null && onlyCheckout === model.front.id;
  const opened = useMemo(() => new Set(folds), [folds]);
  const byPane = useMemo(() => new Map(model.agents.map((row) => [row.pane_id, row])), [model.agents]);
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
        {groups.map(({ group, members, more }) => {
          const collapsible = group === "resolved";
          const expanded = collapsible ? resolvedOpen : true;
          return <section key={group} data-session-group={group} aria-label={t(GROUP_LABEL[group])}>
            <button type="button" data-session-focus="group" className={cn("flex w-full items-center gap-xs px-xs py-sm text-left text-caption font-medium text-subtle-foreground outline-none focus-visible:ring-1 focus-visible:ring-ring", group === "needs_you" && "text-warning")}
              aria-expanded={collapsible ? expanded : undefined} onClick={() => collapsible ? setResolvedOpen(!resolvedOpen) : undefined}>
              {collapsible ? expanded ? <ChevronDownIcon className="size-(--size-icon-sm)" /> : <ChevronRightIcon className="size-(--size-icon-sm)" /> : null}
              <span>{t(GROUP_LABEL[group])}</span><span className="tabular-nums text-muted-foreground">{members.length + more.length}</span>
            </button>
            {expanded ? <ul className="flex min-w-0 flex-col">
              {[...members, ...(idleOpen ? more : [])].map((member) => {
                const row = sessionMember(model, member);
                return <SessionTree key={`${row.project.id}:${row.agent.pane_id}`} row={row} model={model} actions={actions} byPane={byPane} open={opened.has(row.agent.pane_id)} />;
              })}
              {more.length > 0 && !idleOpen ? <li>
                <button type="button" data-session-focus="more" data-session-more={more.length} onClick={() => setIdleOpen(true)} className="flex w-full items-center gap-xs rounded-xs px-xs py-sm text-left text-caption text-subtle-foreground outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring">
                  <ChevronRightIcon className="size-(--size-icon-sm)" /><span>{t("agentSessions.foldedMore", { count: more.length })}</span>
                </button>
              </li> : null}
            </ul> : null}
          </section>;
        })}
      </div>
    </div>
  );
}, (before, after) => before.actions === after.actions && before.folds === after.folds
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

/** A Sessions root and, while opened, its direct children as tree rows (B32). */
function SessionTree({ row, model, actions, byPane, open }: { row: LensAgent; model: SessionsModel; actions: Actions; byPane: ReadonlyMap<string, AgentRow>; open: boolean }) {
  const { agent, checkout } = row;
  const children = childrenOf(agent, byPane);
  const staleness: PrStaleness = { stale: checkout.github?.stale === true, lastRead: checkout.github?.last_success_at_unix_ms ?? null };
  // B32: Sessions opens one level; a child's own children show only as its mark.
  const shown = open && children.length > 0 ? children : [];
  const places = treePlaces(shown.map(() => 1));
  return <>
    <SessionRow row={row} model={model} actions={actions} staleness={staleness} open={children.length > 0 ? open : null} />
    {shown.map((child, index) => <ChildRow key={child.pane_id} parent={agent} child={child} model={model} actions={actions} staleness={staleness} place={places[index]!} workspace={row.project.id} />)}
  </>;
}

const SessionRow = memo(function SessionRow({ row, model, actions, staleness, open }: { row: LensAgent; model: SessionsModel; actions: Actions; staleness: PrStaleness; open: boolean | null }) {
  const { t } = useInterfaceTranslation();
  const { agent, project } = row;
  const session = agent.state.session;
  const needsYou = session.group === "needs_you";
  const open_ = () => { if (model.available) actions.openAgent(agent.pane_id); };
  // The chevron lane spans the row so an opened row's rail runs on to its children.
  return <li className="group/session relative flex min-w-0 items-stretch rounded-xs pr-xs hover:bg-accent focus-within:bg-accent"
    data-session-row={agent.pane_id} onClick={open_}>
    <TreeChevron name={agent.identity_label} open={open} onToggle={() => actions.toggleSessionTree(agent.pane_id)} hangs={false} disabled={!model.available} data-session-chevron={agent.pane_id} />
    <div className="flex min-w-0 flex-1 flex-col gap-xxs py-xs pl-xs">
      <div className="flex min-h-(--size-sidebar-line) min-w-0 items-center gap-xs">
        <button type="button" data-session-focus="row" disabled={!model.available} onClick={(event) => { event.stopPropagation(); open_(); }}
          onKeyDown={(event) => treeKey(event, open, () => actions.toggleSessionTree(agent.pane_id))}
          aria-expanded={open ?? undefined} aria-label={[agent.identity_label, agent.agent_kind, statusText(t, agent.status_code)].join(", ")} title={agent.identity_label}
          className="flex min-w-0 flex-1 items-center gap-xs text-left outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50">
          {/* Unfinished Idle work wears ◐ in the mark's place (B31); the line says what is left. */}
          {session.unfinished ? <StatusMark symbol="◐" className="text-warning" data-session-unfinished="true" /> : <StatusMark symbol={agent.symbol} className={markTone(agent)} />}<AgentMark kind={agent.agent_kind} />
          <span className="min-w-0 truncate text-caption font-medium">{agent.identity_label}</span>
        </button>
        {needsYou ? null : <PrChip agent={agent} staleness={staleness} disabled={!model.available} onOpen={(pull) => actions.openSessionPullRequest({ workspace_id: project.id, url: pull.url, number: pull.number })} />}
        {needsYou || open ? null : <DescendantMark agent={agent} />}
        <Elapsed since={agent.state.request_since} className="shrink-0 text-micro text-muted-foreground" />
        {!agent.resolved ? <Hint label={t("agentSessions.resolve")}><button type="button" disabled={!model.available} aria-label={t("agentSessions.resolve")} data-session-resolve={agent.pane_id} className="shrink-0 rounded-xs p-xxs text-muted-foreground opacity-0 outline-none hover:bg-secondary group-hover/session:opacity-100 group-focus-within/session:opacity-100 focus-visible:ring-1 focus-visible:ring-ring hoverless:opacity-100" onClick={(event) => {event.stopPropagation(); actions.resolveSession(agent.pane_id);}}><CheckIcon className="size-(--size-icon-sm)" /></button></Hint> : null}
      </div>
      <SecondLine agent={agent} />
    </div>
  </li>;
});

/** An opened Sessions row's child: the same tree row, one level, its own PR and, for its children, only the mark. */
function ChildRow({ parent, child, model, actions, staleness, place, workspace }: { parent: AgentRow; child: AgentRow; model: SessionsModel; actions: Actions; staleness: PrStaleness; place: ReturnType<typeof treePlaces>[number]; workspace: string }) {
  const { t } = useInterfaceTranslation();
  const open = () => { if (model.available) actions.followRelation(parent.pane_id, child.pane_id, child.identity_label); };
  return <li className="flex min-w-0 items-stretch rounded-xs pr-xs hover:bg-accent focus-within:bg-accent" data-session-child={child.pane_id} onClick={open}>
    <TreeRails place={place} />
    <TreeChevron name={child.identity_label} open={null} hangs />
    <div className="flex min-w-0 flex-1 flex-col gap-xxs py-xs pl-xs">
      <div className="flex min-w-0 items-center gap-xs">
        <button type="button" data-session-focus="row" disabled={!model.available} onClick={(event) => { event.stopPropagation(); open(); }}
          aria-label={[child.identity_label, child.agent_kind, statusText(t, child.status_code)].join(", ")} title={child.identity_label}
          className="flex min-w-0 flex-1 items-center gap-xs text-left outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50">
          <StatusMark symbol={child.symbol} className={markTone(child)} /><AgentMark kind={child.agent_kind} />
          <span className="min-w-0 truncate text-caption">{child.identity_label}</span>
        </button>
        <PrChip agent={child} staleness={staleness} disabled={!model.available} onOpen={(pull) => actions.openSessionPullRequest({ workspace_id: workspace, url: pull.url, number: pull.number })} />
        <DescendantMark agent={child} />
        <Elapsed since={child.state.request_since} className="shrink-0 text-micro text-muted-foreground" />
      </div>
    </div>
  </li>;
}

/** B31: the ask for Needs You, else the core's line; a block's line is its cause. */
function SecondLine({ agent }: { agent: AgentRow }) {
  const session = agent.state.session;
  const ask = session.group === "needs_you" ? agent.state.ask : null;
  if (ask) return <div className="flex min-w-0 text-micro" data-session-line="ask"><AskLine ask={ask} className="flex-1" /></div>;
  if (!session.line) return null;
  const blocked = agent.status_code === "error";
  return <div className={cn("flex min-w-0 text-micro", blocked ? "text-warning" : "text-subtle-foreground")} data-session-line={blocked ? "blocked" : session.unfinished ? "unfinished" : "label"}>
    <span className="min-w-0 truncate" title={session.line}>{session.line}</span>
  </div>;
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
