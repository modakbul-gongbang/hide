import { AlertCircleIcon, ChevronDownIcon, ChevronRightIcon, FileIcon, LoaderCircleIcon, MonitorSmartphoneIcon, MoreHorizontalIcon, SettingsIcon, SparklesIcon, Trash2Icon, Undo2Icon, UserIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { Actions } from "./actions";
import { AgentLogo } from "./components/agent-logo";
import { PrMark } from "./components/pr-mark";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "./components/ui/alert-dialog";
import { Button } from "./components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "./components/ui/dropdown-menu";
import { Input } from "./components/ui/input";
import { Switch } from "./components/ui/switch";
import { useInterfaceTranslation } from "./i18n/client";
import type { MessageKey } from "./i18n/catalogs";
import { requireInterfaceLanguage } from "./i18n/locale";
import { cn } from "./lib/utils";
import { cardName, memoryCards, memoryTime, sourceTime, visibleCards } from "./memoryList";
import { catalogWorkspaces, frontCheckout, type MemoryCard, type MemoryCardKind, type MemoryQuestion, type MemoryRow, type ProjectMemory } from "./snapshot";
import { useShellStore } from "./store";

const KIND: Record<MemoryCardKind, MessageKey> = { rule: "memory.kind.rule", decision: "memory.kind.decision", lesson: "memory.kind.lesson", fact: "memory.kind.fact" };

/**
 * The Tools column's Memory tab (PRD project-memory-v2 D-17, B19-B32): the
 * front checkout's Project, which the Tools column opens (`useMemoryProject`);
 * until the core answers for that Project, the tab reads.
 */
export function MemoryTab({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const projectId = useShellStore((state) => frontCheckout(state.rest)?.workspace_id ?? null);
  const memory = useShellStore((state) => state.memory?.project ?? null);
  const opened = memory && memory.workspace_id === projectId ? memory : null;
  if (!projectId) return <p className="p-lg text-caption text-muted-foreground">{t("history.openWorkspace")}</p>;
  if (!opened || (opened.loading && opened.rows.length === 0 && !opened.unavailable)) {
    return (
      <p role="status" className="p-lg text-caption text-muted-foreground">
        {t("memory.reading")}
      </p>
    );
  }
  return <MemoryBody project={opened} actions={actions} />;
}

/**
 * Opens the front checkout's Project's Memory while the Tools column shows,
 * whatever its tool, so the Memory tab's dot says when the Project has
 * something to ask (B28). The core keeps one opened Project per daemon: the
 * column names its Project when it shows, when the front Project changes, and
 * when its window comes forward after another window named another one, so
 * two windows never take it from each other in a loop.
 */
export function useMemoryProject(actions: Actions) {
  const projectId = useShellStore((state) => frontCheckout(state.rest)?.workspace_id ?? null);
  const live = useShellStore((state) => state.connection === "live");
  const opened = useShellStore((state) => projectId !== null && state.memory?.project?.workspace_id === projectId);
  const openedRef = useRef(opened);
  openedRef.current = opened;
  useEffect(() => {
    if (!live || !projectId) return;
    actions.memoryOpen(projectId);
    const reclaim = () => {
      if (!openedRef.current) actions.memoryOpen(projectId);
    };
    window.addEventListener("focus", reclaim);
    return () => window.removeEventListener("focus", reclaim);
  }, [actions, live, projectId]);
}

function MemoryBody({ project, actions }: { project: ProjectMemory; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const id = project.workspace_id;
  const busy = project.pending_action !== null;
  const [deleting, setDeleting] = useState(false);
  const label = useShellStore((state) => catalogWorkspaces(state.rest).find((row) => row.id === id)?.label ?? id);
  const unavailable = project.unavailable;
  if (unavailable?.reason === "remote_device") {
    return (
      <div className="flex flex-col gap-md overflow-auto p-lg" data-memory="remote">
        <Header project={project} actions={actions} busy={busy} />
        <p className="flex gap-xs break-keep text-caption text-subtle-foreground">
          <MonitorSmartphoneIcon aria-hidden="true" className="mt-xxs size-(--size-icon-sm) shrink-0" />
          {t("memory.remoteDevice", { device: unavailable.device_name })}
        </p>
      </div>
    );
  }
  if (unavailable) {
    return (
      <div className="flex flex-col items-start gap-sm p-lg" data-memory={unavailable.reason}>
        <p className="break-keep text-caption text-subtle-foreground">{t(unavailable.reason === "unregistered" ? "memory.unregistered" : "memory.unreadable")}</p>
        {unavailable.reason === "unreadable" ? (
          <Button size="sm" variant="secondary" onClick={() => actions.memoryOpen(id)}>
            {t("memory.readAgain")}
          </Button>
        ) : null}
      </div>
    );
  }
  const count = project.rows.length;
  return (
    <div className="relative flex min-h-0 flex-1 flex-col" data-memory={project.enabled ? "on" : "off"}>
      <div className="flex min-h-0 flex-1 flex-col gap-lg overflow-auto p-lg">
        <Header project={project} actions={actions} busy={busy} />
        {project.enabled ? null : (
          <div className="-mt-sm flex flex-col items-start gap-sm">
            <p className="break-keep text-caption text-subtle-foreground">{t("memory.disclosure")}</p>
            {count > 0 ? (
              <button type="button" className="text-caption text-muted-foreground underline-offset-4 hover:underline" onClick={() => setDeleting(true)} data-memory-delete="link">
                {t("memory.deleteData")}
              </button>
            ) : null}
          </div>
        )}
        {project.enabled && project.analysis === "ai_not_ready" ? (
          <div className="-mt-sm flex flex-col items-start gap-sm" data-memory-ai="not_ready">
            <p className="break-keep text-caption text-subtle-foreground">{t("memory.aiNotReady")}</p>
            <Button size="sm" variant="outline" onClick={() => actions.openSettings("hideAi")}>
              <SettingsIcon aria-hidden="true" />
              {t("memory.openHideAi")}
            </Button>
          </div>
        ) : null}
        {project.enabled && project.capacity_reached ? <p className="-mt-sm break-keep text-caption text-warning">{t("memory.capacity")}</p> : null}
        {project.enabled && project.questions.length > 0 ? <Questions project={project} actions={actions} busy={busy} /> : null}
        {project.enabled ? <Cards project={project} actions={actions} busy={busy} onDelete={() => setDeleting(true)} /> : null}
      </div>
      {project.forgotten !== null ? (
        <div className="pointer-events-none absolute inset-x-0 bottom-0 flex p-md">
          <div role="status" className="pointer-events-auto flex items-center gap-sm rounded-md border border-border bg-popover px-md py-xs text-caption text-foreground shadow-md" data-memory-forgotten={project.forgotten}>
            <span>{t("memory.forgotten", { card: cardName(project.forgotten) })}</span>
            <span aria-hidden="true" className="text-muted-foreground">·</span>
            <button type="button" disabled={busy} className="flex items-center gap-xxs font-medium hover:underline disabled:opacity-(--opacity-disabled)" onClick={() => actions.memoryAction(id, "undo", project.forgotten)}>
              <Undo2Icon aria-hidden="true" className="size-(--size-icon-sm)" />
              {t("memory.undo")}
            </button>
          </div>
        </div>
      ) : null}
      <AlertDialog open={deleting} onOpenChange={setDeleting}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("memory.delete.title", { project: label })}</AlertDialogTitle>
            <AlertDialogDescription>{t("memory.delete.body", { count })}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("memory.delete.keep")}</AlertDialogCancel>
            <AlertDialogAction onClick={() => actions.memoryAction(id, "delete")} data-memory-delete="confirm">
              {t("memory.delete.confirm", { count })}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

/** The switch, the analysis chip and the Hide AI chip. */
function Header({ project, actions, busy }: { project: ProjectMemory; actions: Actions; busy: boolean }) {
  const { t } = useInterfaceTranslation();
  const ai = useShellStore((state) => state.rest?.status?.background_ai ?? null);
  const provider = ai?.provider ? ai.providers.find((row) => row.id === ai.provider) ?? null : null;
  const switchId = `memory-switch-${project.workspace_id}`;
  const dim = project.enabled && project.analysis === "ai_not_ready";
  return (
    <div className="flex flex-col gap-sm">
      <div className="flex items-center gap-sm">
        <label htmlFor={switchId} className="text-body text-foreground">
          {t("memory.switch")}
        </label>
        {project.enabled && project.analysis === "analyzing" ? (
          <span role="status" className="flex items-center gap-xxs rounded-sm border border-border px-xs text-micro text-subtle-foreground" data-memory-analysis="analyzing">
            <LoaderCircleIcon aria-hidden="true" className="size-(--size-icon-sm) animate-spin" />
            {t("memory.analyzing")}
          </span>
        ) : null}
        <Switch
          id={switchId}
          className="ml-auto"
          checked={project.enabled}
          disabled={busy || project.unavailable !== null}
          onCheckedChange={(on) => actions.memoryAction(project.workspace_id, on ? "enable" : "disable")}
          data-memory-switch={project.enabled ? "on" : "off"}
        />
      </div>
      <span className={cn("flex w-fit items-center gap-xs rounded-sm bg-secondary px-xs py-xxs text-caption text-subtle-foreground", dim && "opacity-(--opacity-dimmed)")} data-memory-ai-chip={provider?.agent ?? "none"}>
        {provider ? <AgentLogo agent={provider.agent} label={provider.label} /> : null}
        {provider ? t("memory.hideAi", { agent: provider.label }) : t("memory.hideAiOnly")}
      </span>
    </div>
  );
}

function Questions({ project, actions, busy }: { project: ProjectMemory; actions: Actions; busy: boolean }) {
  const { t } = useInterfaceTranslation();
  return (
    <section aria-label={t("memory.questions")} className="flex flex-col" data-memory-questions={project.questions.length}>
      <h3 className="flex items-baseline gap-sm pb-sm text-caption">
        <span className="text-warning">{t("memory.questions")}</span>
        <span className="text-muted-foreground">{project.questions.length}</span>
      </h3>
      {project.questions.map((question, index) => (
        <Question key={question.number} question={question} project={project} actions={actions} busy={busy} last={index === project.questions.length - 1} />
      ))}
    </section>
  );
}

function Question({ question, project, actions, busy, last }: { question: MemoryQuestion; project: ProjectMemory; actions: Actions; busy: boolean; last: boolean }) {
  const { t } = useInterfaceTranslation();
  const id = project.workspace_id;
  return (
    <div className={cn("grid grid-cols-[auto_auto_1fr] gap-x-sm py-sm", !last && "border-b border-border")} data-memory-question={question.kind} data-memory-card={question.number}>
      <CardToggle number={question.number} project={project} actions={actions} />
      <span className="font-mono text-caption text-muted-foreground">{cardName(question.number)}</span>
      <div className="flex min-w-0 flex-col gap-xs">
        <span className="break-words break-keep text-body text-foreground">{question.title}</span>
        {question.kind === "promote" ? (
          <>
            <span className="break-keep text-caption text-muted-foreground">{t("memory.promote.reason", { human: question.human_source_count, sessions: question.pulled_session_count })}</span>
            <span className="text-caption text-foreground">{t("memory.promote.ask")}</span>
            <span className="flex items-center gap-sm">
              <Button size="sm" disabled={busy} onClick={() => actions.memoryAction(id, "promote", question.number)} data-memory-promote={question.number}>
                {t("memory.promote.yes")}
              </Button>
              <Button size="sm" variant="ghost" disabled={busy} onClick={() => actions.memoryAction(id, "dismiss", question.number)} data-memory-dismiss={question.number}>
                {t("memory.promote.no")}
              </Button>
            </span>
          </>
        ) : (
          <span className="flex items-center gap-sm">
            <span className="flex min-w-0 flex-1 items-center gap-xs text-caption text-subtle-foreground">
              <AlertCircleIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0 text-warning" />
              {t("memory.failed")}
            </span>
            <Button size="sm" variant="outline" disabled={busy} onClick={() => actions.memoryAction(id, "promote", question.number)} data-memory-retry={question.number}>
              {t("memory.retry")}
            </Button>
          </span>
        )}
        {project.expanded?.number === question.number ? <CardDetail card={project.expanded} project={project} actions={actions} busy={busy} /> : null}
      </div>
    </div>
  );
}

function Cards({ project, actions, busy, onDelete }: { project: ProjectMemory; actions: Actions; busy: boolean; onDelete: () => void }) {
  const { t } = useInterfaceTranslation();
  const id = project.workspace_id;
  // The search is the Project's, shared by every window: only a local edit is
  // sent, and otherwise the field follows the accepted query, so two windows
  // never send each other's answer back.
  const shared = project.search?.query ?? "";
  const [query, setQuery] = useState(shared);
  const editing = useRef(false);
  const [all, setAll] = useState(false);
  useEffect(() => {
    if (!editing.current) setQuery(shared);
  }, [shared]);
  useEffect(() => {
    if (!editing.current) return;
    const timer = window.setTimeout(() => {
      editing.current = false;
      if (query !== shared) actions.memorySearch(id, query);
    }, 220);
    return () => window.clearTimeout(timer);
  }, [actions, id, query, shared]);
  const cards = memoryCards(project);
  // Every card is already asked about above (a failed promotion of the
  // Project's one card): the list would be an empty search (design v7, 5).
  if (project.rows.length > 0 && cards.length === 0 && project.search === null) return null;
  const { shown, folded } = visibleCards(cards, project.expanded?.number ?? null, all || project.search !== null);
  return (
    <section aria-label={t("memory.cards")} className="flex flex-col gap-sm" data-memory-cards={project.rows.length}>
      <h3 className="flex items-center gap-sm text-caption">
        <span className="text-subtle-foreground">{t("memory.cards")}</span>
        <span className="text-muted-foreground">{project.rows.length}</span>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button size="icon-sm" variant="ghost" className="ml-auto" aria-label={t("memory.menu")} data-memory-menu="true">
              <MoreHorizontalIcon aria-hidden="true" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem variant="destructive" disabled={busy || project.rows.length === 0} onSelect={onDelete} data-memory-delete="menu">
              <Trash2Icon aria-hidden="true" />
              {t("memory.deleteData")}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </h3>
      {project.rows.length === 0 ? (
        <div className="flex flex-col gap-xs" data-memory-empty="true">
          <span className="flex items-center gap-xs text-body text-foreground">
            <SparklesIcon aria-hidden="true" className="size-(--size-icon-sm) text-muted-foreground" />
            {t("memory.empty.title")}
          </span>
          <span className="break-keep text-caption text-muted-foreground">{t("memory.empty.body")}</span>
        </div>
      ) : (
        <>
          <Input value={query} onChange={(event) => { editing.current = true; setQuery(event.target.value); }} placeholder={t("memory.search")} aria-label={t("memory.search")} data-memory-search="true" />
          {project.search && cards.length === 0 ? <p className="text-caption text-muted-foreground">{t("memory.noMatch")}</p> : null}
          <ol className="flex flex-col">
            {shown.map((row) => (
              <CardRow key={row.number} row={row} project={project} actions={actions} busy={busy} />
            ))}
          </ol>
          {folded > 0 ? (
            <button type="button" className="flex items-center gap-xs self-start text-caption text-subtle-foreground hover:text-foreground" onClick={() => setAll(true)} data-memory-more={folded}>
              <ChevronRightIcon aria-hidden="true" className="size-(--size-icon-sm)" />
              {t("memory.more", { count: folded })}
            </button>
          ) : null}
        </>
      )}
    </section>
  );
}

/** The chevron that shows a card in full or folds it. */
function CardToggle({ number, project, actions }: { number: number; project: ProjectMemory; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const open = project.expanded?.number === number;
  const Icon = open ? ChevronDownIcon : ChevronRightIcon;
  return (
    <button
      type="button"
      aria-expanded={open}
      aria-label={t(open ? "memory.collapse" : "memory.expand", { card: cardName(number) })}
      className="mt-xxs flex size-(--size-icon) items-center justify-center rounded-xs text-muted-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring"
      onClick={() => actions.memoryExpand(project.workspace_id, open ? null : number)}
    >
      <Icon aria-hidden="true" className="size-(--size-icon-sm)" />
    </button>
  );
}

function CardRow({ row, project, actions, busy }: { row: MemoryRow; project: ProjectMemory; actions: Actions; busy: boolean }) {
  const { t, i18n } = useInterfaceTranslation();
  const language = requireInterfaceLanguage(i18n.language);
  const open = project.expanded?.number === row.number;
  const merged = row.promotion?.state === "merged";
  const self = useRef<HTMLLIElement>(null);
  useEffect(() => {
    if (open) self.current?.scrollIntoView({ block: "nearest" });
  }, [open]);
  return (
    <li ref={self} className={cn("grid grid-cols-[auto_auto_1fr] gap-x-sm rounded-md px-xs py-sm", open && "bg-card", merged && !open && "opacity-(--opacity-dimmed)")} data-memory-card={row.number} data-memory-promotion={row.promotion?.state}>
      <CardToggle number={row.number} project={project} actions={actions} />
      <span className="font-mono text-caption text-muted-foreground">{cardName(row.number)}</span>
      <div className="flex min-w-0 flex-col gap-xs">
        <button type="button" className="line-clamp-2 break-words break-keep text-left text-body text-foreground" onClick={() => actions.memoryExpand(project.workspace_id, open ? null : row.number)}>
          {row.title}
        </button>
        <span className="flex flex-wrap items-center gap-x-sm gap-y-xxs text-caption text-muted-foreground">
          <span className="rounded-xs bg-secondary px-xxs">{t(KIND[row.kind])}</span>
          {merged ? (
            <>
              {row.promotion?.target_path ? <span className="rounded-xs bg-secondary px-xxs font-mono">{row.promotion.target_path}</span> : null}
              <span>{t("memory.merged")}</span>
            </>
          ) : (
            <>
              <span>{t("memory.provided", { count: row.provided_session_count })}</span>
              <span>{t("memory.pulled", { count: row.pull_count })}</span>
              <span>{t("memory.human", { count: row.human_source_count })}</span>
              <span className="ml-auto font-mono">{memoryTime(row.updated_at_unix_ms, language)}</span>
            </>
          )}
        </span>
        {row.promotion?.state === "running" ? (
          <span className="flex min-w-0 items-center gap-xs text-caption text-subtle-foreground" data-memory-running={row.number}>
            <LoaderCircleIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0 animate-spin" />
            <span className="shrink-0 whitespace-nowrap">{t("memory.running")}</span>
            {row.promotion.agent ? (
              row.promotion.agent.pane_id ? (
                <button type="button" className="min-w-0 truncate font-mono underline underline-offset-4 hover:text-foreground" onClick={() => actions.openAgent(row.promotion!.agent!.pane_id!)}>
                  {row.promotion.agent.name}
                </button>
              ) : (
                <span className="min-w-0 truncate font-mono">{row.promotion.agent.name}</span>
              )
            ) : null}
          </span>
        ) : null}
        {row.promotion?.state === "pr_open" && row.promotion.pr_number !== null ? (
          <span className="flex items-center gap-sm text-caption text-subtle-foreground">
            <button type="button" aria-label={t("memory.openPr", { number: row.promotion.pr_number })} className="rounded-xs outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring" onClick={() => actions.openPullRequestRow(project.workspace_id, row.promotion!.pr_number)}>
              <PrMark state="pending" number={row.promotion.pr_number} />
            </button>
            {t("memory.prOpen")}
          </span>
        ) : null}
        {open && project.expanded ? <CardDetail card={project.expanded} project={project} actions={actions} busy={busy} /> : null}
      </div>
    </li>
  );
}

/** A card in full, in place: what changed, the body, cited files, sources and Forget. */
function CardDetail({ card, project, actions, busy }: { card: MemoryCard; project: ProjectMemory; actions: Actions; busy: boolean }) {
  const { t, i18n } = useInterfaceTranslation();
  const language = requireInterfaceLanguage(i18n.language);
  const [previous, setPrevious] = useState(false);
  const humans = card.sources.filter((source) => source.human).length;
  const id = project.workspace_id;
  return (
    <div className="flex flex-col gap-md pt-xs" data-memory-detail={card.number}>
      {card.previous ? (
        <div className="flex flex-col gap-xs">
          <span className="text-caption text-muted-foreground">
            {t("memory.changed", { time: memoryTime(card.previous.changed_at_unix_ms, language) })}
            {" · "}
            <button type="button" aria-expanded={previous} className="underline underline-offset-4 hover:text-foreground" onClick={() => setPrevious(!previous)} data-memory-previous={card.number}>
              {t("memory.previous")}
            </button>
          </span>
          {previous ? (
            <div className="flex flex-col items-start gap-xs border-l border-border pl-sm">
              <p className="whitespace-pre-wrap break-words text-caption text-muted-foreground">{card.previous.body}</p>
              <Button size="sm" variant="outline" disabled={busy} onClick={() => actions.memoryAction(id, "undo", card.number)} data-memory-undo={card.number}>
                <Undo2Icon aria-hidden="true" />
                {t("memory.undo")}
              </Button>
            </div>
          ) : null}
        </div>
      ) : null}
      <p className="whitespace-pre-wrap break-words break-keep text-body text-foreground">{card.body}</p>
      {card.cited_paths.length > 0 ? (
        <div className="flex flex-col gap-xxs">
          <span className="text-caption text-muted-foreground">{t("memory.citedFiles")}</span>
          {card.cited_paths.map((path) => (
            <span key={path} className="flex items-center gap-xs font-mono text-caption text-foreground">
              <FileIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0 text-muted-foreground" />
              <span className="min-w-0 truncate">{path}</span>
            </span>
          ))}
        </div>
      ) : null}
      {card.sources.length > 0 ? (
        <div className="flex flex-col gap-xxs">
          <span className="text-caption text-muted-foreground">{t("memory.sources", { count: card.sources.length, human: humans })}</span>
          {card.sources.map((source, index) => (
            <span key={index} className="grid grid-cols-[auto_1fr_auto_auto] items-center gap-x-sm text-caption">
              <UserIcon aria-hidden="true" className={cn("size-(--size-icon-sm)", source.human ? "text-foreground" : "text-muted-foreground")} />
              <span className="min-w-0 truncate text-subtle-foreground">{source.provider_label}</span>
              <span className="font-mono text-muted-foreground">{source.at_unix_ms ? sourceTime(source.at_unix_ms, language) : ""}</span>
              <span className="text-foreground">{source.human ? t("memory.humanSource") : ""}</span>
            </span>
          ))}
        </div>
      ) : null}
      <Button size="sm" variant="ghost" className="self-end text-destructive hover:text-destructive" disabled={busy} onClick={() => actions.memoryAction(id, "forget", card.number)} data-memory-forget={card.number}>
        <Trash2Icon aria-hidden="true" />
        {t("memory.forget")}
      </Button>
    </div>
  );
}
