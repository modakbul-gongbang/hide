// The Factory screens on the gallery's synthetic scene (PRD
// software-factory-ui B24, D-18): the shell's real `Sidebar` beside the real
// `FactoryScreen`, fed by `factoryScene` instead of an engine. The scene
// answers the screen's requests the way the core does: the settings tab's
// `config` read gets its answer, and an answer, merge or retry takes its item
// out of 내 차례. One scene fills one document, because the stores it seeds are
// the app's singletons.

import { useLayoutEffect, useMemo, useState } from "react";
import { createActions } from "../actions";
import { TooltipProvider } from "../components/ui/tooltip";
import { TaskCardView } from "../factory/FactoryCard";
import { FactoryScreen } from "../factory/FactoryScreen";
import type { FactoryCommand } from "../factory/commands";
import { FACTORY_TABS, FACTORY_ENTRY, useUiStore, type FactoryTab } from "../ui";
import { Sidebar } from "../sidebar";
import { clientI18n } from "../i18n/translator";
import { useShellStore } from "../store";
import { factoryScene } from "./factorySceneData";
import { REFERENCE_FOLDS, sidebarScene, type SceneContent } from "./sceneData";

/** What one Factory scene document shows; every value comes from its query string. */
export type FactorySceneParams = {
  theme: "light" | "dark";
  state: string | null;
  language: "ko" | "en";
  tab: FactoryTab;
  /** A Task id whose page opens over the tabs. */
  task: string | null;
  content: SceneContent;
  /** The Observer set (PRD factory-observer) instead of the reference board. */
  observer: boolean;
  /** The Factory the project filter keeps; all of them when absent. */
  factory: string | null;
};

export function factorySceneParams(params: URLSearchParams): FactorySceneParams {
  const state = params.get("state");
  if (state !== null && !["board", "graph", "sizes"].includes(state)) throw new Error(`Unknown Factory scene state ${state}`);
  const tab = state === "board" || state === "graph" ? state : params.get("tab") ?? "turn";
  if (!(FACTORY_TABS as readonly string[]).includes(tab)) throw new Error(`Unknown Factory scene tab ${tab}`);
  const content = params.get("content") ?? "reference";
  if (content !== "reference" && content !== "long") throw new Error(`Unknown scene content ${content}`);
  return { state, language: params.get("lang") === "en" ? "en" : "ko", theme: params.get("theme") === "light" ? "light" : "dark", tab: tab as FactoryTab, task: params.get("task"), content, observer: params.get("observer") === "1", factory: params.get("factory") };
}

/** The verbs that finish an inbox item, so the engine would take it off the list. */
const TAKES_ITEM = new Set(["answer", "merge", "retry", "cancel", "request_changes"]);

