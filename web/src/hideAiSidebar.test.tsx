import { emptyScope, legacyRest } from "../test/legacyAgentScope";
import { legacyAgentRow } from "../test/legacyAgentRow";
// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterAll, expect, it, vi } from "vitest";
import tokensText from "../../design/tokens.json?raw";
import { createActions } from "./actions";
import { TooltipProvider } from "./components/ui/tooltip";
import { SidebarAgentRow, type AgentRowMenu } from "./components/sidebar-agent-row";
import { Sidebar } from "./sidebar";
import type { AgentRow, HideAiCheckout, SnapshotRest, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";

// The sidebar's tree and rows as the operator sees them when Hide AI started
// work (PRD hide-ai-origin-mark B1 to B7, B9, B10, B13); xterm is not drawn.
const browserCanvas = vi.hoisted(() => {
  const original = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = () => null;
  return { restore: () => { HTMLCanvasElement.prototype.getContext = original; } };
});
afterAll(() => browserCanvas.restore());

const LOCAL = { id: "local", label: "This Mac", kind: "local", state: "local", message: null };
const MEMORY_M43 = { kind: "memory_promote", target: "m43" };

function checkout(id: string, branch: string, path: string, over: object = {}) {
  return {
    agent_scope: emptyScope(), id, workspace_id: "workspace:w1", label: branch, path, branch, is_primary: id === "checkout:main",
    is_worktree: id !== "checkout:main", exists: true, active_tab_id: null, strip: [],
    worktree: { branch, head_sha: null, last_commit_unix_seconds: null }, tabs: [{ id: `tab:${id}`, label: "agent", panes: [{ id: `pane:${id}` }] }], ...over,
  };
}

function agent(id: string, label: string, over: Partial<AgentRow> = {}): AgentRow {
  return legacyAgentRow({
    id: `agent:${id}`, pane_id: `pane:${id}`, identity_label: label, agent_kind: "claude", symbol: "●", group: "working", status_code: "working",
    changed_at_unix_ms: null, emphasized: false, unread: false, demand: "none", activity: "working", ...over,
  }) as AgentRow;
}

function setup(listed: HideAiCheckout[]) {
  const workspace = { agent_scope: emptyScope(), id: "workspace:w1", label: "herdr-ide", path: "/repo", device_id: "local", is_git: true, registered: true,
    temporary: false, pinned: false, remote_target_id: null, inactive_checkouts: { expanded: false, checkout_ids: [] },
    checkouts: [
      checkout("checkout:main", "main", "/repo"),
      checkout("checkout:m43", "hide-memory-m43-mv4n0561", "/repo.worktrees/hide-memory-m43-mv4n0561"),
      checkout("checkout:m72", "hide-memory-m72-mv4msuko", "/repo.worktrees/hide-memory-m72-mv4msuko", { tabs: [], has_panes: false, purpose: { text: "PR 없이 끝남", origin: "token" } }),
      // The operator's own checkout whose branch merely spells Hide AI's naming (B13).
      checkout("checkout:own", "hide-memory-m9-by-hand", "/repo.worktrees/hide-memory-m9-by-hand"),
    ],
  } as unknown as Workspace;
  const rows = [agent("main", "사이드바 가독성 개선"), agent("checkout:m43", "m43 규칙을 문서로 옮기는 중", { hide_ai: { task: MEMORY_M43 } })];
  const rest = { navigator: { devices: [LOCAL], workspaces: [workspace], agents: rows, hide_ai_checkouts: listed, focused_device_id: "local" } } as unknown as SnapshotRest;
  return { rest, rows };
}

async function render(listed: HideAiCheckout[]) {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("ResizeObserver", class { observe() {} unobserve() {} disconnect() {} });
  const tokens = (JSON.parse(tokensText) as { tokens: Record<string, { value: number }> }).tokens;
  const widthTokens = ["--size-sidebar-ideal", "--size-sidebar-min", "--size-sidebar-max"];
  for (const name of widthTokens) document.documentElement.style.setProperty(name, `${tokens[name]?.value}px`);
  const shell = useShellStore.getState();
  const ui = useUiStore.getState();
  const { rest, rows } = setup(listed);
  useShellStore.setState({ rest: legacyRest(rest, rows), agents: rows, connection: "live" });
  useUiStore.setState({ sidebarMode: "projects" });
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => root.render(<TooltipProvider><Sidebar actions={createActions(() => true)} /></TooltipProvider>));
  return {
    container,
    cleanup: async () => {
      await act(async () => root.unmount());
      container.remove();
      useShellStore.setState(shell, true);
      useUiStore.setState(ui, true);
      for (const name of widthTokens) document.documentElement.style.removeProperty(name);
      vi.unstubAllGlobals();
    },
  };
}

