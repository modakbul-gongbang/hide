import { ChevronDownIcon, ChevronRightIcon, GitPullRequestIcon } from "lucide-react";
import { memo, useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import type { Actions } from "./actions";
import { AgentMark } from "./AgentMark";
import { markTone } from "./agentRow";
import { sessionsModel, type SessionsModel } from "./sessionPanel";
import { AgentChildrenPopover } from "./components/agent-children-popover";
import { Elapsed } from "./components/elapsed";
import { StatusMark } from "./components/status-mark";
import { Hint } from "./components/ui/tooltip";
import { useInterfaceTranslation } from "./i18n/client";
import type { MessageKey } from "./i18n/catalogs";
import { cn } from "./lib/utils";
import { IssueChip, lensHandlers } from "./OverviewLenses";
import type { LensAgent } from "./overviewLens";
import type { SessionGroup, SessionTag } from "./snapshot";
import { useShellStore } from "./store";
import { ChecksMark } from "./TaskBoards";

const GROUP_LABEL: Record<SessionGroup, MessageKey> = {
  my_turn: "agentSessions.group.my_turn", review_merge: "agentSessions.group.review_merge",
  in_progress: "agentSessions.group.in_progress", resting: "agentSessions.group.resting",
  resolved_today: "agentSessions.group.resolved_today",
};
const TAG_LABEL: Record<SessionTag, MessageKey> = {
  answer: "agentSessions.tag.answer", approval: "agentSessions.tag.approval", fix: "agentSessions.tag.fix",
  review: "agentSessions.tag.review", merge: "agentSessions.tag.merge", stopped: "agentSessions.tag.stopped",
  result: "agentSessions.tag.result", working: "agentSessions.tag.working", ci_wait: "agentSessions.tag.ci_wait",
  waiting: "agentSessions.tag.waiting", idle: "agentSessions.tag.idle",
};
const STATS = ["my_turn", "review_merge", "in_progress", "resolved_today"] as const;

export function AgentSessions({ actions }: { actions: Actions }) {
  const rest = useShellStore((state) => state.rest);
  const agents = useShellStore((state) => state.agents);
  const live = useShellStore((state) => state.connection === "live");
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
  return <SessionsContent model={model} actions={actions} onlyCheckout={onlyCheckout} onOnlyCheckout={setOnlyCheckout} />;
}

const SessionsContent = memo(function SessionsContent({ model, actions, onlyCheckout, onOnlyCheckout }: {
  model: SessionsModel; actions: Actions; onlyCheckout: string | null; onOnlyCheckout: (id: string | null) => void;
}) {
  const { t } = useInterfaceTranslation();
  const [resting, setResting] = useState(false);
  const [resolved, setResolved] = useState(false);
  const scope = model.scope;
  const groups = scope?.sessions.groups ?? [];
  const filtered = model.front !== null && onlyCheckout === model.front.id;
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col" data-agent-sessions="true">
      <header className="flex shrink-0 flex-col gap-sm border-b border-border px-md py-sm">
        <span className="truncate text-caption font-medium" title={model.project?.label}>{model.project?.label ?? t("overview.allProjects")}</span>
        {model.project && model.front ? <div className="flex min-w-0 gap-xs">
          <button type="button" aria-pressed={!filtered} className="rounded-xs bg-secondary px-xs py-xxs text-micro outline-none focus-visible:ring-1 focus-visible:ring-ring" onClick={() => onOnlyCheckout(null)}>{t("agentSessions.allCheckouts")}</button>
          <button type="button" aria-pressed={filtered} className="min-w-0 truncate rounded-xs px-xs py-xxs text-micro outline-none aria-pressed:bg-secondary focus-visible:ring-1 focus-visible:ring-ring" title={model.front.label} onClick={() => onOnlyCheckout(filtered ? null : model.front!.id)}>{t("agentSessions.onlyCheckout", { checkout: model.front.label })}</button>
        </div> : null}
        <div className="grid grid-cols-4 gap-xs" data-session-counts="true">
          {STATS.map((group) => <span key={group} className={cn("flex min-w-0 flex-col text-micro text-muted-foreground", group === "my_turn" && "text-warning")}>
            <span className="text-caption font-medium tabular-nums" data-session-count={group}>{scope ? scope.sessions.counts[group] : ""}</span>
            <span className="truncate" title={t(GROUP_LABEL[group])}>{t(GROUP_LABEL[group])}</span>
          </span>)}
        </div>
      </header>
      {!model.available ? <p className="px-md py-sm text-caption text-muted-foreground" role="status">{model.reason ?? t("devices.rail.notConnected")}</p> : null}
      <div className="min-h-0 flex-1 overflow-y-auto px-sm pb-md" onKeyDown={moveFocus}>
        {groups.length === 0 ? <p className="px-xs py-md text-caption text-muted-foreground" data-sessions-empty="true">{t("requests.nothingToDo")}</p> : null}
        {groups.map(({ group, members }) => {
          const collapsible = group === "resting" || group === "resolved_today";
          const expanded = group === "resting" ? resting : group === "resolved_today" ? resolved : true;
          return <section key={group} data-session-group={group} aria-label={t(GROUP_LABEL[group])}>
            <button type="button" data-session-focus="group" className={cn("flex w-full items-center gap-xs px-xs py-sm text-left text-caption font-medium text-subtle-foreground outline-none focus-visible:ring-1 focus-visible:ring-ring", group === "my_turn" && "text-warning")}
              aria-expanded={collapsible ? expanded : undefined} onClick={() => group === "resting" ? setResting(!resting) : group === "resolved_today" ? setResolved(!resolved) : undefined}>
              {collapsible ? expanded ? <ChevronDownIcon className="size-(--size-icon-sm)" /> : <ChevronRightIcon className="size-(--size-icon-sm)" /> : null}
              <span>{t(GROUP_LABEL[group])}</span><span className="tabular-nums text-muted-foreground">{members.length}</span>
            </button>
            {expanded ? <ul className="flex min-w-0 flex-col">
              {members.map((member) => {
                const row = sessionMember(model, member);
                return <SessionRow key={`${row.project.id}:${row.agent.pane_id}`} row={row} model={model} actions={actions} />;
              })}
            </ul> : null}
          </section>;
        })}
      </div>
    </div>
  );
}, (before, after) => before.actions === after.actions
  && before.onlyCheckout === after.onlyCheckout && before.onOnlyCheckout === after.onOnlyCheckout
  && sameModel(before.model, after.model));

