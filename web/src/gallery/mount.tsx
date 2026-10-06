import { StrictMode } from "react";
import type { Root } from "react-dom/client";
import { Gallery, GalleryFrame } from "./Gallery";
import { CmdkScene, cmdkSceneParams } from "./CmdkScene";
import { GALLERY, type Section } from "./manifest";
import { ServerSessionScene } from "./ServerSessionScene";
import { sceneParams, SidebarScene } from "./SidebarScene";
import { AreaFocusScene } from "./AreaFocusScene";
import { AgentOnboardingScene } from "./AgentOnboardingScene";
import { FactoryScene, factorySceneParams } from "./FactoryScene";

export function mountGallery(root: Root) {
  const params = new URLSearchParams(window.location.search);
  const scene = params.get("scene");
  if (scene !== null) {
    if (scene === "area-focus") {
      root.render(<StrictMode><AreaFocusScene theme={params.get("theme") === "light" ? "light" : "dark"} /></StrictMode>);
      return;
    }
    // A scene seeds the app's own stores, so it is one document per scene.
    if (scene === "workspace-servers" || scene === "session-search") {
      root.render(<StrictMode><ServerSessionScene scene={scene} {...sceneParams(params)}/></StrictMode>);
      return;
    }
    if (scene === "agent-onboarding") {
      document.title = "hide · Agent onboarding scene";
      root.render(<StrictMode><AgentOnboardingScene theme={params.get("theme") === "light" ? "light" : "dark"} /></StrictMode>);
      return;
    }
    if (scene === "search-palette") {
      document.title = "hide · ⌘K scene";
      root.render(<StrictMode><CmdkScene {...cmdkSceneParams(params)} /></StrictMode>);
      return;
    }
    if (scene === "factory") {
      document.title = "hide · Factory scene";
      root.render(<StrictMode><FactoryScene {...factorySceneParams(params)} /></StrictMode>);
      return;
    }
    if (scene !== "projects-sidebar") throw new Error(`Unknown gallery scene ${scene}`);
    document.title = "hide · Projects sidebar scene";
    root.render(
      <StrictMode>
        <SidebarScene {...sceneParams(params)} />
      </StrictMode>,
    );
    return;
  }
  const frame = params.get("frame");
  const theme = params.get("theme") === "light" ? "light" : "dark";
  if (frame) {
    // A frame's layer takes focus as it opens; without this the gallery page
    // around the frame would scroll to it on every load.
    const focus = HTMLElement.prototype.focus;
    HTMLElement.prototype.focus = function (options?: FocusOptions) {
      focus.call(this, { ...options, preventScroll: true });
    };
    const cut = frame.indexOf("/");
    const section = frame.slice(0, cut) as Section;
    if (!(section in GALLERY)) throw new Error(`Unknown gallery section ${section}`);
    root.render(
      <StrictMode>
        <GalleryFrame section={section} state={frame.slice(cut + 1)} theme={theme} />
      </StrictMode>,
    );
    return;
  }
  document.title = "hide · System gallery";
  root.render(
    <StrictMode>
      <Gallery />
    </StrictMode>,
  );
}
