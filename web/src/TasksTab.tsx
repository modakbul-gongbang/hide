import { ArrowRightIcon, ChevronRightIcon, CircleDotIcon, ExternalLinkIcon, FileTextIcon } from "lucide-react";
import { useMemo, type MouseEvent } from "react";
import type { Actions } from "./actions";
import type { IncomingTaskScope, TaskCardScope } from "./agentScope";
import { agentProductName } from "./agentAdapters";
import { HideMark } from "./AgentMark";
import { leadWord, markTone } from "./agentRow";
import { PrMark } from "./components/pr-mark";
import { StatusMark } from "./components/status-mark";
import { Button } from "./components/ui/button";
import { Hint } from "./components/ui/tooltip";
import { useInterfaceTranslation } from "./i18n/client";
import { requireInterfaceLanguage } from "./i18n/locale";
import { cn } from "./lib/utils";
import { ageWords } from "./overviewLens";
import { cardChangedAt, cardItems, cleanupFold, isMemoKey, cardLine, cardPulls, cardQuestion, restFold, taskSections, taskTitle, type CardLine, type TaskSection } from "./overviewTasks";
import { backlogOf } from "./overviewBacklog";
import { PR_LOOK } from "./prMark";
import type { BoardProject } from "./projectBoard";
import type { AgentRow } from "./snapshot";
import { toggledFold, useUiStore } from "./ui";

// The Tasks tab (PRD overview-device-tasks D-05, D-06, B6-B15): one section per
// project of the device, its task cards in the core's order, the resting and
// the merged-but-not-cleaned folded, and the dashed entries another device
// left here. A card is a button; its chips are buttons of their own.

/** The colour of the stage bar and the card's line, by what the line says (B9). */
const LINE_TONE: Record<CardLine["kind"], { bar: string; text: string }> = {
  needs_you: { bar: "bg-warning", text: "text-warning" },
  blocked: { bar: "bg-destructive", text: "text-destructive" },
  merged: { bar: "bg-pr-merged", text: "text-pr-merged" },
  pr: { bar: "bg-agent-working", text: "text-subtle-foreground" },
  working: { bar: "bg-agent-working", text: "text-agent-working" },
  done: { bar: "bg-success", text: "text-success" },
  resting: { bar: "bg-muted-foreground", text: "text-muted-foreground" },
};

const CARD = "relative flex min-w-0 flex-col gap-xxs rounded-md border bg-card px-sm py-xs text-caption";
const CHIP = "relative z-1 inline-flex min-w-0 items-center gap-xxs rounded-xs px-xxs outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring";
/** A project head's link, in the head's caption size rather than the heading's inherited one. */
const HEAD_LINK = `${CHIP} shrink-0 text-caption text-muted-foreground hover:text-foreground`;
const GRID = "grid grid-cols-[repeat(auto-fill,minmax(var(--size-panel-min),1fr))] gap-sm";

