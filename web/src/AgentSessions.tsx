import { CheckIcon, ChevronDownIcon, ChevronRightIcon, GitPullRequestIcon } from "lucide-react";
import { memo, useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import type { Actions } from "./actions";
import { AgentMark } from "./AgentMark";
import { lineTone, markTone } from "./agentRow";
import { sessionsModel, type SessionsModel } from "./sessionPanel";
import { DescendantBadge } from "./components/agent-row";
import { Elapsed } from "./components/elapsed";
import { StatusMark } from "./components/status-mark";
import { Hint } from "./components/ui/tooltip";
import { useInterfaceTranslation } from "./i18n/client";
import type { MessageKey } from "./i18n/catalogs";
import { formatDateTime } from "./i18n/format";
import { requireInterfaceLanguage } from "./i18n/locale";
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
    <div className="flex min-h-0 min-w-0 flex-1 flex-col" data-session-panel="true">
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
              <span>{t(GROUP_LABEL[group])}</span><span className="tabular-nums text-muted-foreground">{scope!.sessions.counts[group]}</span>
            </button>
            {expanded ? <ul className="flex min-w-0 flex-col">
              {members.map((member) => {
                const row = sessionMember(model, member);
                return <SessionRow key={`${row.project.id}:${row.agent.pane_id}`} row={row} model={model} actions={actions} />;
              })}
              {group === "review_merge" ? scope!.sessions.closed_prs.map((row) => <ClosedSessionRow key={`${row.project_id}:${row.number}`} state={row} model={model} actions={actions} />) : null}
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

const SessionRow = memo(function SessionRow({ row, model, actions }: { row: LensAgent; model: SessionsModel; actions: Actions }) {
  const { t, i18n } = useInterfaceTranslation();
  const button = useRef<HTMLButtonElement>(null);
  const { agent, project, checkout } = row;
  const state = agent.state.session;
  const work = project.agent_scope.work[agent.pane_id];
  const pull = work?.pull == null ? null : agent.request?.pull_requests[work.pull] ?? null;
  const issues = (work?.issue_chips ?? []).map((key) => project.tasks?.tasks.find((task) => task.key === key)).filter((task) => task !== undefined);
  const byPane = new Map(model.agents.map((child) => [child.pane_id, child]));
  const children = (agent.lineage_child_pane_ids ?? []).map((id) => {
    const child = byPane.get(id);
    if (!child) throw new Error("Sessions references a missing child");
    return child;
  });
  const handlers = lensHandlers(actions, {
    openIssue: (_project, task) => actions.openOverview(project.device_id, project.id, { issue: task.key }),
    toggleFold: () => undefined,
  });
  const tag = state.tag ? t(TAG_LABEL[state.tag]) : null;
  const raisedLine = agent.state.line?.mode === "raised_child" ? agent.state.line : null;
  const line = raisedLine?.text ?? agent.request?.line;
  const github = checkout.github;
  const stale = github?.stale === true;
  const pullHint = [pull?.title, stale && github?.last_success_at_unix_ms != null ? t("agentSessions.lastRead", {time: formatDateTime(requireInterfaceLanguage(i18n.language), github.last_success_at_unix_ms, { dateStyle: "short", timeStyle: "short" })}) : null].filter(Boolean).join("\n");
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
      {pull ? <Hint label={pullHint}><button type="button" disabled={!model.available} onClick={(event) => { event.stopPropagation(); actions.openSessionPullRequest({workspace_id: project.id, url: pull.url, number: pull.number}); }} className="flex shrink-0 items-center gap-xxs rounded-xs text-micro text-muted-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring" data-session-pr={pull.number} style={stale ? {opacity: 0.5} : undefined}>
        <GitPullRequestIcon className="size-(--size-icon-sm)" /><span>#{pull.number}</span>
        {pull.checks === "passing" || pull.checks === "failed" || pull.checks === "pending" ? <ChecksMark checks={pull.checks} /> : null}
        {work && work.more > 0 ? <span>+{work.more}</span> : null}
      </button></Hint> : null}
      {children.length > 0 ? <span onClick={(event) => event.stopPropagation()}><DescendantBadge agent={agent} descendants={children.length} childRows={children} onOpenChild={(pane) => actions.followRelation(agent.pane_id, pane, byPane.get(pane)!.identity_label)} onUnfold={() => actions.openAgentsOverview()} returnFocus={() => button.current?.focus()} /></span> : null}
      <Elapsed since={agent.state.request_since} className="shrink-0 text-micro text-muted-foreground" />
      {!agent.resolved ? <Hint label={t("agentSessions.resolve")}><button type="button" disabled={!model.available} aria-label={t("agentSessions.resolve")} data-session-resolve={agent.pane_id} className="shrink-0 rounded-xs p-xxs text-muted-foreground opacity-0 outline-none hover:bg-secondary group-hover/session:opacity-100 group-focus-within/session:opacity-100 focus-visible:ring-1 focus-visible:ring-ring hoverless:opacity-100" onClick={(event) => {event.stopPropagation(); actions.resolveSession(agent.pane_id);}}><CheckIcon className="size-(--size-icon-sm)" /></button></Hint> : null}
    </div>
    <div className="flex min-w-0 items-baseline gap-xs pl-lg text-micro">
      {agent.escalation && agent.lineage_parent_pane_id ? <span className="max-w-1/3 truncate text-muted-foreground" title={byPane.get(agent.lineage_parent_pane_id)?.identity_label}>↰ {byPane.get(agent.lineage_parent_pane_id)?.identity_label}</span> : null}
      {tag ? <span className={cn("shrink-0 text-subtle-foreground", state.group === "my_turn" && "text-warning", state.tag === "fix" && "text-destructive")}>{tag}</span> : null}
      {line ? <span className={cn("min-w-0 flex-1 truncate", raisedLine ? lineTone(raisedLine, agent) : "text-subtle-foreground")} title={line}>{line}</span> : <span className="flex-1" />}
      <span className="max-w-2/5 truncate text-muted-foreground" title={[project.label, checkout.path, checkout.branch].filter(Boolean).join(" · ")}>{location}</span>
    </div>
  </li>;
});

function ClosedSessionRow({ state, model, actions }: { state: NonNullable<SessionsModel["scope"]>["sessions"]["closed_prs"][number]; model: SessionsModel; actions: Actions }) {
  const { t, i18n } = useInterfaceTranslation();
  const project = model.projects.find((project) => project.id === state.project_id);
  const pull = project?.pull_requests?.find((pull) => pull.number === state.number);
  if (!project || !pull) throw new Error("Closed Sessions PR is missing its source facts");
  const issue = project.agent_scope.prs.rows.find((row) => row.number === pull.number)?.issue;
  const status = project.checkouts.find((checkout) => checkout.github)?.github;
  const stale = status?.stale === true;
  const title = [pull.title, stale && status?.last_success_at_unix_ms != null ? t("agentSessions.lastRead", {time: formatDateTime(requireInterfaceLanguage(i18n.language), status.last_success_at_unix_ms, { dateStyle: "short", timeStyle: "short" })}) : null].filter(Boolean).join("\n");
  return <li className="rounded-xs px-xs py-sm hover:bg-accent focus-within:bg-accent" data-session-closed-pr={pull.number}>
    <button type="button" disabled={!model.available} data-session-focus="row" className="flex w-full min-w-0 flex-col gap-xxs text-left outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50" title={title} onClick={() => actions.openPullRequestRow(project.id, pull.number)}>
      <span className="flex w-full min-w-0 items-center gap-xs text-caption"><GitPullRequestIcon className="size-(--size-icon-sm) shrink-0" /><span className="min-w-0 flex-1 truncate">{pull.title}</span>
        {issue ? <span className="shrink-0 rounded-xs bg-secondary px-xs text-micro">{issue.label}</span> : null}
        <span className={cn("flex shrink-0 items-center gap-xxs text-micro", stale && "opacity-50")}>#{pull.number}{pull.checks === "passing" || pull.checks === "failed" || pull.checks === "pending" ? <ChecksMark checks={pull.checks} /> : null}</span>
      </span>
      <span className="flex w-full min-w-0 gap-xs pl-lg text-micro text-muted-foreground"><span className="shrink-0 text-subtle-foreground">{t(TAG_LABEL[state.tag])}</span><span className="shrink-0">{t("agentSessions.closed")}</span><span className="min-w-0 flex-1 truncate" title={pull.head_branch}>{pull.head_branch}</span>{!model.project ? <span className="max-w-2/5 truncate">{project.label}</span> : null}</span>
    </button>
  </li>;
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
