import { useEffect, useMemo } from "react";
import { CirclePauseIcon, MessageSquareIcon, PauseIcon, PlayIcon, PlusIcon } from "lucide-react";
import type { Actions } from "../actions";
import { Elapsed } from "../components/elapsed";
import { Button } from "../components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../components/ui/select";
import { Tabs, TabsList, TabsTrigger } from "../components/ui/tabs";
import { Hint } from "../components/ui/tooltip";
import { useInterfaceTranslation } from "../i18n/client";
import { cn } from "../lib/utils";
import { useShellStore } from "../store";
import { FACTORY_TABS, useUiStore, type FactoryPlace, type FactoryTab } from "../ui";
import { CreateSheet } from "./CreateSheet";
import { FactoryBoard } from "./FactoryBoard";
import { FactoryGraph } from "./FactoryGraph";
import { FactorySettings } from "./FactorySettings";
import { COLUMN_LABEL } from "./labels";
import type { Column, FactorySummary, FactoryView } from "./model";
import { MyTurn, Refusal } from "./MyTurn";
import { useFactoryRequest } from "./request";
import { TaskPage } from "./TaskPage";
import { outsideRead, shownFactories, shownFlow } from "./view";

const TAB_LABEL = { turn: "factory.tab.turn", board: "factory.tab.board", graph: "factory.tab.graph", settings: "factory.tab.settings" } as const;

const FLOW_CELLS: readonly { column: Column; count: keyof ReturnType<typeof shownFlow> }[] = [
  { column: "before", count: "before" },
  { column: "moving", count: "moving" },
  { column: "stuck", count: "stuck" },
  { column: "done", count: "done_today" },
];

/**
 * The Factory screen (PRD software-factory-ui D-03, B1, B7, B21): one job per
 * screen, 내 차례 · 보드 · 그래프 · 설정 under a header of the project filter,
 * the flow bar and 비서에게 묻기, or one Task's page over them. Everything it
 * shows is the engine's summary; before the first one arrives it draws its
 * frame, and with no Factory it offers only + Factory 만들기.
 */
export function FactoryScreen({ actions }: { actions: Actions }) {
  const place = useUiStore((s) => (s.screen?.kind === "factory" ? s.screen.place : null));
  const summary = useShellStore((s) => s.factory?.summary ?? null);
  if (!place) return null;
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-background" data-factory-screen={summary === null ? "loading" : "ready"}>
      {summary === null ? <FactorySkeleton /> : <FactoryBody summary={summary} place={place} actions={actions} />}
      <CreateSheet actions={actions} />
    </div>
  );
}

