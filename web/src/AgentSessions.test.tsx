// @vitest-environment jsdom
// Literal core wire facts pin what the operator sees; no web state ladder.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterAll, expect, it, vi } from "vitest";
import { emptyScope } from "../test/legacyAgentScope";
import { legacyAgentRow } from "../test/legacyAgentRow";
import type { Actions } from "./actions";
import type { AgentScope } from "./agentScope";
import { TooltipProvider } from "./components/ui/tooltip";
import { clientI18n } from "./i18n/translator";
import { sessionsModel } from "./sessionPanel";
import type { AgentRow, Checkout, SessionGroup, SnapshotRest, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { Tools } from "./Tools";

const canvas = vi.hoisted(() => vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null));
afterAll(() => canvas.mockRestore());

vi.mock("./ExplorerTree", () => ({ ExplorerTree: () => null }));
vi.mock("./HistoryList", () => ({ HistoryList: () => null }));

function fixture() {
  const row = (id: string, group: SessionGroup): AgentRow => ({
    ...legacyAgentRow({ id, pane_id: id, identity_label: `긴 한국어 작업 제목 ${id}`, agent_kind: "codex", symbol: "○", group: "seen", demand: "question", unread: false, status_code: "question", emphasized: false, changed_at_unix_ms: 1 }),
    state: { ...legacyAgentRow({}).state, session: { group, tag: group === "my_turn" ? "fix" : "working" } },
    request: { verb: group === "my_turn" ? "fix" : "working", verb_since_unix_ms: 1, line: "확인할 결과 한 줄", request: null, later_by: null, reply: null, pull_requests: [] },
  });
  const agents = [row("first", "my_turn"), row("second", "in_progress")];
  const scope = (members: string[]): AgentScope => ({ ...emptyScope(),
    members: members.map((pane_id) => ({ pane_id, project_id: "project", checkout_id: pane_id === "first" ? "main" : "feature" })),
    sessions: { counts: { my_turn: members.includes("first") ? 1 : 0, review_merge: 0, in_progress: members.includes("second") ? 1 : 0, resting: 0, resolved_today: 0 },
      groups: members.map((id, member) => ({ group: id === "first" ? "my_turn" as const : "in_progress" as const, members: [member] })) },
  });
  const checkouts = ["main", "feature"].map((id, index) => ({ id, workspace_id: "project", label: id, branch: id, path: `/fixture/${id}`, agent_scope: scope([agents[index]!.pane_id]), tabs: [{ id: `tab-${id}`, panes: [{ id: agents[index]!.pane_id }] }] })) as Checkout[];
  const project = { id: "project", device_id: "local", label: "프로젝트", path: "/fixture", is_git: true, agent_scope: scope(["first", "second"]), checkouts } as Workspace;
  const rest = { navigator: { devices: [{ id: "local", kind: "local", label: "Local", agent_scope: project.agent_scope }], focused_device_id: "local", focused_checkout_id: "main", workspaces: [project] } } as unknown as SnapshotRest;
  return { rest, agents, project };
}

it("resolves project and checkout members using the core's groups and counts", () => {
  const { rest, agents } = fixture();
  const all = sessionsModel(rest, agents, null);
  expect(all.project?.id).toBe("project");
  expect(all.members.map((row) => row.agent.pane_id)).toEqual(["first", "second"]);
  expect(all.scope?.sessions.groups.map((group) => group.group)).toEqual(["my_turn", "in_progress"]);
  const front = sessionsModel(rest, agents, "main");
  expect(front.members.map((row) => row.agent.pane_id)).toEqual(["first"]);
  expect(front.scope?.sessions.counts).toMatchObject({ my_turn: 1, in_progress: 0 });
});

it("draws Sessions first, core counts and two-line rows, then opens and filters the named pane", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const original = useShellStore.getState();
  const language = clientI18n.language;
  await clientI18n.changeLanguage("ko");
  const { rest, agents } = fixture();
  const actions = { showTool: vi.fn(), observeRequestView: vi.fn(), openAgent: vi.fn() } as unknown as Actions;
  useShellStore.setState({ rest, agents, connection: "live" });
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  try {
    await act(async () => root.render(<TooltipProvider><Tools tool="agent_sessions" actions={actions} /></TooltipProvider>));
    expect([...container.querySelectorAll("[data-tool-tab]")].map((tab) => tab.getAttribute("data-tool-tab"))).toEqual(["agent_sessions", "explorer", "changes"]);
    expect(container.querySelector('[data-session-count="my_turn"]')?.textContent).toBe("1");
    expect(container.querySelector('[data-session-count="in_progress"]')?.textContent).toBe("1");
    expect(container.querySelector('[data-session-row="first"]')?.textContent).toContain("긴 한국어 작업 제목 first");
    expect(container.querySelector('[data-session-row="first"]')?.textContent).toContain("확인할 결과 한 줄");
    const first = container.querySelector<HTMLButtonElement>('[data-session-row="first"] button')!;
    await act(async () => first.click());
    expect(actions.openAgent).toHaveBeenCalledWith("first");
    first.focus();
    await act(async () => first.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true })));
    expect(document.activeElement?.getAttribute("data-session-focus")).toBe("group");
    const only = [...container.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "main만")!;
    await act(async () => only.click());
    expect(container.querySelector('[data-session-row="second"]')).toBeNull();
    expect(container.querySelector('[data-session-count="in_progress"]')?.textContent).toBe("0");
    const tab = container.querySelector<HTMLButtonElement>('[data-tool-tab="agent_sessions"]')!;
    await act(async () => tab.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true })));
    expect(actions.showTool).toHaveBeenCalledWith("explorer");
    expect(actions.observeRequestView).toHaveBeenCalledWith(true);
  } finally {
    await act(async () => root.unmount());
    container.remove();
    useShellStore.setState(original, true);
    await clientI18n.changeLanguage(language);
    vi.unstubAllGlobals();
  }
  expect(actions.observeRequestView).toHaveBeenLastCalledWith(false);
});
