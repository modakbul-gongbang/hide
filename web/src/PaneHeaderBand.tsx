import { CircleAlertIcon, CornerDownRightIcon, GitPullRequestIcon, MessageCircleIcon, MoonIcon } from "lucide-react";
import type { Actions } from "./actions";
import { DescendantBadge } from "./components/agent-row";
import { Elapsed } from "./components/elapsed";
import { Hint } from "./components/ui/tooltip";
import { useInterfaceTranslation } from "./i18n/client";
import type { MessageKey } from "./i18n/catalogs";
import { cn } from "./lib/utils";
import type { PaneHeader, SnapshotRest, AgentRow } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { relationState } from "./lineage";

const LABELS: Record<string, MessageKey> = {
  sleeping: "panes.sleep.captionSleeping", waking: "panes.sleep.captionWaking", failed: "panes.sleep.captionFailed",
  disconnected: "panes.transport.disconnected", closing: "panes.transport.closing", starting: "panes.transport.starting",
  unavailable: "panes.transport.remoteUnavailable", terminated: "panes.transport.remoteEnded", exit: "agentSessions.exitCode",
  controlled_elsewhere: "panes.transport.scrollElsewhere", device_offline: "panes.transport.disconnected",
  approval: "agentSessions.tag.approval", answer: "agentSessions.tag.answer", stopped: "agentSessions.tag.stopped",
  result: "agentSessions.tag.result", fix: "agentSessions.tag.fix", review: "agentSessions.tag.review", merge: "agentSessions.tag.merge",
  raised_child: "agentSessions.children",
};
const TONES = { muted: "bg-secondary text-muted-foreground", warning: "bg-warning/10 text-warning", error: "bg-destructive/10 text-destructive", success: "bg-success/10 text-success", pr: "bg-pr-open/10 text-pr-open" };

export function PaneHeaderBand({ paneId, header, actions }: { paneId: string; header: PaneHeader | undefined; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const relation = useUiStore((state) => state.relation);
  const outcome = useShellStore((state) => state.rest?.status?.pane_focus_request);
  const band = header?.band;
  if (!band) return header?.working ? <div className="pointer-events-none absolute inset-x-0 top-0 z-10 h-[2px] bg-agent-working" data-pane-working-line={paneId} /> : null;
  const labelKey = LABELS[band.kind];
  if (!labelKey) throw new Error(`Unknown core pane band: ${band.kind}`);
  const label = band.kind === "raised_child" ? (band.child_tag ? t(LABELS[band.child_tag]!) : "↳") : band.kind === "exit" ? t("agentSessions.exitCode", { exitCode: band.exit_code! }) : t(labelKey as Exclude<MessageKey, "agentSessions.exitCode">);
  const Icon = ["sleeping", "waking"].includes(band.kind) ? MoonIcon : band.kind === "raised_child" ? CornerDownRightIcon : ["fix", "review", "merge"].includes(band.kind) ? GitPullRequestIcon : band.kind === "answer" ? MessageCircleIcon : CircleAlertIcon;
  const action = band.action;
  const tracked = action?.kind === "child" && relation?.sourcePaneId === paneId && relation.targetPaneId === action.pane_id ? relation : null;
  const progress = relationState(tracked, outcome, t);
  const reason = progress?.phase === "failed" ? progress.message : progress?.phase === "pending" ? t("panes.relation.opening", { name: tracked!.label }) : band.reason;
  return <div className={cn("absolute inset-x-0 top-0 z-10 flex h-7 min-w-0 items-center gap-xs px-sm text-caption", TONES[band.tone])} data-pane-header-band={band.kind}>
    <Icon className="size-(--size-icon-sm) shrink-0" aria-hidden="true" /><span className="shrink-0">{label}</span>
    <Hint label={reason ?? label}><span className={cn("min-w-0 flex-1 truncate", progress?.phase === "failed" && "text-destructive")} role={progress?.phase === "failed" ? "alert" : progress?.phase === "pending" ? "status" : undefined} data-pane-band-navigation={progress?.phase}>{reason}</span></Hint>
    {band.more > 0 ? <span className="shrink-0 text-micro">+{band.more}</span> : null}
    <Elapsed since={band.since_unix_ms} className="shrink-0 font-mono text-micro" />
    {action ? <button type="button" data-pane-band-open={paneId} disabled={progress?.phase === "pending" || (progress?.phase === "failed" && !progress.retryable)} aria-busy={progress?.phase === "pending"} className="shrink-0 rounded-xs px-xs py-xxs text-micro outline-none hover:bg-background/40 focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50" onClick={() => action.kind === "child" ? actions.followRelation(paneId, action.pane_id, action.label) : actions.openPullRequestRow(action.workspace_id, action.number)}>{progress?.phase === "failed" && progress.retryable ? t("common.retry") : action.kind === "child" ? t("common.open") : t("agentSessions.openPr")}</button> : null}
  </div>;
}

function rows(rest: SnapshotRest | null, local: AgentRow[]): AgentRow[] {
  return [...local, ...(rest?.status?.remote ?? []).flatMap((remote) => remote.session?.agents ?? [])];
}

export function PaneChildrenBadge({ paneId, actions }: { paneId: string; actions: Actions }) {
  const rest = useShellStore((state) => state.rest);
  const local = useShellStore((state) => state.agents);
  const agents = rows(rest, local);
  const parent = agents.find((agent) => agent.pane_id === paneId);
  if (!parent) return null;
  const children = (parent.lineage_child_pane_ids ?? []).map((id) => agents.find((agent) => agent.pane_id === id)).filter((agent): agent is AgentRow => agent !== undefined);
  if (!children.length) return null;
  return <DescendantBadge agent={parent} descendants={children.length} childRows={children} onOpenChild={(id) => actions.followRelation(paneId, id, children.find((child) => child.pane_id === id)!.identity_label)} onUnfold={() => actions.openAgentsOverview()} returnFocus={() => document.querySelector<HTMLButtonElement>(`[data-pane-view="${paneId}"] [data-descendant-badge]`)?.focus()} />;
}
