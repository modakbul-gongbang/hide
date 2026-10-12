// The production Tools column on its Memory tab over invented wire values,
// one state per document, for comparison with the v7 design exports (PRD
// project-memory-v2 D-17, B19-B32). Runtime policy is tested in herdr-core.
import { useLayoutEffect, useMemo } from "react";
import { createActions } from "../actions";
import { InterfaceLanguageBoundary } from "../i18n/client";
import { TooltipProvider } from "../components/ui/tooltip";
import type { MemoryRow, ProjectMemory, SnapshotRest } from "../snapshot";
import { useShellStore } from "../store";
import { Tools } from "../Tools";

export const MEMORY_SCENE_STATES = ["workspace", "off", "ai_not_ready", "analyzing", "empty", "remote", "promote", "failed", "forgotten"] as const;
export type MemorySceneState = (typeof MEMORY_SCENE_STATES)[number];
export type MemorySceneParams = { theme: "light" | "dark"; state: MemorySceneState; width: number };

export function memorySceneParams(params: URLSearchParams): MemorySceneParams {
  const state = params.get("state") ?? "workspace";
  if (!MEMORY_SCENE_STATES.includes(state as MemorySceneState)) throw new Error(`Unknown memory scene state ${state}`);
  const width = Number(params.get("width") ?? "360");
  if (!(width > 0)) throw new Error(`Scene width must be a positive number, got ${params.get("width")}`);
  return { theme: params.get("theme") === "light" ? "light" : "dark", state: state as MemorySceneState, width };
}

const MINUTE = 60_000;
const row = (number: number, title: string, kind: MemoryRow["kind"], given: number, pulled: number, human: number, ago: number, promotion: MemoryRow["promotion"] = null): MemoryRow => ({
  number, title, kind, provided_session_count: given, pull_count: pulled, human_source_count: human, updated_at_unix_ms: Date.now() - ago, verifying: false, promotion,
});
const ROWS: MemoryRow[] = [
  row(17, "스크린샷이 큰 세션을 읽다 막히면 종 알림이 멈추므로, 먼저 tool_result 크기를 확인한다", "lesson", 0, 0, 1, 60 * MINUTE),
  row(7, "앱 교체는 운영자가 요청할 때만 한다", "rule", 6, 4, 2, 5 * 1440 * MINUTE, { state: "running", agent: { name: "memory-promote-m7", pane_id: "w1:p9" }, pr_number: null, target_path: null, github_failure: null }),
  row(9, "라이선스는 FCL로 바꾸고 저장소는 공개로 둔다", "decision", 3, 2, 2, 3 * 1440 * MINUTE, { state: "pr_open", agent: null, pr_number: 924, target_path: null, github_failure: null }),
  row(10, "릴리스 노트는 운영자가 확인한 뒤 올린다", "rule", 2, 1, 1, 2 * 1440 * MINUTE, { state: "unconfirmed", agent: null, pr_number: null, target_path: null, github_failure: "not_logged_in" }),
  row(8, "하위 작업은 hide agent spawn --parent here로 띄운다", "rule", 7, 5, 1, 6 * 1440 * MINUTE, { state: "merged", agent: null, pr_number: 921, target_path: "AGENTS.md", github_failure: null }),
  row(12, "nextest 0.9.145 미만은 macOS에서 가짜 LEAK를 낸다", "lesson", 2, 1, 1, 12 * MINUTE),
  row(3, "Rust 검증은 scripts/verify-cargo.sh로만 돌린다", "rule", 9, 12, 3, 1440 * MINUTE),
  row(5, "커밋 메시지에 도구 이름을 넣지 않는다", "rule", 6, 4, 2, 4 * 1440 * MINUTE),
  ...Array.from({ length: 17 }, (_, index) => row(20 + index, `예시 카드 ${index + 1}`, "fact", 1, 0, 0, (7 + index) * 1440 * MINUTE)),
];
const EXPANDED: ProjectMemory["expanded"] = {
  number: 12,
  body: "0.9.145 미만 nextest는 macOS에서 가짜 LEAK를 내므로, 그 버전 이상을 쓰고 LEAK는 먼저 버전을 의심한다.",
  cited_paths: [".config/nextest.toml"],
  sources: [
    { provider_label: "Claude Code", at_unix_ms: Date.now() - 1440 * MINUTE, human: true },
    { provider_label: "Codex", at_unix_ms: Date.now() - 3 * 1440 * MINUTE, human: false },
  ],
  previous: { body: "0.9.132 미만 nextest는 macOS에서 가짜 LEAK를 낸다.", changed_at_unix_ms: Date.now() - 12 * MINUTE },
};

function project(state: MemorySceneState): ProjectMemory {
  const base: ProjectMemory = {
    workspace_id: "herdr-ide", unavailable: null, loading: false, enabled: true, capacity_reached: false,
    rows: ROWS, questions: [], analysis: "idle", forgotten: null, pending_action: null, expanded: null, search: null,
  };
  switch (state) {
    case "workspace":
      return { ...base, expanded: EXPANDED, questions: [
        { kind: "promote", number: 3, title: ROWS[5]!.title, human_session_count: 3, pulled_session_count: 9 },
        { kind: "promote", number: 5, title: ROWS[6]!.title, human_session_count: 2, pulled_session_count: 6 },
      ] };
    case "off": return { ...base, enabled: false };
    case "ai_not_ready": return { ...base, analysis: "ai_not_ready" };
    case "analyzing": return { ...base, analysis: "analyzing" };
    case "empty": return { ...base, rows: [] };
    case "remote": return { ...base, unavailable: { reason: "remote_device", device_name: "mini" }, rows: [] };
    case "promote": return { ...base, rows: ROWS.slice(1, 4) };
    case "failed": return { ...base, rows: [ROWS[3]!], questions: [{ kind: "failed", number: 8, title: ROWS[3]!.title }] };
    case "forgotten": return { ...base, rows: ROWS.filter((card) => card.number !== 9), forgotten: 9 };
  }
}

export function MemoryScene({ theme, state, width }: MemorySceneParams) {
  const actions = useMemo(() => createActions(() => true), []);
  const rest = useMemo(() => ({
    navigator: {
      focused_device_id: "local", focused_checkout_id: "main", focused_workspace_id: "herdr-ide",
      devices: [{ id: "local", label: "This Mac", kind: "local", state: "local" }],
      workspaces: [{ id: "herdr-ide", label: "herdr-ide", device_id: "local", path: "/projects/herdr-ide", checkouts: [{ id: "main", workspace_id: "herdr-ide", path: "/projects/herdr-ide", label: "main", tabs: [], active_tab_id: null, strip: [] }] }],
    },
    status: { background_ai: { enabled: state !== "ai_not_ready", provider: "claude", chosen: true, unavailable_reason: null, providers: [{ id: "claude", label: "Claude Code", agent: "claude-code", state: "ready", selectable: true }] } },
    ui_state: { interface_language: "ko" },
  }) as unknown as SnapshotRest, [state]);
  useLayoutEffect(() => {
    useShellStore.setState({ rest, memory: { project: project(state) }, connection: "gone" });
    document.documentElement.classList.toggle("dark", theme === "dark");
    document.documentElement.classList.toggle("light", theme === "light");
  }, [rest, state, theme]);
  return (
    <TooltipProvider>
      <InterfaceLanguageBoundary />
      <div className="flex h-screen bg-background text-foreground" style={{ width }} data-gallery-scene="memory" data-memory-scene={state}>
        <Tools tool="memory" actions={actions} />
      </div>
    </TooltipProvider>
  );
}
