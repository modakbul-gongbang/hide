import { TriangleAlertIcon } from "lucide-react";
import { useMemo } from "react";
import type { Actions } from "./actions";
import { Hint } from "./components/ui/tooltip";
import { frontDeviceId } from "./devices";
import { useInterfaceTranslation } from "./i18n/client";
import { boardProjects } from "./navigation";
import { PrList } from "./PrList";
import { prListModel, type PrListModel, type PrScopeProject } from "./prWork";
import { useRequestViewDemand } from "./requestViewDemand";
import { catalogWorkspaces, frontCheckout, type AgentRow, type SnapshotRest, type Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { workspaceViewOf } from "./workspace";

// The PRs tool (PRD sidebar-task-lens D-17, D-31, D-45, B26-B29): the front
// checkout's project's pull requests, or at Home every project's of the
// device, in the PRs lens's groups, rows, order and folds, behind one scope
// chip. Its Needs You rows are the rail icon's number.

type PrsScope = {
  /** The front checkout's project; null at Home. */
  project: Workspace | null;
  front: ReturnType<typeof frontCheckout>;
  /** The checkout the chip narrows to, or null for all of them. */
  only: string | null;
  model: PrListModel;
};

function prsScope(rest: SnapshotRest | null, agents: AgentRow[], now: number): PrsScope {
  const deviceId = frontDeviceId(rest);
  const front = frontCheckout(rest);
  const owner = catalogWorkspaces(rest).find((project) => project.device_id === deviceId && project.checkouts.some((checkout) => checkout.id === front?.id));
  const project = owner && !owner.is_home ? owner : null;
  const only = project && front && workspaceViewOf(rest)?.prs?.only_checkout ? front.id : null;
  const projects: PrScopeProject[] = boardProjects(rest, agents, deviceId).filter((item) => !project || item.workspace.id === project.id);
  return { project, front, only, model: prListModel(projects, only, now) };
}

/** The rail icon's number (B29): the PRs tool's Needs You rows, the folded ones not counted, for the whole project. */
export function usePrsNeedsYou(): number {
  const rest = useShellStore((state) => state.rest);
  const agents = useShellStore((state) => state.agents);
  return useMemo(() => {
    const deviceId = frontDeviceId(rest);
    const front = frontCheckout(rest);
    const owner = catalogWorkspaces(rest).find((project) => project.device_id === deviceId && project.checkouts.some((checkout) => checkout.id === front?.id));
    const project = owner && !owner.is_home ? owner : null;
    const projects = boardProjects(rest, agents, deviceId).filter((item) => !project || item.workspace.id === project.id);
    return prListModel(projects, null, Date.now()).needsYou.length;
  }, [rest, agents]);
}

export function PrsPanel({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const rest = useShellStore((state) => state.rest);
  const agents = useShellStore((state) => state.agents);
  const live = useShellStore((state) => state.connection === "live");
  const { project, front, only, model } = useMemo(() => prsScope(rest, agents, Date.now()), [rest, agents]);
  // The board's GitHub reads follow what is on screen.
  useRequestViewDemand(actions, live);
  const stale = model.reading.state === "stale" ? model.reading.minutes : undefined;
  const staleText = stale === undefined ? null : stale === null ? t("work.lens.staleNow") : t("work.lens.stale", { minutes: stale });
  if (model.projects === 0 && model.reading.state !== "blocked") {
    return <p className="px-md py-sm text-caption text-muted-foreground" data-prs-view="no-remote">{t("prs.noRemote")}</p>;
  }
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col" data-prs-panel="true">
      {model.reading.state === "blocked" ? null : (
        <header className="flex shrink-0 items-center gap-xs px-md py-sm" data-prs-header="true">
          {project && front ? (
            <div className="flex min-w-0 flex-1 gap-xs" data-prs-scope="true">
              <button type="button" aria-pressed={only === null} className="rounded-xs px-xs py-xxs text-micro outline-none aria-pressed:bg-secondary focus-visible:ring-1 focus-visible:ring-ring" onClick={() => actions.setPrsScope(false)}>{t("agentSessions.allCheckouts")}</button>
              <button type="button" aria-pressed={only !== null} className="min-w-0 truncate rounded-xs px-xs py-xxs text-micro outline-none aria-pressed:bg-secondary focus-visible:ring-1 focus-visible:ring-ring" title={front.label} onClick={() => actions.setPrsScope(only === null)}>{t("agentSessions.onlyCheckout", { checkout: front.branch ?? front.label })}</button>
            </div>
          ) : <span className="flex-1" />}
          {staleText ? (
            <Hint label={staleText}>
              <span role="img" aria-label={staleText} className="inline-flex shrink-0 text-warning" data-prs-stale="true"><TriangleAlertIcon aria-hidden="true" className="size-(--size-icon-sm)" /></span>
            </Hint>
          ) : null}
          {model.reading.state === "reading" && model.groups.length === 0 ? null : <span className="shrink-0 text-micro text-muted-foreground tabular-nums" data-prs-total={model.total}>{t("prs.open", { count: model.groups.filter((group) => group.group !== "merged").reduce((sum, group) => sum + group.entries.length + group.team.length + group.old.length, 0) })}</span>}
        </header>
      )}
      <PrList model={model} actions={actions} onlyCheckout={only !== null} showProject={project === null} onAllCheckouts={() => actions.setPrsScope(false)} />
    </div>
  );
}
