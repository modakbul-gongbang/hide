import { ChevronDownIcon, CircleAlertIcon, GitPullRequestIcon, MoonIcon } from "lucide-react";
import type { TFunction } from "i18next";
import type { Actions } from "./actions";
import { AgentMark } from "./AgentMark";
import { askWhat, TreeButtonFace, VerbText } from "./components/agent-tree";
import { AgentPrMark, useAgentStaleness } from "./components/pr-mark";
import { reviewWord } from "./prMark";
import { AgentTreePopover } from "./components/agent-tree-popover";
import { Elapsed } from "./components/elapsed";
import { Hint } from "./components/ui/tooltip";
import { useInterfaceTranslation } from "./i18n/client";
import type { MessageKey } from "./i18n/catalogs";
import { cn } from "./lib/utils";
import type { PaneHeader, SnapshotRest, AgentRow } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { relationState } from "./lineage";
import { deviceConnected } from "./devices";
import { remoteTargetOfPane } from "./remote";

const LABELS: Record<string, MessageKey> = {
  sleeping: "panes.sleep.captionSleeping", waking: "panes.sleep.captionWaking", failed: "panes.sleep.captionFailed",
  disconnected: "panes.transport.disconnected", closing: "panes.transport.closing", starting: "panes.transport.starting",
  unavailable: "panes.transport.remoteUnavailable", terminated: "panes.transport.remoteEnded", exit: "agentSessions.exitCode",
  controlled_elsewhere: "panes.transport.scrollElsewhere", device_offline: "panes.transport.disconnected",
  blocked: "agentSessions.tag.blocked", stopped: "agentSessions.tag.stopped",
  result: "agentSessions.tag.result", fix: "agentSessions.tag.fix", review: "agentSessions.tag.review", merge: "agentSessions.tag.merge",
};
const TONES = { muted: "bg-secondary text-muted-foreground", warning: "bg-warning/10 text-warning", error: "bg-destructive/10 text-destructive", success: "bg-success/10 text-success", pr: "bg-pr-mergeable/10 text-pr-mergeable" };
const ACTION_TONES = { muted: "bg-secondary text-secondary-foreground", warning: "bg-warning text-status-foreground", error: "bg-destructive text-destructive-foreground", success: "bg-success text-status-foreground", pr: "bg-pr-mergeable text-status-foreground" };

