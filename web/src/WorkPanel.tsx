import { TriangleAlertIcon } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { Actions } from "./actions";
import { AgentSessions } from "./AgentSessions";
import { Hint } from "./components/ui/tooltip";
import { frontDeviceId } from "./devices";
import { useInterfaceTranslation } from "./i18n/client";
import { cn } from "./lib/utils";
import { boardProjects } from "./navigation";
import { PrList } from "./PrList";
import { prListModel, type PrScopeProject } from "./prWork";
import { sessionsModel } from "./sessionPanel";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { workspaceViewOf, type WorkLens } from "./workspace";

// The Work tool (PRD work-tab-pr-lens B1-B5, B41-B43): one panel holding the
// project's Sessions and its PRs behind one switch and one scope chip. It does
// not depend on where the shell puts it, so the Tools column can host it
// today and another frame later; its count is `workCount.ts`.

const NO_WORK = { lens: "sessions", only_checkout: false } as const;

export function WorkPanel({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const rest = useShellStore((state) => state.rest);
  const agents = useShellStore((state) => state.agents);
  const recordOpen = useUiStore((state) => state.sessionWorkTarget !== null);
  const work = workspaceViewOf(rest)?.work ?? NO_WORK;
  const scope = useMemo(() => sessionsModel(rest, agents, null), [rest, agents]);
  const { project, front } = scope;
  // The chip narrows to the front checkout; Home has no checkout to narrow to.
  const only = work.only_checkout && project !== null && front !== null ? front.id : null;
  const projects = useMemo<PrScopeProject[]>(
    () => boardProjects(rest, agents, frontDeviceId(rest)).filter((item) => !project || item.workspace.id === project.id),
    [rest, agents, project],
  );
  const [now] = useState(() => Date.now());
  const prs = useMemo(() => prListModel(projects, only, Date.now()), [projects, only, now]);
  const sessions = useMemo(() => sessionsModel(rest, agents, only), [rest, agents, only]);
  const sessionsNeed = sessions.scope?.sessions.groups.find((group) => group.group === "needs_you")?.members.length ?? 0;
  // A project with no GitHub remote has only Sessions, whatever it was left on (B41).
  const hasPrs = prs.projects > 0;
  const lens: WorkLens = hasPrs ? work.lens : "sessions";
  const label = project?.label ?? t("overview.allProjects");
  const hidden = recordOpen && lens === "sessions";
  const live = useShellStore((state) => state.connection === "live");
  // PRs shares the one visible-board demand Sessions reports (AgentSessions does it for its own lens).
  useEffect(() => {
    if (!live || lens !== "prs") return;
    const report = () => actions.observeRequestView(document.visibilityState === "visible");
    report();
    document.addEventListener("visibilitychange", report);
    return () => {
      document.removeEventListener("visibilitychange", report);
      actions.observeRequestView(false);
    };
  }, [actions, live, lens]);
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col" data-work-panel={lens}>
      {hidden ? null : (
        <header className="flex shrink-0 flex-col gap-sm border-b border-border px-md py-sm" data-work-header="true">
          <span className="truncate text-caption font-medium" title={label}>{label}</span>
          <div className="flex min-w-0 flex-wrap items-center gap-xs">
            {hasPrs ? <LensSwitch lens={lens} sessionsNeed={sessionsNeed} prsNeed={prs.needsYou.length} stale={prs.reading.state === "stale" ? prs.reading.minutes : undefined} actions={actions} /> : null}
          </div>
          {project && front ? (
            <div className="flex min-w-0 gap-xs" data-work-scope="true">
              <button type="button" aria-pressed={only === null} className="rounded-xs bg-secondary px-xs py-xxs text-micro outline-none focus-visible:ring-1 focus-visible:ring-ring aria-[pressed=false]:bg-transparent" onClick={() => actions.setWorkScope(false)}>{t("agentSessions.allCheckouts")}</button>
              <button type="button" aria-pressed={only !== null} className="min-w-0 truncate rounded-xs px-xs py-xxs text-micro outline-none aria-pressed:bg-secondary focus-visible:ring-1 focus-visible:ring-ring" title={front.label} onClick={() => actions.setWorkScope(only === null)}>{t("agentSessions.onlyCheckout", { checkout: front.label })}</button>
            </div>
          ) : null}
        </header>
      )}
      {lens === "prs" ? (
        <PrList model={prs} actions={actions} onlyCheckout={only !== null} showProject={project === null} onAllCheckouts={() => actions.setWorkScope(false)} now={now} />
      ) : (
        <AgentSessions actions={actions} onlyCheckout={only} />
      )}
    </div>
  );
}

/**
 * `Sessions | PRs` with no numbers: a warning dot where that basis has
 * something for the operator, named for assistive tools as `PRs · Needs You 4`
 * (B3), and ⚠ on PRs while its last read failed (B43).
 */
function LensSwitch({ lens, sessionsNeed, prsNeed, stale, actions }: { lens: WorkLens; sessionsNeed: number; prsNeed: number; stale: number | null | undefined; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const entries: { id: WorkLens; need: number }[] = [{ id: "sessions", need: sessionsNeed }, { id: "prs", need: prsNeed }];
  return (
    <div role="group" aria-label={t("work.lens.aria")} className="flex min-w-0 flex-1 rounded-sm bg-muted p-xxs" data-work-switch="true">
      {entries.map(({ id, need }) => {
        const name = t(`work.lens.${id}`);
        const label = need > 0 ? t("work.lens.needsYou", { lens: name, count: need }) : name;
        const warn = id === "prs" && stale !== undefined;
        const staleText = warn ? (stale === null ? t("work.lens.staleNow") : t("work.lens.stale", { minutes: stale })) : null;
        return (
          <button key={id} type="button" aria-pressed={lens === id} aria-label={staleText ? `${label} · ${staleText}` : label} data-work-lens={id}
            className={cn("flex min-w-0 flex-1 items-center justify-center gap-xs rounded-xs px-sm py-xxs text-caption outline-none focus-visible:ring-1 focus-visible:ring-ring", lens === id ? "bg-background font-medium text-foreground" : "text-subtle-foreground hover:text-foreground")}
            onClick={() => lens !== id && actions.setWorkLens(id)}>
            <span className="truncate">{name}</span>
            {need > 0 ? <Hint label={label}><span role="img" aria-label={label} className="size-(--size-status-mark) shrink-0 rounded-full bg-warning" data-work-dot={id} /></Hint> : null}
            {staleText ? <Hint label={staleText}><span role="img" aria-label={staleText} className="inline-flex text-warning" data-work-stale="true"><TriangleAlertIcon aria-hidden="true" className="size-(--size-icon-sm)" /></span></Hint> : null}
          </button>
        );
      })}
    </div>
  );
}
