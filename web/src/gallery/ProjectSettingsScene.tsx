// Real settings components over invented catalog/provider answers. This scene
// proves rendered states; daemon persistence is exercised in an isolated run.
import { useLayoutEffect, useMemo } from "react";
import { createActions } from "../actions";
import { TooltipProvider } from "../components/ui/tooltip";
import { InterfaceLanguageBoundary } from "../i18n/client";
import { ProjectSettingsSheet } from "../ProjectSettingsSheet";
import { settingsTarget, type ProjectSettingsPatch, type ProjectSettingsWrite } from "../projectSettings";
import { useShellStore } from "../store";
import { useUiStore } from "../ui";
import { memorySceneProject } from "./MemoryScene";
import { REFERENCE_FOLDS, sidebarScene } from "./sceneData";

export function ProjectSettingsScene({ params }: { params: URLSearchParams }) {
  const theme = params.get("theme") === "light" ? "light" : "dark";
  const content = params.get("content") === "long" ? "long" : "reference";
  const scale = Number(params.get("scale") ?? "1");
  const state = params.get("state") ?? "connections";
  const lang = params.get("lang") ?? "ko";
  if (!["en", "ko", "ja", "zh-CN"].includes(lang)) throw new Error(`Unknown project settings language ${lang}`);
  const scene = useMemo(() => {
    const fixture = sidebarScene(content, REFERENCE_FOLDS, Date.now(), "one");
    if (content === "long") {
      const project = fixture.rest.navigator!.workspaces!.find((row) => !row.is_home && row.is_git)!;
      project.label = "프로젝트 설정과 여러 기기에서 이어지는 아주 긴 한국어 작업 이름";
      project.path = "/fixture/projects/프로젝트-설정-검증을-위한-아주-긴-경로/여러-체크아웃을-함께-사용하는-프로젝트";
    }
    return fixture;
  }, [content]);
  const workspace = scene.rest.navigator!.workspaces!.find((row) => !row.is_home && row.is_git)!;
  const target = settingsTarget(workspace);
  const actions = useMemo(() => createActions((event) => {
    if (event.kind === "project_settings_set") {
      const request = event.payload as { request_id: string; patch: ProjectSettingsPatch };
      useShellStore.setState((shell) => ({ rest: { ...shell.rest, ui_state: { ...shell.rest?.ui_state, project_settings: { example: { version: 1, device_id: workspace.device_id, root: workspace.path, ...shell.rest?.ui_state?.project_settings?.example, ...request.patch } }, project_settings_writes: { example: { device_id: workspace.device_id, root: workspace.path, request_id: request.request_id, state: "saved", error: null } } } } }));
    }
    return true;
  }), [workspace]);
  useLayoutEffect(() => {
    const workspaces = scene.rest.navigator!.workspaces!.map((row) => row === workspace ? { ...row, home_issues: { ...row.home_issues, issues: row.home_issues?.issues ?? [], overflow: row.home_issues?.overflow ?? false, repository: "example/hide" } } : row);
    const receipt: Record<string, ProjectSettingsWrite & { device_id: string; root: string }> = state === "pending" || state === "failed" ? { example: { device_id: workspace.device_id, root: workspace.path, request_id: "scene-intent", state, error: state === "failed" ? "project_settings_save_failed" : null, patch: { issue_source: "local" } } } : {};
    const rest = { ...scene.rest, navigator: { ...scene.rest.navigator, workspaces }, ui_state: { ...scene.rest.ui_state, interface_language: lang as "en" | "ko" | "ja" | "zh-CN", project_settings_writes: receipt } };
    useShellStore.setState({ rest, agents: scene.agents, connection: state === "unavailable" ? "gone" : "live", memory: { project: { ...memorySceneProject(state === "memory-off" ? "off" : "empty"), workspace_id: workspace.id } } });
    useUiStore.setState({ overlay: "project_settings", projectSettingsTarget: target });
    document.documentElement.classList.toggle("dark", theme === "dark");
    document.documentElement.classList.toggle("light", theme === "light");
    document.documentElement.style.setProperty("--interface-scale", String(scale));
  }, [scene, workspace, theme, scale, lang, state]);
  const page = state === "work" ? "work" : state.startsWith("memory") ? "memory" : "connections";
  return <TooltipProvider><InterfaceLanguageBoundary /><div className="h-screen w-full" data-gallery-scene="project-settings" data-scene-ready="true"><ProjectSettingsSheet actions={actions} initialPage={page} /></div></TooltipProvider>;
}
