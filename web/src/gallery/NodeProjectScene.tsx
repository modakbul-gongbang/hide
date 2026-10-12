// A MacBook project's Overview after the core moved (PRD core-host-node-move
// D-29) on the gallery's synthetic data: the shipped Overview on its Issues
// tab with a GitHub or a Local source, the New issue dialog over it, and the
// disk cleanup sheet the size opens. Each state is set up from the address;
// one scene fills one document, because the stores it seeds are the app's
// singletons.

import { useLayoutEffect, useMemo } from "react";
import { createActions } from "../actions";
import { TooltipProvider } from "../components/ui/tooltip";
import { clientI18n } from "../i18n/translator";
import { ProjectOverview } from "../ProjectOverview";
import { useShellStore } from "../store";
import { entryLens, useUiStore, type OverviewLens } from "../ui";
import { WorkspaceDialogs } from "../WorkspaceDialogs";
import { NODE_DEVICE } from "./coreMoveSceneData";
import { NODE_PROJECT, nodeProjectRest } from "./nodeProjectSceneData";

export const NODE_PROJECT_STATES = ["issues-github", "issues-local", "new-issue", "cleanup"] as const;
export type NodeProjectState = (typeof NODE_PROJECT_STATES)[number];
export type NodeProjectSceneParams = { theme: "light" | "dark"; state: NodeProjectState; language: "ko" | "en" };

export function nodeProjectSceneParams(params: URLSearchParams): NodeProjectSceneParams {
  const state = params.get("state") ?? NODE_PROJECT_STATES[0];
  if (!(NODE_PROJECT_STATES as readonly string[]).includes(state)) throw new Error(`Unknown node-project scene state ${state}`);
  return { theme: params.get("theme") === "light" ? "light" : "dark", state: state as NodeProjectState, language: params.get("lang") === "en" ? "en" : "ko" };
}

export function NodeProjectScene({ theme, state, language }: NodeProjectSceneParams) {
  const fixture = useMemo(() => nodeProjectRest(state === "issues-github" ? "github" : "local", state === "cleanup", "reference", Date.now()), [state]);
  const actions = useMemo(() => createActions(() => true), []);
  const lens: OverviewLens = { ...entryLens(null, "board"), tab: "issues" };

  useLayoutEffect(() => {
    // The MacBook's window names its machine in the address, as `hide connect` does.
    window.history.replaceState(null, "", `${window.location.pathname}${window.location.search}#node=${NODE_DEVICE}`);
    useShellStore.setState({ rest: fixture.rest, agents: fixture.agents, connection: "live" });
    useUiStore.setState({
      sidebarMode: "projects",
      screen: { kind: "main" },
      overviewProjectId: NODE_PROJECT,
      overviewLens: lens,
      workspaceDialog: state === "new-issue" ? { kind: "new_issue", workspaceId: NODE_PROJECT } : state === "cleanup" ? { kind: "disk_cleanup", workspaceId: NODE_PROJECT, filter: "done" } : null,
    });
    document.documentElement.classList.toggle("dark", theme === "dark");
    document.documentElement.classList.toggle("light", theme === "light");
  }, [fixture, state, theme]);

  useLayoutEffect(() => {
    void clientI18n.changeLanguage(language);
    document.documentElement.lang = language;
  }, [language]);

  return (
    <TooltipProvider>
      <div className="flex h-screen w-screen flex-col overflow-hidden bg-background text-foreground" data-gallery-scene="node-project" data-node-project-scene-ready={state}>
        <ProjectOverview projectId={NODE_PROJECT} lens={lens} actions={actions} />
        <WorkspaceDialogs actions={actions} />
      </div>
    </TooltipProvider>
  );
}