function sameModel(a: SessionsModel, b: SessionsModel): boolean {
  return a.scope === b.scope && a.project === b.project && a.front === b.front
    && a.agents === b.agents && a.available === b.available && a.reason === b.reason && a.deviceId === b.deviceId
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

const SessionRow = memo(function SessionRow({ row, model, actions }: { row: LensAgent; model: SessionsModel; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const button = useRef<HTMLButtonElement>(null);
  const { agent, project, checkout } = row;
  const state = agent.state.session;
  const work = project.agent_scope.work[agent.pane_id];
  const pull = work?.pull == null ? null : agent.request?.pull_requests[work.pull] ?? null;
  const issues = (work?.issue_chips ?? []).map((key) => project.tasks?.tasks.find((task) => task.key === key)).filter((task) => task !== undefined);
  const byPane = new Map(model.agents.map((child) => [child.pane_id, child]));
  const children = (project.agent_scope.children[agent.pane_id] ?? []).map((id) => {
    const child = byPane.get(id);
    if (!child) throw new Error("Sessions references a missing child");
    return child;
  });
  const handlers = lensHandlers(actions, {
    openIssue: (_project, task) => actions.openOverview(project.device_id, project.id, { issue: task.key }),
    toggleFold: () => undefined,
  });
  const tag = state.tag ? t(TAG_LABEL[state.tag]) : null;
  const line = agent.request?.line;
  const location = [model.project ? null : project.label, checkout.label].filter(Boolean).join(" · ");
  const open = () => { if (model.available) actions.openAgent(agent.pane_id); };
  return <li className={cn("group/session relative flex min-w-0 flex-col gap-xxs rounded-xs border-l-2 border-transparent px-xs py-sm hover:bg-accent focus-within:bg-accent", model.front?.id === checkout.id && "border-l-primary")}
    data-session-row={agent.pane_id} onClick={open}>
    <div className="flex min-w-0 items-center gap-xs">
      <button ref={button} type="button" data-session-focus="row" disabled={!model.available} onClick={(event) => { event.stopPropagation(); open(); }} aria-label={[agent.identity_label, tag, location].filter(Boolean).join(", ")} title={agent.identity_label}
        className="flex min-w-0 flex-1 items-center gap-xs text-left outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50">
        <StatusMark symbol={agent.symbol} className={markTone(agent)} /><AgentMark kind={agent.agent_kind} />
        <span className="min-w-0 truncate text-caption font-medium">{agent.identity_label}</span>
      </button>
      {issues.map((task) => <IssueChip key={task.key} project={project} task={task} handlers={handlers} now={Date.now()} />)}
      {pull ? <Hint label={pull.title}><button type="button" disabled={!model.available} onClick={(event) => { event.stopPropagation(); actions.openPullRequestRow(project.id, pull.number); }} className="flex shrink-0 items-center gap-xxs rounded-xs text-micro text-muted-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring" data-session-pr={pull.number}>
        <GitPullRequestIcon className="size-(--size-icon-sm)" /><span>#{pull.number}</span>
        {pull.checks === "passing" || pull.checks === "failed" || pull.checks === "pending" ? <ChecksMark checks={pull.checks} /> : null}
        {work && work.more > 0 ? <span>+{work.more}</span> : null}
      </button></Hint> : null}
      {children.length > 0 ? <AgentChildrenPopover parent={agent} childRows={children} onOpenChild={actions.openAgent} onUnfold={() => actions.openOverview(project.device_id, project.id)} returnFocus={() => button.current?.focus()}
        trigger={<button type="button" disabled={!model.available} onClick={(event) => event.stopPropagation()} aria-label={t("agentSessions.children", { count: children.length })} className="shrink-0 rounded-xs px-xxs text-micro text-muted-foreground outline-none hover:bg-secondary data-[state=open]:bg-secondary focus-visible:ring-1 focus-visible:ring-ring">↳ {children.length}</button>} /> : null}
      <Elapsed since={agent.state.request_since} className="shrink-0 text-micro text-muted-foreground" />
    </div>
    <div className="flex min-w-0 items-baseline gap-xs pl-lg text-micro">
      {tag ? <span className={cn("shrink-0 text-subtle-foreground", state.group === "my_turn" && "text-warning")}>{tag}</span> : null}
      {line ? <span className="min-w-0 flex-1 truncate text-subtle-foreground" title={line}>{line}</span> : <span className="flex-1" />}
      <span className="max-w-2/5 truncate text-muted-foreground" title={[project.label, checkout.path, checkout.branch].filter(Boolean).join(" · ")}>{location}</span>
    </div>
  </li>;
});

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
