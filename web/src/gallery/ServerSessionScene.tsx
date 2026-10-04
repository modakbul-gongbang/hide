// Production screens over invented local data. No daemon or provider is used.
import { useLayoutEffect, useMemo } from "react";
import { createActions } from "../actions";
import { TooltipProvider } from "../components/ui/tooltip";
import { ProjectOverview } from "../ProjectOverview";
import { Sidebar } from "../sidebar";
import type { ProjectSessionDetail, SessionSearch } from "../snapshot";
import { useShellStore } from "../store";
import { entryLens, useUiStore } from "../ui";
import { WorkspaceScreen } from "../WorkspaceScreen";
import { REFERENCE_FOLDS, sidebarScene } from "./sceneData";
import type { SceneParams } from "./SidebarScene";

export function ServerSessionScene({ scene: kind, theme, scale, content }: SceneParams & { scene: "workspace-servers" | "session-search" }) {
  const fixture = useMemo(() => {
    const scene = sidebarScene(content, REFERENCE_FOLDS, Date.now());
    const workspace = scene.rest.navigator!.workspaces!.find((project) => project.id === "herdr-ide")!;
    const checkout = workspace.checkouts[0]!;
    const tab = checkout.tabs[0]!;
    const pane = tab.panes[0]!;
    for (const project of scene.rest.navigator!.workspaces!) for (const row of project.checkouts) row.tabs = [];
    checkout.tabs = [{...tab, panes:[pane]}];
    pane.servers = [{host:"127.0.0.1",port:3000},{host:"::1",port:5173}];
    scene.agents = scene.agents.filter((row) => row.pane_id === pane.id);
    scene.rest.navigator!.focused_workspace_id = workspace.id;
    scene.rest.navigator!.focused_checkout_id = checkout.id;
    scene.rest.workspace_view = {device_id:"local",path:checkout.path,views:false,tools:false,tool:"explorer",views_width:null,tools_width:null,views_called:0,views_calls:0};
    scene.rest.status = {...scene.rest.status,server_discovery:{loading:false,failure:null}};
    const row = {id:"conversation",provider:"codex",provider_label:"Codex",locator:"/work/history/conversation.jsonl",checkout_path:checkout.path,first_human_request:"배포 스크립트 정리",started_at_unix_ms:Date.now(),updated_at_unix_ms:Date.now(),title:"로그인 연결 확인",unavailable_reason:null};
    const detail:ProjectSessionDetail = {session_id:row.id,locator:row.locator,loading:false,failure:null,archive:{id:row.id,kind:"session",title:row.title,provider:"codex",unavailable_reason:null,events:[{source_offset:0,kind:"human",role:"user",at_unix_ms:Date.now(),text:"지난 작업에서 로그인 연결을 확인해 줘."},{source_offset:100,kind:"assistant",role:"assistant",at_unix_ms:Date.now(),text:"대화검색으로 로그인 연결을 확인했습니다."}]}};
    return {scene,workspace,row,detail};
  }, [content]);
  const actions = useMemo(() => createActions((event) => {
    if (event.kind === "session_search") {
      const query = String(event.payload.query).trim();
      const match = query.length > 0 && "대화검색으로 로그인 연결을 확인했습니다.".includes(query);
      const search:SessionSearch = {workspace_id:fixture.workspace.id,device_id:"local",provider:String(event.payload.provider ?? "all"),query,loading:false,indexing:false,indexed:1,total:1,days:90,policy_loaded:true,control_failure:null,failure:null,page:{hits:match?[{session_id:fixture.row.id,source_offset:100,role:"Assistant",at_unix_ms:fixture.row.updated_at_unix_ms,snippet:"대화검색으로 로그인 연결을 확인했습니다."}]:[],limited:false,stale:false}};
      useShellStore.setState({sessionSearch:search});
    }
    if (event.kind === "archive_open") useShellStore.setState((state) => ({projectSessions:state.projectSessions ? {...state.projectSessions,detail:fixture.detail} : null}));
    return true;
  }), [fixture]);
  useLayoutEffect(() => {
    useShellStore.setState({rest:fixture.scene.rest,agents:fixture.scene.agents,connection:"live",projectSessions:{device_id:"local",workspace_id:fixture.workspace.id,loading:false,failure:null,unavailable_reason:null,rows:[fixture.row],detail:null},sessionSearch:null});
    useUiStore.setState({sidebarMode:"projects",screen:kind === "session-search" ? {kind:"overview",projectId:fixture.workspace.id,lens:{...entryLens(null,"board"),tab:"sessions"}} : {kind:"workspace"}});
    document.documentElement.classList.toggle("dark",theme === "dark");
    document.documentElement.classList.toggle("light",theme === "light");
    document.documentElement.style.setProperty("--interface-scale",String(scale));
  }, [fixture,kind,theme,scale]);
  return <TooltipProvider><div className="flex h-full bg-background text-foreground" data-gallery-scene={kind}><Sidebar actions={actions}/>{kind === "workspace-servers" ? <WorkspaceScreen actions={actions}/> : <ProjectOverview projectId={fixture.workspace.id} lens={{...entryLens(null,"board"),tab:"sessions"}} actions={actions}/>}</div></TooltipProvider>;
}