const LISTED: HideAiCheckout[] = [
  { device_id: "local", path: "/repo.worktrees/hide-memory-m43-mv4n0561", task: MEMORY_M43 },
  { device_id: "local", path: "/repo.worktrees/hide-memory-m72-mv4msuko", task: { kind: "memory_promote", target: "m72" } },
];

it("gathers the checkouts Hide AI made under one head behind the operator's, with their task and no badge inside (B1, B2, B6, B13)", async () => {
  const { container, cleanup } = await render(LISTED);
  try {
    const group = container.querySelector("[data-hide-ai-group]")!;
    expect(group.querySelector("[data-hide-ai-count]")?.textContent).toBe("2");
    const inGroup = [...group.querySelectorAll("[data-checkout-row]")].map((row) => row.getAttribute("data-checkout-row"));
    expect(inGroup).toEqual(["checkout:m43", "checkout:m72"]);
    // The operator's checkouts, the lookalike included, stay above the head.
    const rows = [...container.querySelectorAll("[data-checkout-row]")].map((row) => row.getAttribute("data-checkout-row"));
    expect(rows).toEqual(["checkout:main", "checkout:own", "checkout:m43", "checkout:m72"]);
    // An open checkout names the task under its agent row; a folded one in the purpose line's place, ahead of the purpose.
    expect(group.querySelector('[data-pane="pane:checkout:m43"] [data-agent-task]')?.textContent).toBe("Promote Memory · m43");
    expect(group.querySelector('[data-checkout-row="checkout:m72"] [data-purpose]')?.textContent).toBe("Promote Memory · m72 · PR 없이 끝남");
    expect(group.querySelector("[data-hide-ai-badge]")).toBeNull();
    // The operator's agent carries no task line.
    expect(container.querySelector('[data-pane="pane:main"] [data-agent-task]')).toBeNull();
  } finally {
    await cleanup();
  }
});

it("draws no head while no checkout is Hide AI's, even when a branch is named like one (B5, B13)", async () => {
  const { container, cleanup } = await render([]);
  try {
    expect(container.querySelector("[data-hide-ai-group]")).toBeNull();
    expect(container.querySelectorAll("[data-checkout-row]")).toHaveLength(4);
  } finally {
    await cleanup();
  }
});

const MENU: AgentRowMenu = { items: () => [], onSelect: () => {} };

function Row({ row, place, tree }: { row: AgentRow; place: string | null; tree?: boolean }) {
  return (
    <TooltipProvider>
      <ul>
        <SidebarAgentRow
          agent={row} device={null} place={place} depth={0} selected={false} onOpen={() => {}} onOpenPullRequest={() => {}} inset="0"
          tree={tree ? { place: { depth: 0, chain: [], last: true } as never, open: null, onToggle: () => {}, popover: null } : null}
          menu={MENU}
        />
      </ul>
    </TooltipProvider>
  );
}

it("badges the provider icon of a Hide AI row where it stands alone, names Hide AI and the task, and leaves other rows plain (B3, B7, B9)", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  try {
    const started = agent("m72", "m72 결정을 문서로 옮기기", { hide_ai: { task: { kind: "memory_promote", target: "m72" } }, group: "done", status_code: "stopped" });
    const operators = agent("ci", "CI 러너 샤드 정리", { group: "done", status_code: "stopped" });
    await act(async () => root.render(<Row row={started} place="herdr-ide › Promote Memory · m72" />));
    expect(container.querySelector("[data-hide-ai-badge]")).not.toBeNull();
    expect(container.querySelector("[data-agent-place]")?.textContent).toBe("herdr-ide › Promote Memory · m72");
    expect(container.querySelector("[data-agent-open]")?.getAttribute("aria-label")).toContain("Hide AI · Promote Memory · m72");
    await act(async () => root.render(<Row row={operators} place="herdr-ide › main" />));
    expect(container.querySelector("[data-hide-ai-badge]")).toBeNull();
    expect(container.querySelector("[data-agent-open]")?.getAttribute("aria-label")).not.toContain("Hide AI");
    // In a project's tree the group says it, so the row has no badge but a task line (B2); with no task it says Hide AI alone (B10).
    const named = agent("bare", "알 수 없는 일", { hide_ai: {} });
    await act(async () => root.render(<Row row={named} place={null} tree />));
    expect(container.querySelector("[data-hide-ai-badge]")).toBeNull();
    expect(container.querySelector("[data-agent-task]")?.textContent).toBe("Hide AI");
  } finally {
    await act(async () => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  }
});
