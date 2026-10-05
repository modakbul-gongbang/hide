import { useInterfaceTranslation } from "./i18n/client";
import { useMemo } from "react";
import { LayoutDashboardIcon } from "lucide-react";
import type { Actions } from "./actions";
import { catalogWorkspaces, frontCheckout } from "./snapshot";
import { contextAgents } from "./remote";
import { sidebarBody } from "./devices";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { MainScreen } from "./MainScreen";
import { ProjectOverview } from "./ProjectOverview";
import { Button } from "./components/ui/button";
import { Dialog, DialogContent, DialogTitle } from "./components/ui/dialog";
import { Tabs, TabsList, TabsTrigger } from "./components/ui/tabs";
import { Kbd } from "./components/ui/kbd";
import { Hint } from "./components/ui/tooltip";
import { commandLabel } from "./shortcutLabels";
import { projectEntryLens } from "./navigation";

/** The same selected-device group that the Agents sidebar draws. */
export function useOverviewCount() {
  const rest = useShellStore((s) => s.rest);
  const agents = useShellStore((s) => s.agents);
  return useMemo(() => rest && sidebarBody(rest, "agents") !== "disconnected"
    ? contextAgents(rest, agents).filter((agent) => agent.group === "needs_you").length : 0, [rest, agents]);
}

export function OverviewButton({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const count = useOverviewCount();
  const open = useUiStore((s) => s.overviewOpen);
  const chord = useShellStore((s) => commandLabel("overview", s.rest?.ui_state));
  const description = count > 0 ? t("overview.needsYou", { count }) : undefined;
  return <Hint label={count > 0 ? t("overview.needsYouHint", { count }) : t("overview.title")} shortcut={chord}>
    <Button variant={open ? "secondary" : "ghost"} size="icon-sm" className="relative" aria-label={t("overview.title")} aria-description={description} aria-pressed={open} data-open-overview="true" onClick={actions.toggleOverview}>
      <LayoutDashboardIcon />
      {count > 0 ? <span aria-hidden="true" className="absolute right-xxs top-xxs size-(--size-tab-status-dot) rounded-full bg-warning" data-overview-dot="true" /> : null}
    </Button>
  </Hint>;
}

/** One Overview, as a layer over work or a page when Home is shown. */
export function OverviewPage({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const projectId = useUiStore((s) => s.overviewProjectId);
  const lens = useUiStore((s) => s.overviewLens);
  const current = useShellStore((s) => {
    const front = frontCheckout(s.rest);
    return front ? catalogWorkspaces(s.rest).find((project) => !project.is_home && project.checkouts.some((checkout) => checkout.id === front.id)) ?? null : null;
  });
  const project = useShellStore((s) => projectId ? catalogWorkspaces(s.rest).find((project) => project.id === projectId) ?? null : null);
  const scope = project ?? current;
  return <div className="flex min-h-0 flex-1 flex-col" data-overview-page="true">
    <div className="flex shrink-0 items-center gap-md border-b border-border px-lg py-sm">
      <span className="text-title font-semibold">{t("overview.title")}</span>
      <Tabs value={projectId ?? "all"} onValueChange={(value) => useUiStore.getState().setOverviewProject(value === "all" ? null : value, value === "all" ? undefined : projectEntryLens(useShellStore.getState().rest, value))} className="min-w-0 flex-1">
        <TabsList aria-label={t("overview.scopeLabel")} className="min-w-0 max-w-full">
          <TabsTrigger value="all">{t("overview.allProjects")}</TabsTrigger>
          {scope ? <Hint label={scope.label}><TabsTrigger value={scope.id} className="min-w-0"><span className="truncate">{scope.label}</span></TabsTrigger></Hint> : null}
        </TabsList>
      </Tabs>
      <Button variant="ghost" size="sm" aria-label={t("overview.close")} onClick={actions.closeOverview}><Kbd>Esc</Kbd></Button>
    </div>
    <div className="flex min-h-0 flex-1 flex-col overflow-auto">
      {projectId ? <ProjectOverview projectId={projectId} lens={lens} actions={actions} /> : <MainScreen actions={actions} />}
    </div>
  </div>;
}

export function OverviewModal({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const open = useUiStore((s) => s.overviewOpen);
  return <Dialog baseEscape open={open} onOpenChange={(next) => { if (!next) actions.closeOverview(); }}>
    {open ? <DialogContent initialFocus="container" aria-describedby={undefined} className="h-(--size-settings-sheet-h) w-(--size-settings-sheet-w)" data-overview-modal="true" returnFocusTo={() => useUiStore.getState().overviewReturnFocus}>
      <DialogTitle className="sr-only">{t("overview.title")}</DialogTitle>
      <OverviewPage actions={actions} />
    </DialogContent> : null}
  </Dialog>;
}
