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
import { factoryScene, OBSERVER_STATES, type ObserverVariant } from "./factorySceneData";
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
  /** Every kind of card at the three sizes instead of the screen. */
  sizes: boolean;
  /** 직접 or 맡김 in place of the Observer set's 함께. */
  variant: ObserverVariant | null;
  /** Hide AI turned off. */
  aiOff: boolean;
};

export function factorySceneParams(params: URLSearchParams): FactorySceneParams {
  const state = params.get("state");
  if (state !== null && !["board", "graph", "sizes"].includes(state) && !(state in OBSERVER_STATES)) throw new Error(`Unknown Factory scene state ${state}`);
  const content = params.get("content") ?? "reference";
  if (content !== "reference" && content !== "long") throw new Error(`Unknown scene content ${content}`);
  const common = { state, language: params.get("lang") === "en" ? ("en" as const) : ("ko" as const), theme: params.get("theme") === "light" ? ("light" as const) : ("dark" as const), content: content as SceneContent };
  // An Observer frame's state names everything it opens.
  const frame = state === null ? undefined : OBSERVER_STATES[state];
  if (frame) return { ...common, tab: frame.tab, task: frame.task ?? null, observer: true, factory: frame.factory ?? null, sizes: frame.cards === true, variant: frame.variant ?? null, aiOff: frame.aiOff === true };
  const tab = state === "board" || state === "graph" ? state : params.get("tab") ?? "turn";
  if (!(FACTORY_TABS as readonly string[]).includes(tab)) throw new Error(`Unknown Factory scene tab ${tab}`);
  return { ...common, tab: tab as FactoryTab, task: params.get("task"), observer: params.get("observer") === "1", factory: params.get("factory"), sizes: state === "sizes", variant: null, aiOff: false };
}

/** The cards each sizes sheet draws, with the label the Observer frame gives its row. */
const REFERENCE_CARDS = [420, 405, 417, 412, 415, 398, 426, 430, 421, 410].map((number) => [number, ""] as const);
const OBSERVER_CARDS = [[436, "답 필요 · 선택지 셋"], [437, "카드가 틀림 · AI 제안"], [435, "보고 없음 · 진단"], [433, "작업자 사라짐 · 재시작 뒤"], [434, "일시정지"]] as const;

/** The verbs that finish an inbox item, so the engine would take it off the list. */
const TAKES_ITEM = new Set(["answer", "merge", "retry", "cancel", "request_changes"]);

export function FactoryScene({ theme, tab, task, content, state, language, observer, factory, sizes, variant, aiOff }: FactorySceneParams) {
  const fixture = useMemo(() => factoryScene(content, Date.now(), observer, variant), [content, observer, variant]);
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
    const background_ai = { enabled: !aiOff, provider: "claude", chosen: true, providers: [provider("claude", "Claude Code", "claude-code", ["sonnet", "opus", "haiku"]), provider("codex", "Codex", "codex", ["gpt-6.1-sol", "gpt-6.1-luna"])], unavailable_reason: null };
    useShellStore.setState({
      rest: { ...sidebar.rest, status: { ...sidebar.rest.status, background_ai } } as typeof sidebar.rest,
      agents: [...sidebar.agents, ...fixture.workers],
      connection: "live",
      factory: { summary, actions: useShellStore.getState().factory?.actions ?? [] },
      factoryTask: task === null || openFactory === null ? null : { factory: openFactory.id, task, detail: fixture.detail(task) },
    });
  }, [sidebar, summary, fixture, task, openFactory, aiOff]);

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
        {sizes ? <main data-factory-screen="sizes" className="flex flex-col gap-lg p-xl">
          <div className="flex gap-xl text-caption text-muted-foreground">
            {observer ? <span className="factory-sizes-label" /> : null}
            <div className="factory-sizes-grid gap-xl"><span>작게 · 240 미만</span><span>보통</span><span>넓게 · 420 이상</span></div>
          </div>
          {(observer ? OBSERVER_CARDS : REFERENCE_CARDS).map(([number, label]) => {
            const view = summary.factories[0]!;
            const card = view.columns.flatMap((column) => column.cards).find((card) => (number === 420 && !observer ? card.state === "blocked" : card.task === `t-${number}`))!;
            const item = summary.inbox.find((item) => item.factory === view.id && item.task === card.task && item.kind !== "notice");
            return (
              <div key={number} className="flex items-start gap-xl">
                {observer ? <span className="factory-sizes-label text-caption text-subtle-foreground">{label}</span> : null}
                <div className="factory-sizes-grid items-start gap-xl">{[210, 300, 470].map((width) => <TaskCardView key={width} factory={view} card={card} showProject={false} actions={actions} item={item} />)}</div>
              </div>
            );
          })}
        </main> : <><Sidebar actions={actions} /><FactoryScreen actions={actions} /></>}
      </div>
    </TooltipProvider>
  );
}