export function OverviewTasks({ projects, rows, deviceId, actions }: { projects: readonly BoardProject[]; rows: ReadonlyMap<string, AgentRow>; deviceId: string; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const filter = useUiStore((s) => s.overview.taskFilter);
  const query = useUiStore((s) => s.overview.taskQuery);
  const only = useUiStore((s) => s.overview.projectId);
  const sections = useMemo(() => taskSections(projects, rows, filter, query, only), [projects, rows, filter, query, only]);
  const filtering = filter !== "all" || query.trim() !== "";
  if (filtering && sections.length === 0) {
    return (
      <div className="flex flex-col items-center gap-sm p-xl text-caption text-muted-foreground" data-tasks-no-match="true">
        <p>{t("overviewTasks.noMatch")}</p>
        <Button variant="ghost" size="sm" onClick={() => useUiStore.getState().setOverview({ taskFilter: "all", taskQuery: "" })} data-tasks-clear-filter="true">
          {t("overviewTasks.clearFilter")}
        </Button>
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-lg px-lg py-md" data-overview-tasks="true">
      {sections.map((section) => (
        <TaskSectionView key={section.project.id} section={section} rows={rows} deviceId={deviceId} actions={actions} />
      ))}
    </div>
  );
}

function TaskSectionView({ section, rows, deviceId, actions }: { section: TaskSection; rows: ReadonlyMap<string, AgentRow>; deviceId: string; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const { project } = section;
  const open = useUiStore((s) => s.overview.open);
  const backlog = backlogOf(project).rows.length;
  const restOpen = open.includes(restFold(project.id));
  const cleanupOpen = open.includes(cleanupFold(project.id));
  const toggle = (fold: string) => useUiStore.getState().setOverview({ open: toggledFold(useUiStore.getState().overview.open, fold) });
  const empty = section.count === 0 && section.incoming.length === 0;
  return (
    <section aria-label={project.label} data-task-section={project.id}>
      <h2 className="flex min-w-0 items-baseline gap-sm pb-xs">
        <span className="min-w-0 truncate text-subhead font-semibold text-foreground">{project.label}</span>
        <span className="shrink-0 text-caption text-muted-foreground">
          {[t("overviewTasks.taskCount", { count: section.count }), section.working > 0 ? t("overviewTasks.workingCount", { count: section.working }) : null].filter(Boolean).join(" · ")}
          {section.needsYou > 0 ? <span className="text-warning"> · {t("overviewTasks.needsYouCount", { count: section.needsYou })}</span> : null}
        </span>
        <span className="flex-1" />
        {backlog > 0 ? (
          <button type="button" className={HEAD_LINK} data-section-backlog={project.id} onClick={() => { useUiStore.getState().setMainView("backlog"); useUiStore.getState().setOverview({ projectId: project.id, panel: null }); }}>
            <CircleDotIcon aria-hidden="true" className="size-(--size-icon-sm)" />
            {t("overviewTasks.backlogLink", { count: backlog })}
            <ChevronRightIcon aria-hidden="true" className="size-(--size-icon-sm)" />
          </button>
        ) : null}
        <Hint label={t("overviewTasks.openHint", { project: project.label })}>
          <button type="button" className={HEAD_LINK} data-section-open={project.id} onClick={() => actions.openProjectSessions(project.device_id, project.id)}>
            {t("overviewTasks.open")}
            <ExternalLinkIcon aria-hidden="true" className="size-(--size-icon-sm)" />
          </button>
        </Hint>
      </h2>
      {empty ? (
        <p className="flex items-center gap-xs text-caption text-muted-foreground" data-section-empty={project.id}>
          {t("overviewTasks.none")} ·
          <button type="button" className={cn(CHIP, "text-foreground")} onClick={() => actions.openStartPanel()} data-section-new-agent={project.id}>
            {t("overviewTasks.newAgent")}
          </button>
        </p>
      ) : (
        <div className={GRID}>
          {section.open.map((card) => <TaskCardView key={card.root} card={card} project={project} rows={rows} deviceId={deviceId} actions={actions} />)}
          {section.incoming.map((entry) => <IncomingCardView key={entry.root} entry={entry} rows={rows} />)}
          {cleanupOpen ? section.cleanup.map((card) => <TaskCardView key={card.root} card={card} project={project} rows={rows} deviceId={deviceId} actions={actions} />) : null}
          {restOpen ? section.resting.map((card) => <TaskCardView key={card.root} card={card} project={project} rows={rows} deviceId={deviceId} actions={actions} />) : null}
          {section.cleanup.length > 0 || section.resting.length > 0 ? (
            <span className="flex flex-wrap items-start gap-xs">
              {section.cleanup.length > 0 ? <FoldChip label={cleanupOpen ? t("overviewTasks.fold") : t("overviewTasks.cleanup", { count: section.cleanup.length })} open={cleanupOpen} tone="text-pr-merged" onToggle={() => toggle(cleanupFold(project.id))} fold={cleanupFold(project.id)} /> : null}
              {section.resting.length > 0 ? <FoldChip label={restOpen ? t("overviewTasks.fold") : t("overviewTasks.resting", { count: section.resting.length })} open={restOpen} tone="text-muted-foreground" onToggle={() => toggle(restFold(project.id))} fold={restFold(project.id)} /> : null}
            </span>
          ) : null}
        </div>
      )}
    </section>
  );
}

function FoldChip({ label, open, tone, onToggle, fold }: { label: string; open: boolean; tone: string; onToggle: () => void; fold: string }) {
  return (
    <button type="button" aria-expanded={open} className={cn("inline-flex items-center gap-xxs rounded-sm border border-dashed border-border px-sm py-xxs text-caption outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring", tone)} onClick={onToggle} data-task-fold={fold}>
      {label}
      {open ? null : <ChevronRightIcon aria-hidden="true" className="size-(--size-icon-sm)" />}
    </button>
  );
}

/** The card's line (B9) in words. */
function lineWords(line: CardLine, t: ReturnType<typeof useInterfaceTranslation>["t"]): string {
  switch (line.kind) {
    case "needs_you": {
      const agent = agentProductName(line.agent) ?? line.agent;
      return line.verb ? t("overviewTasks.line.waiting", { verb: t(`agentSessions.verb.${line.verb}`), agent }) : t("overviewTasks.line.waitingAgent", { agent });
    }
    case "blocked":
      return line.what ? t("overviewTasks.line.blocked", { what: line.what }) : t("overviewTasks.state.blocked");
    case "merged":
      return t(line.cleanup ? "overviewTasks.line.mergedCleanup" : "overviewTasks.line.merged");
    case "pr":
      return t("overviewTasks.line.pr", { number: line.number, word: t(PR_LOOK[line.state].word) });
    case "working":
      return t("overviewTasks.line.working", { count: Math.max(1, line.agents) });
    case "done": {
      // The root's own word, as its row leads with it (status-marks B6, B20).
      const word = line.status ? leadWord(t, { status_code: line.status }) : null;
      return word ? t("overviewTasks.line.done", { word }) : t("overviewTasks.line.unseen");
    }
    case "resting":
      return t("overviewTasks.line.resting");
  }
}

/** `작업 → PR → 검토 → 머지` filled up to the card's stage (B9). */
export function StageBar({ stage, tone }: { stage: 1 | 2 | 3 | 4; tone: string }) {
  const { t } = useInterfaceTranslation();
  return (
    <span role="img" aria-label={t("overviewTasks.stage", { stage })} className="flex w-(--size-control-lg) shrink-0 gap-px" data-stage-bar={stage}>
      {[1, 2, 3, 4].map((step) => <span key={step} className={cn("h-(--size-hairline) flex-1 rounded-full", step <= stage ? tone : "bg-muted")} />)}
    </span>
  );
}

function TaskCardView({ card, project, rows, deviceId, actions }: { card: TaskCardScope; project: import("./snapshot").Workspace; rows: ReadonlyMap<string, AgentRow>; deviceId: string; actions: Actions }) {
  const { t, i18n } = useInterfaceTranslation();
  const selected = useUiStore((s) => s.overview.panel?.kind === "task" && s.overview.panel.root === card.root);
  const root = rows.get(card.root);
  const title = taskTitle(card, rows);
  const line = cardLine(card, rows);
  const tone = LINE_TONE[line.kind];
  const words = lineWords(line, t);
  const question = cardQuestion(card, rows);
  const pulls = cardPulls(card, rows);
  const items = cardItems(card, project);
  const children = card.members.slice(1).map((pane) => rows.get(pane)).filter((row): row is AgentRow => row !== undefined);
  const changed = cardChangedAt(card, rows);
  const language = requireInterfaceLanguage(i18n.language);
  const stop = (event: MouseEvent) => event.stopPropagation();
  const select = () => useUiStore.getState().setOverview({ panel: { kind: "task", root: card.root } });
  return (
    <div
      className={cn(CARD, card.state === "needs_you" ? "border-warning" : "border-border", selected && "ring-2 ring-ring")}
      data-task-card={card.root}
      data-task-state={card.state}
    >
      <button
        type="button"
        aria-label={[title, t(`overviewTasks.state.${card.state}`), question, words].filter(Boolean).join(", ")}
        aria-pressed={selected}
        className="absolute inset-0 rounded-md outline-none hover:bg-accent/40 focus-visible:ring-2 focus-visible:ring-ring"
        onClick={select}
        data-task-card-open={card.root}
      />
      <span className="pointer-events-none flex min-w-0 items-center gap-xs">
        {root ? <StatusMark mark={root.mark} className={markTone(root)} /> : null}
        {root?.hide_ai ? <HideMark className="size-(--size-agent-badge-compact)" /> : null}
        <span className="min-w-0 flex-1 truncate font-medium text-foreground" title={title} data-task-title="true">{title}</span>
        {changed !== null ? <span className="shrink-0 font-mono text-micro text-muted-foreground">{ageWords(language, Date.now() - changed, t)}</span> : null}
      </span>
      {question ? <span className="pointer-events-none line-clamp-2 text-warning" data-task-question="true">{question}</span> : null}
      {children.length > 0 || pulls.length > 0 || items.length > 0 || card.out_projects?.length || card.out_devices?.length ? (
        <span className="flex min-w-0 flex-wrap items-center gap-xs">
          {children.length > 0 ? (
            <span className="pointer-events-none inline-flex items-center gap-xxs text-muted-foreground" data-task-children={children.length}>
              {children.slice(0, 8).map((child) => <StatusMark key={child.pane_id} mark={child.mark} className={cn(markTone(child), "size-(--size-icon-sm)")} />)}
              {t("overviewTasks.children", { count: children.length })}
            </span>
          ) : null}
          {pulls.map((pull) => (
            <button key={pull.url} type="button" className={CHIP} onClick={(event) => { stop(event); actions.openSessionPullRequest({ workspace_id: project.id, number: pull.number, url: pull.url }); }} data-task-pr={pull.number}>
              <PrMark state={pull.state} number={pull.number} />
            </button>
          ))}
          {items.map((item) => (
            <button key={item.key} type="button" className={cn(CHIP, "font-mono text-subtle-foreground")} onClick={(event) => { stop(event); useUiStore.getState().setOverview({ panel: { kind: "item", workspaceId: project.id, taskKey: item.key } }); useUiStore.getState().setMainView("backlog"); }} data-task-item={item.token}>
              {isMemoKey(item.key) ? <FileTextIcon aria-hidden="true" className="size-(--size-icon-sm)" /> : <CircleDotIcon aria-hidden="true" className="size-(--size-icon-sm)" />}
              {item.token}
            </button>
          ))}
          {card.out_projects?.map((out) => (
            <Hint key={out.project_id} label={t("overviewTasks.toProject", { project: out.label })}>
              <button type="button" className={cn(CHIP, "rounded-xs border border-border text-subtle-foreground")} onClick={(event) => { stop(event); useUiStore.getState().setOverview({ projectId: out.project_id, panel: null }); }} data-task-out-project={out.project_id}>
                <ArrowRightIcon aria-hidden="true" className="size-(--size-icon-sm)" />
                <span className="truncate">{out.label}</span>
              </button>
            </Hint>
          ))}
          {card.out_devices?.map((out) => (
            <Hint key={out.device_id} label={t("overviewTasks.toDevice", { device: out.label ?? out.device_id })}>
              <button type="button" className={cn(CHIP, "rounded-xs border border-border text-subtle-foreground")} onClick={(event) => { stop(event); useUiStore.getState().setOverview({ deviceId: out.device_id, panel: { kind: "task", root: card.root, from: deviceId } }); }} data-task-out-device={out.device_id}>
                <ArrowRightIcon aria-hidden="true" className="size-(--size-icon-sm)" />
                <span className="truncate">{out.label ?? out.device_id}</span>
              </button>
            </Hint>
          ))}
        </span>
      ) : null}
      <span className="pointer-events-none flex min-w-0 items-center gap-sm">
        {card.stage ? <StageBar stage={card.stage} tone={tone.bar} /> : null}
        <span className={cn("min-w-0 truncate", tone.text)} data-task-line={line.kind}>{words}</span>
      </span>
    </div>
  );
}

/** Work another device's task delegated here (B14): dashed, its child's mark and title, never counted. */
function IncomingCardView({ entry, rows }: { entry: IncomingTaskScope; rows: ReadonlyMap<string, AgentRow> }) {
  const { t } = useInterfaceTranslation();
  const selected = useUiStore((s) => s.overview.panel?.kind === "task" && s.overview.panel.root === entry.root);
  const children = entry.pane_ids.map((pane) => rows.get(pane)).filter((row): row is AgentRow => row !== undefined);
  const from = t("overviewTasks.incoming", { device: entry.root_device_label ?? entry.root_device_id });
  return (
    <div className={cn(CARD, "border-dashed border-border bg-transparent", selected && "ring-2 ring-ring")} data-task-incoming={entry.root}>
      <button
        type="button"
        aria-label={[from, ...children.map((child) => child.identity_label)].join(", ")}
        aria-pressed={selected}
        className="absolute inset-0 rounded-md outline-none hover:bg-accent/40 focus-visible:ring-2 focus-visible:ring-ring"
        onClick={() => useUiStore.getState().setOverview({ panel: { kind: "task", root: entry.root, from: entry.root_device_id } })}
      />
      <span className="pointer-events-none text-muted-foreground">← {from}</span>
      {children.map((child) => (
        <span key={child.pane_id} className="pointer-events-none flex min-w-0 items-center gap-xs">
          <StatusMark mark={child.mark} className={markTone(child)} />
          <span className="min-w-0 truncate text-foreground">{child.identity_label}</span>
        </span>
      ))}
    </div>
  );
}