function FactoryBody({ summary, place, actions }: { summary: FactorySummary; place: FactoryPlace; actions: Actions }) {
  const open = summary.factories.filter((view) => !view.closed);
  // A filter naming a Factory that closed or went away shows them all.
  const factory = place.factory !== null && open.some((view) => view.id === place.factory) ? place.factory : null;
  const factories = useMemo(() => shownFactories(summary, factory), [summary, factory]);
  if (open.length === 0) return <NoFactory />;
  if (place.task) return <TaskPage summary={summary} place={place} actions={actions} />;
  return (
    <>
      <FactoryHeader factories={open} factory={factory} actions={actions} />
      <FlowBar factories={factories} />
      <div className="flex shrink-0 items-center px-lg pb-sm">
        <Tabs value={place.tab} onValueChange={(value) => useUiStore.getState().setFactoryPlace({ tab: value as FactoryTab })}>
          <TabsList data-factory-tabs="true">
            {FACTORY_TABS.map((tab) => (
              <TabsTrigger key={tab} value={tab} data-factory-tab={tab}>
                <TabLabel tab={tab} count={summary.my_turn} />
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
      </div>
      <div className="flex min-h-0 flex-1 flex-col overflow-auto" data-factory-body={place.tab}>
        {place.tab === "turn" ? <MyTurn summary={summary} factory={factory} actions={actions} /> : null}
        {place.tab === "board" ? <FactoryBoard factories={factories} place={place} actions={actions} inbox={summary.inbox} /> : null}
        {place.tab === "graph" ? <FactoryGraph factories={factories} filtered={factory !== null} /> : null}
        {place.tab === "settings" ? <FactorySettings factories={factories} filtered={factory !== null} summary={summary} actions={actions} /> : null}
      </div>
    </>
  );
}

/** 내 차례 carries the one person-facing number, the same the sidebar's Factory row shows (B12). */
function TabLabel({ tab, count }: { tab: FactoryTab; count: number }) {
  const { t } = useInterfaceTranslation();
  return (
    <span className="flex items-center gap-xs">
      {t(TAB_LABEL[tab])}
      {tab === "turn" && count > 0 ? (
        <span className="text-warning" data-factory-turn-count={count}>
          {count}
        </span>
      ) : null}
    </span>
  );
}

function FactoryHeader({ factories, factory, actions }: { factories: FactoryView[]; factory: string | null; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const view = factories.find((other) => other.id === factory) ?? null;
  return (
    <div className="flex shrink-0 items-center gap-md px-lg pt-lg pb-sm" data-factory-header="true">
      <h1 className="text-title font-semibold">{t("factory.title")}</h1>
      <Select value={factory ?? "all"} onValueChange={(value) => useUiStore.getState().setFactoryPlace({ factory: value === "all" ? null : value, column: null })}>
        <SelectTrigger size="sm" className="w-auto max-w-2/5" aria-label={t("factory.projectFilter")} data-factory-project-filter="true">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="all">{t("factory.allProjects")}</SelectItem>
          {factories.map((view) => (
            <SelectItem key={view.id} value={view.id} data-factory-project-option={view.id}>
              {view.project_name}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {view?.paused ? (
        <span className="flex shrink-0 items-center gap-xxs rounded-full bg-muted px-sm py-xxs text-caption text-subtle-foreground" data-factory-paused-chip="true">
          <CirclePauseIcon aria-hidden="true" className="size-(--size-icon-sm)" />
          {t("factory.pausedChip")}
        </span>
      ) : null}
      <span className="flex-1" />
      {view ? <PauseFactory view={view} actions={actions} /> : null}
      <Button variant="ghost" size="sm" data-factory-create-open="true" onClick={() => useUiStore.getState().setFactoryPlace({ create: true })}>
        <PlusIcon />
        {t("factory.create.open")}
      </Button>
      <Button variant="ghost" size="sm" data-factory-ask-secretary="true" onClick={() => actions.openSecretary()}>
        <MessageSquareIcon />
        {t("factory.secretary.ask")}
      </Button>
    </div>
  );
}

/**
 * Pauses or resumes the one Factory the header shows (D-48): a paused Factory
 * starts nothing, asks Factory AI nothing and keeps its workers asleep.
 */
function PauseFactory({ view, actions }: { view: FactoryView; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const request = useFactoryRequest(actions);
  const sending = request.state.phase === "sending";
  return (
    <>
      <Refusal state={request.state} />
      {view.paused ? (
        <Button variant="outline" size="sm" disabled={sending} data-factory-pause="resume" onClick={() => request.send({ verb: "resume_factory", project: view.project })}>
          <PlayIcon />
          {t("factory.resume")}
        </Button>
      ) : (
        <Button variant="ghost" size="sm" disabled={sending} data-factory-pause="pause" onClick={() => request.send({ verb: "pause_factory", project: view.project })}>
          <PauseIcon />
          {t("factory.pause")}
        </Button>
      )}
    </>
  );
}

/**
 * The flow bar (B7, B12, B20): the Tasks moving, never a person's-turn cell;
 * a cell opens the board filtered to it. The last outside read sits at its
 * end and turns the warning colour after three failed reads in a row.
 */
function FlowBar({ factories }: { factories: FactoryView[] }) {
  const { t } = useInterfaceTranslation();
  const flow = shownFlow(factories);
  const read = outsideRead(factories);
  // A paused Factory's workers are asleep, so its moving cell says so.
  const asleep = factories.length > 0 && factories.every((view) => view.paused);
  return (
    <div className="mx-lg flex shrink-0 items-stretch gap-xxs rounded-md bg-muted p-xxs" data-factory-flow="true">
      {FLOW_CELLS.map((cell) => (
        <button
          key={cell.column}
          type="button"
          className="flex flex-1 items-baseline gap-sm rounded-sm px-md py-xs text-left text-body text-subtle-foreground outline-none hover:bg-background focus-visible:ring-1 focus-visible:ring-ring"
          data-factory-flow-cell={cell.column}
          onClick={() => useUiStore.getState().setFactoryPlace({ tab: "board", column: cell.column, cancelled: false })}
        >
          <span className="truncate">{cell.column === "done" ? t("factory.flow.doneToday") : cell.column === "moving" && asleep ? t("factory.flow.asleep") : t(COLUMN_LABEL[cell.column])}</span>
          <span className="font-semibold text-foreground" data-factory-flow-count={flow[cell.count]}>
            {flow[cell.count]}
          </span>
        </button>
      ))}
      {read ? (
        <Hint label={read.stale ? t("factory.flow.staleHint") : t("factory.flow.readHint")}>
          <span
            className={cn("flex shrink-0 items-center gap-xxs self-center px-md text-caption", read.stale ? "text-warning" : "text-muted-foreground")}
            data-factory-outside-read={read.stale ? "stale" : "fresh"}
            tabIndex={0}
          >
            {t("factory.flow.read")}
            {read.at === null ? <span>{t("factory.flow.notRead")}</span> : <Elapsed since={read.at} />}
          </span>
        </Hint>
      ) : null}
    </div>
  );
}

/** Before the core's first summary: the screen's frame, never an empty state that might be wrong (B21). */
function FactorySkeleton() {
  const { t } = useInterfaceTranslation();
  return (
    <div className="flex flex-col gap-md px-lg pt-lg" aria-busy="true" aria-label={t("factory.loading")} data-factory-skeleton="true">
      <div className="h-(--size-control) w-1/5 rounded-sm bg-muted" />
      <div className="h-(--size-control) rounded-md bg-muted" />
      <div className="h-(--size-control-sm) w-2/5 rounded-sm bg-muted" />
      <div className="h-(--size-control-lg) rounded-md bg-muted" />
      <div className="h-(--size-control-lg) rounded-md bg-muted" />
    </div>
  );
}

/** No Factory on this machine (B1): only the way to make one. */
function NoFactory() {
  const { t } = useInterfaceTranslation();
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-md" data-factory-empty="none">
      <Button data-factory-create-open="true" onClick={() => useUiStore.getState().setFactoryPlace({ create: true })}>
        <PlusIcon />
        {t("factory.create.open")}
      </Button>
    </div>
  );
}

/** Keeps the core's Task page open while the page shows it, and closes it when the page goes (B18). */
export function useTaskDetail(actions: Actions, task: FactoryPlace["task"]) {
  const factory = task?.factory ?? null;
  const id = task?.task ?? null;
  useEffect(() => {
    if (factory === null || id === null) return undefined;
    actions.factoryTaskOpen(factory, id);
    return () => actions.factoryTaskClose();
  }, [actions, factory, id]);
  return useShellStore((s) => (s.factoryTask && s.factoryTask.factory === factory && s.factoryTask.task === id ? s.factoryTask : null));
}
