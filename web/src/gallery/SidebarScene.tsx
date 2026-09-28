// The production sidebar on the gallery's synthetic scene (PRD
// design-review-workflow D-04, B3, B4). This is the shell's own `Sidebar` and
// `createActions`, fed by `sidebarScene` instead of a daemon: a fold the
// operator clicks goes through the real action, and `applyEvent` answers it
// the way the core does. One scene fills one document, because the stores it
// seeds are the app's singletons.

import { useLayoutEffect, useMemo, useState } from "react";
import { createActions } from "../actions";
import { TooltipProvider } from "../components/ui/tooltip";
import { Sidebar } from "../sidebar";
import { useShellStore } from "../store";
import { entryLens, useUiStore } from "../ui";
import { applyEvent, REFERENCE_FOLDS, sidebarScene, type SceneContent, type SceneFolds } from "./sceneData";

/** What one scene document shows; every value comes from its query string. */
export type SceneParams = {
  theme: "light" | "dark";
  /** The sidebar's width in CSS pixels, seeded as the core's `ui_state.sidebar_width`; absent, the sidebar's default. */
  width: number | null;
  /** The interface text scale the Appearance font size sets (`--interface-scale`); the sidebar itself does not follow it (PRD sidebar-typography D-05). */
  scale: number;
  content: SceneContent;
};

export function sceneParams(params: URLSearchParams): SceneParams {
  const width = params.get("width");
  const scale = Number(params.get("scale") ?? "1");
  const content = params.get("content") ?? "reference";
  if (width !== null && !(Number(width) > 0)) throw new Error(`Scene width must be a positive number, got ${width}`);
  if (!(scale > 0)) throw new Error(`Scene scale must be a positive number, got ${params.get("scale")}`);
  if (content !== "reference" && content !== "long") throw new Error(`Unknown scene content ${content}`);
  return { theme: params.get("theme") === "light" ? "light" : "dark", width: width === null ? null : Number(width), scale, content };
}

export function SidebarScene({ theme, width, scale, content }: SceneParams) {
  const [folds, setFolds] = useState<SceneFolds>(REFERENCE_FOLDS);
  const actions = useMemo(
    () =>
      createActions((event) => {
        setFolds((current) => {
          const next = applyEvent(current, event);
          if (!next) console.info(`gallery scene: ${event.kind} is not modelled here`);
          return next ?? current;
        });
      }),
    [],
  );

  // Seed the app's stores before the first paint, and again on every fold.
  const scene = useMemo(() => sidebarScene(content, folds, Date.now()), [content, folds]);
  useLayoutEffect(() => {
    const rest = width === null ? scene.rest : { ...scene.rest, ui_state: { ...scene.rest.ui_state, sidebar_width: width } };
    useShellStore.setState({ rest, agents: scene.agents, connection: "live" });
  }, [scene, width]);

  useLayoutEffect(() => {
    useUiStore.setState({ sidebarMode: "projects", screen: { kind: "overview", projectId: "herdr-ide", lens: entryLens(null, "board") } });
  }, []);

  useLayoutEffect(() => {
    const root = document.documentElement;
    root.classList.toggle("dark", theme === "dark");
    root.classList.toggle("light", theme === "light");
    if (scale === 1) root.style.removeProperty("--interface-scale");
    else root.style.setProperty("--interface-scale", String(scale));
  }, [theme, scale]);

  return (
    <TooltipProvider>
      <div className="flex h-full bg-background text-foreground" data-gallery-scene="projects-sidebar" data-scene-content={content}>
        <Sidebar actions={actions} />
      </div>
    </TooltipProvider>
  );
}
