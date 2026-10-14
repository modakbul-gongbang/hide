// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterAll, expect, it, vi } from "vitest";
import { emptyScope } from "../test/legacyAgentScope";
import { legacyAgentRow } from "../test/legacyAgentRow";
import { createActions } from "./actions";
import { AgentSessions } from "./AgentSessions";
import { TooltipProvider } from "./components/ui/tooltip";
import type { AgentScope } from "./agentScope";
import type { AgentRow, Checkout, SnapshotRest, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";

const browserCanvas = vi.hoisted(() => {
  const original = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = () => null;
  return { restore: () => { HTMLCanvasElement.prototype.getContext = original; } };
});
afterAll(() => browserCanvas.restore());

// The Sessions tab's rows as the operator sees them when Hide AI started one
// of the agents (PRD hide-ai-origin-mark B4, B9).
function row(id: string, over: Partial<AgentRow> = {}): AgentRow {
  return {
    ...legacyAgentRow({ id, pane_id: id, identity_label: `작업 ${id}`, agent_kind: "claude", symbol: "○", group: "seen", demand: "none", unread: false, status_code: "idle", emphasized: false, changed_at_unix_ms: 1, ...over }),
    state: { ...legacyAgentRow({}).state, session: { group: "working", line: "", unfinished: false } },
  } as AgentRow;
}

it("draws the owl on the Sessions row of an agent Hide AI started, names Hide AI in its label, and leaves the operator's row plain", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const agents = [row("operator"), row("started", { hide_ai: { task: { kind: "memory_promote", target: "m43" } } })];
  const scope: AgentScope = { ...emptyScope(),
    members: agents.map((agent) => ({ pane_id: agent.pane_id, project_id: "project", checkout_id: "main" })),
    sessions: { groups: [{ group: "working", members: [0, 1] }] },
  };
  const checkout = { id: "main", workspace_id: "project", label: "main", branch: "main", path: "/fixture", agent_scope: scope, tabs: [{ id: "tab", panes: agents.map((agent) => ({ id: agent.pane_id })) }] } as unknown as Checkout;
  const project = { id: "project", device_id: "local", label: "프로젝트", path: "/fixture", is_git: true, agent_scope: scope, checkouts: [checkout] } as Workspace;
  const rest = { navigator: { devices: [{ id: "local", kind: "local", label: "Local", agent_scope: scope }], focused_device_id: "local", focused_checkout_id: "main", workspaces: [project], agents } } as unknown as SnapshotRest;
  const shell = useShellStore.getState();
  const ui = useUiStore.getState();
  useShellStore.setState({ rest, agents, connection: "live" });
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  try {
    await act(async () => root.render(<TooltipProvider><AgentSessions actions={createActions(() => true)} /></TooltipProvider>));
    const started = container.querySelector('[data-session-row="started"]');
    expect(started).not.toBeNull();
    expect(started?.querySelector("[data-hide-mark]")).not.toBeNull();
    expect(started?.querySelector("[data-agent-mark]")).toBeNull();
    expect(started?.querySelector("[data-session-focus]")?.getAttribute("aria-label")).toContain("Claude Code");
    expect(started?.querySelector("[data-session-focus]")?.getAttribute("aria-label")).toContain("Hide AI · Promote Memory · m43");
    const operators = container.querySelector('[data-session-row="operator"]');
    expect(operators).not.toBeNull();
    expect(operators?.querySelector("[data-hide-mark]")).toBeNull();
    expect(operators?.querySelector("[data-agent-mark]")).toBeNull();
    expect(operators?.querySelector("[data-session-focus]")?.getAttribute("aria-label")).not.toContain("Hide AI");
  } finally {
    await act(async () => root.unmount());
    container.remove();
    useShellStore.setState(shell, true);
    useUiStore.setState(ui, true);
    vi.unstubAllGlobals();
  }
});