export function PaneHeaderBand({ paneId, header, actions }: { paneId: string; header: PaneHeader | undefined; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const relation = useUiStore((state) => state.relation);
  const outcome = useShellStore((state) => state.rest?.status?.pane_focus_request);
  const band = header?.band;
  // Before any early return: a hook runs on every render.
  const unreachable = useUnreachable(band?.action?.kind === "child" ? band.action.pane_id : null);
  if (!band) return header?.working ? <div className="pointer-events-none absolute inset-x-0 top-0 z-10 h-[calc(2*var(--size-hairline))] bg-agent-working" data-pane-working-line={paneId} /> : null;
  const action = band.action;
  const tracked = action?.kind === "child" && relation?.sourcePaneId === paneId && relation.targetPaneId === action.pane_id ? relation : null;
  const progress = relationState(tracked, outcome, t);
  const navigation = progress?.phase === "failed" ? progress.message : progress?.phase === "pending" ? t("panes.relation.opening", { name: tracked!.label }) : null;
  const ask = band.kind === "raised" || band.kind === "approval" || band.kind === "answer";
  const labelKey = LABELS[band.kind];
  if (!ask && !labelKey) throw new Error(`Unknown core pane band: ${band.kind}`);
  const label = ask ? null : band.kind === "exit" ? t("agentSessions.exitCode", { exitCode: band.exit_code! }) : t(labelKey as Exclude<MessageKey, "agentSessions.exitCode">);
  const Icon = ["sleeping", "waking"].includes(band.kind) ? MoonIcon : ["fix", "review", "merge"].includes(band.kind) ? GitPullRequestIcon : CircleAlertIcon;
  const reason = navigation ?? (ask ? null : bandReason(band, t));
  // An ask reads as text on a quiet fill with the warning rail (D-43); the verb carries the colour.
  return <div className="absolute inset-x-0 top-0 z-10 bg-background"><div className={cn("flex h-[var(--size-pane-header)] min-w-0 items-center gap-xs px-sm text-caption", ask ? "border-l-2 border-warning bg-secondary text-foreground" : TONES[band.tone])} data-pane-header-band={band.kind}>
    {ask ? <AskBand paneId={paneId} band={band} navigation={navigation} unreachable={unreachable} failed={progress?.phase === "failed"} actions={actions} /> : <>
      <Icon className="size-(--size-icon-sm) shrink-0" aria-hidden="true" /><span className="shrink-0">{label}</span>
      <Hint label={reason ?? label!}><span className={cn("min-w-0 flex-1 truncate", progress?.phase === "failed" && "text-destructive")} role={progress?.phase === "failed" ? "alert" : progress?.phase === "pending" ? "status" : undefined} data-pane-band-navigation={progress?.phase}>{reason}</span></Hint>
      {band.more > 0 ? <span className="shrink-0 text-micro">+{band.more}</span> : null}
      <Elapsed since={band.since_unix_ms} className="shrink-0 font-mono text-micro" />
    </>}
    {action ? <button type="button" data-pane-band-open={paneId} disabled={unreachable !== null || progress?.phase === "pending" || (progress?.phase === "failed" && !progress.retryable)} aria-busy={progress?.phase === "pending"} className={cn("shrink-0 rounded-xs px-xs py-xxs text-micro outline-none hover:brightness-95 focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50", ask ? "border border-border bg-background text-foreground" : ACTION_TONES[band.tone])} onClick={() => action.kind === "child" ? actions.followRelation(paneId, action.pane_id, action.label) : actions.openSessionPullRequest(action)}>{progress?.phase === "failed" && progress.retryable ? t("common.retry") : action.kind === "child" ? t("common.open") : t("agentSessions.openPr")}</button> : null}
  </div></div>;
}

/**
 * An ask band (PRD D-43; B4 to B7): verb · what · who · waited · 외 N건 · Open.
 * The pane's own approval or question names no one; a raised descendant's
 * names its provider and title, whose hover gives the path from the root,
 * the checkout and the wait. 외 N건 opens the tree with the raised rows first.
 */
function AskBand({ paneId, band, navigation, unreachable, failed, actions }: { paneId: string; band: NonNullable<PaneHeader["band"]>; navigation: string | null; unreachable: string | null; failed: boolean; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const rest = useShellStore((state) => state.rest);
  const local = useShellStore((state) => state.agents);
  const agents = rows(rest, local);
  const self = agents.find((agent) => agent.pane_id === paneId);
  const raised = band.raised ?? null;
  const verb = raised?.verb ?? (band.kind === "answer" ? "answer" : "approval");
  const what = askWhat(t, verb, raised ? raised.what : band.reason ?? (band.facts ? bandReason(band, t) : null));
  const minutes = band.since_unix_ms == null ? null : Math.max(0, Math.floor((Date.now() - band.since_unix_ms) / 60_000));
  // The path from the root already ends with the agent named; the wait is its verb's (B5).
  const who = raised ? [
    raised.path.join(" › "),
    [raised.checkout, minutes === null ? null : t("agentSessions.waited", { minutes, verb: t(`agentSessions.verb.${verb}`) })].filter(Boolean).join(" · "),
  ].filter(Boolean).join("\n") : null;
  const more = band.more > 0 && self ? <AgentTreePopover
    parent={self}
    agents={agents}
    raised
    onOpenChild={(id, label) => actions.followRelation(paneId, id, label)}
    onGraph={() => actions.openAgentsOverview()}
    returnFocus={() => document.querySelector<HTMLButtonElement>(`[data-pane-band-more="${paneId}"]`)?.focus()}
    triggerLabel={t("agentSessions.moreAsks", { count: band.more })}
    trigger={<button type="button" data-pane-band-more={paneId} className="shrink-0 rounded-xs px-xs py-xxs text-micro text-muted-foreground underline-offset-2 outline-none hover:text-foreground hover:underline focus-visible:ring-1 focus-visible:ring-ring">{t("agentSessions.moreAsks", { count: band.more })}</button>}
  /> : null;
  return <>
    <VerbText verb={verb} />
    <Hint label={navigation ?? what}><span className={cn("min-w-0 flex-1 truncate", failed && "text-destructive")} role={failed ? "alert" : navigation ? "status" : undefined} data-pane-band-navigation={navigation ? failed ? "failed" : "pending" : undefined}>{navigation ?? what}</span></Hint>
    {unreachable && !navigation ? <span className="min-w-0 max-w-1/4 shrink truncate text-muted-foreground" title={unreachable} data-pane-band-unreachable="true">{unreachable}</span> : null}
    {raised ? <Hint label={who!}><span className="flex min-w-0 max-w-1/3 shrink items-center gap-xxs text-muted-foreground" data-pane-band-who={raised.pane_id}><AgentMark kind={raised.agent_kind} /><span className="min-w-0 truncate">{raised.title}</span></span></Hint> : null}
    {raised?.unreceived_by && minutes !== null ? <span className="min-w-0 max-w-1/4 shrink truncate text-muted-foreground" data-pane-band-unreceived="true">{t("agentSessions.unreceived", { name: raised.unreceived_by, minutes })}</span> : null}
    {raised?.unreceived_by ? null : <Elapsed since={band.since_unix_ms} className="shrink-0 font-mono text-micro text-muted-foreground" />}
    {more}
  </>;
}

/** Why a pane on a device that is not connected cannot be opened now, or null when it can (B8). */
function useUnreachable(paneId: string | null): string | null {
  const { t } = useInterfaceTranslation();
  return useShellStore((state) => {
    if (paneId === null) return null;
    const device = remoteTargetOfPane(state.rest, paneId);
    if (device === null || deviceConnected(state.rest, device)) return null;
    return state.rest?.status?.remote?.find((row) => row.target_id === device)?.message ?? t("devices.rail.notConnected");
  });
}

function rows(rest: SnapshotRest | null, local: AgentRow[]): AgentRow[] {
  return [...local, ...(rest?.status?.remote ?? []).flatMap((remote) => remote.session?.agents ?? [])];
}

/** Every agent row this page knows, local and from connected devices. */
export function usePaneAgents(): AgentRow[] {
  const rest = useShellStore((state) => state.rest);
  const local = useShellStore((state) => state.agents);
  return rows(rest, local);
}

/** The pane's own PR mark after its title (B21, B22); a descendant's PRs stay on its own rows. */
export function PanePrChip({ paneId, actions }: { paneId: string; actions: Actions }) {
  const agent = usePaneAgents().find((row) => row.pane_id === paneId);
  if (!agent?.state.pr) return null;
  return <PaneAgentPrMark agent={agent} actions={actions} />;
}

function PaneAgentPrMark({ agent, actions }: { agent: AgentRow; actions: Actions }) {
  const staleness = useAgentStaleness(agent);
  // The pane names no project; the PR's URL finds the one that lists it.
  return <AgentPrMark agent={agent} staleness={staleness} onOpen={(pull) => actions.openSessionPullRequest({ workspace_id: null, url: pull.url, number: pull.number })} />;
}

/** The pane header's child button (B21): a tree icon and the direct child count, opening the tree popover. */
export function PaneTreeButton({ paneId, actions }: { paneId: string; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const agents = usePaneAgents();
  const parent = agents.find((agent) => agent.pane_id === paneId);
  if (!parent) return null;
  const count = (parent.lineage_child_pane_ids ?? []).filter((id) => agents.some((agent) => agent.pane_id === id)).length;
  if (count === 0) return null;
  const label = t("agentSessions.tree.children", { count });
  return <AgentTreePopover
    parent={parent}
    agents={agents}
    onOpenChild={(id, name) => actions.followRelation(paneId, id, name)}
    onGraph={() => actions.openAgentsOverview()}
    returnFocus={() => document.querySelector<HTMLButtonElement>(`[data-pane-tree="${paneId}"]`)?.focus()}
    triggerLabel={label}
    trigger={<button type="button" aria-label={label} data-pane-tree={paneId} className="flex h-(--size-sidebar-line-detail) shrink-0 items-center gap-xxs rounded-xs border border-border px-xs outline-none hover:bg-popover focus-visible:ring-1 focus-visible:ring-ring data-[state=open]:bg-secondary"><TreeButtonFace count={count} /><ChevronDownIcon aria-hidden="true" className="size-(--size-icon-sm) text-muted-foreground" /></button>}
  />;
}

/** Localize the core's facts; no verb or attention decision lives here. */
function bandReason(band: NonNullable<PaneHeader["band"]>, t: TFunction<"translation">): string | null {
  const facts = band.facts;
  if (!facts) return band.reason;
  if (facts.kind === "approval_command_unavailable") return t("agentSessions.approvalCommandUnavailable");
  return [t(`agentSessions.checks.${facts.checks}`), t(reviewWord(facts.review)?.key ?? "agentSessions.review.unknown"), facts.checks === "failed" ? t("agentSessions.checkNamesUnavailable") : null].filter(Boolean).join(" · ");
}