export function FactoryScene({ theme, tab, task, content, state, language, observer, factory }: FactorySceneParams) {
  const fixture = useMemo(() => factoryScene(content, Date.now(), observer), [content, observer]);
  const [summary, setSummary] = useState(fixture.summary);
  const openFactory = task === null ? null : (summary.factories.find((view) => view.columns.some((column) => column.cards.some((card) => card.task === task))) ?? null);
  if (task !== null && openFactory === null) throw new Error(`Unknown Factory scene task ${task}`);

  const actions = useMemo(
    () =>
      createActions((event) => {
        if (event.kind === "factory_action") {
          const { request_id: requestId, command } = event.payload as { request_id: string; command: FactoryCommand };
          const answer = command.verb === "config" ? { ok: true, ...fixture.config } : { ok: true };
          if (TAKES_ITEM.has(command.verb) && "task" in command) {
            const ref = command.task;
            setSummary((current) => {
              const inbox = current.inbox.filter((item) => `${item.factory}/${item.task}` !== ref || ("question" in command && command.question !== null && item.question !== command.question));
              return { ...current, inbox, my_turn: inbox.filter((item) => item.group !== "notice").length };
            });
          }
          useShellStore.setState((state) => ({ factory: state.factory ? { ...state.factory, actions: [...state.factory.actions, { request_id: requestId, answer }] } : state.factory }));
        } else if (event.kind === "factory_task_open") {
          const { factory, task: id } = event.payload as { factory: string; task: string };
          useShellStore.setState({ factoryTask: { factory, task: id, detail: fixture.detail(id) } });
        } else if (event.kind === "factory_task_close") {
          useShellStore.setState({ factoryTask: null });
        } else {
          console.info(`gallery scene: ${event.kind} is not modelled here`);
        }
        return true;
      }),
    [fixture],
  );

  const sidebar = useMemo(() => sidebarScene(content, REFERENCE_FOLDS, Date.now()), [content]);
  useLayoutEffect(() => {
    // The agents Hide AI and the worker pickers list; invented, like the rest of the scene.
    const provider = (id: string, label: string, agent: string, models: string[]) => ({ id, label, agent, state: "ready", headline: "", message: null, installed: true, selectable: true, retry_at_ms: null, model: "", models, models_fixed: false, cli_default: true, models_unavailable_reason: null });
    const background_ai = { enabled: true, provider: "claude", chosen: true, providers: [provider("claude", "Claude Code", "claude-code", ["sonnet", "opus", "haiku"]), provider("codex", "Codex", "codex", ["gpt-6.1-sol", "gpt-6.1-luna"])], unavailable_reason: null };
    useShellStore.setState({
      rest: { ...sidebar.rest, status: { ...sidebar.rest.status, background_ai } } as typeof sidebar.rest,
      agents: [...sidebar.agents, ...fixture.workers],
      connection: "live",
      factory: { summary, actions: useShellStore.getState().factory?.actions ?? [] },
      factoryTask: task === null || openFactory === null ? null : { factory: openFactory.id, task, detail: fixture.detail(task) },
    });
  }, [sidebar, summary, fixture, task, openFactory]);

  useLayoutEffect(() => {
    useUiStore.setState({
      sidebarMode: "projects",
      screen: { kind: "factory", place: { ...FACTORY_ENTRY, tab, factory: factory ?? (state === "board" || state === "graph" ? "f-herdr-ide" : null), task: task === null || openFactory === null ? null : { factory: openFactory.id, task } } },
      overviewOpen: false,
    });
  }, [tab, task, openFactory, state, factory]);

  useLayoutEffect(() => {
    void clientI18n.changeLanguage(language);
    document.documentElement.lang = language;
  }, [language]);

  useLayoutEffect(() => {
    const root = document.documentElement;
    root.classList.toggle("dark", theme === "dark");
    root.classList.toggle("light", theme === "light");
  }, [theme]);

  return (
    <TooltipProvider>
      <div className="flex h-full bg-background text-foreground" data-gallery-scene="factory" data-scene-content={content} data-scene-tab={tab}>
        {state === "sizes" ? <main data-factory-screen="sizes" className="flex flex-col gap-lg p-xl">
          <div className="factory-sizes-grid gap-xl text-caption text-muted-foreground"><span>작게 · 240 미만</span><span>보통</span><span>넓게 · 420 이상</span></div>
          {(observer ? [436, 437, 435, 433, 434] : [420, 405, 417, 412, 415, 398, 426, 430, 421, 410]).map((number) => {
            const view = summary.factories[0]!;
            const card = view.columns.flatMap((column) => column.cards).find((card) => (number === 420 && !observer ? card.state === "blocked" : card.task === `t-${number}`))!;
            const item = summary.inbox.find((item) => item.factory === view.id && item.task === card.task && item.kind !== "notice");
            return <div key={number} className="factory-sizes-grid items-start gap-xl">{[210, 300, 470].map((width) => <TaskCardView key={width} factory={view} card={card} showProject={false} actions={actions} item={item} />)}</div>;
          })}
        </main> : <><Sidebar actions={actions} /><FactoryScreen actions={actions} /></>}
      </div>
    </TooltipProvider>
  );
}
