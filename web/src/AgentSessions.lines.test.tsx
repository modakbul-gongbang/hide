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

// The Agents tab's second line says what the sidebar's does: a waiting ring
// leads with 대기, a sleeping row has none, a waking or unknown one says its
// state, and only an unknown row is dimmed (status-marks B16, B29, B31).
function row(id: string, over: Partial<AgentRow>): AgentRow {
  const base = legacyAgentRow({ id, pane_id: id, identity_label: `작업 ${id}`, agent_kind: "claude", mark: "quiet", group: "seen", demand: "none", unread: false, status_code: "idle", emphasized: false, changed_at_unix_ms: 1, ...over });
  return { ...base, state: { ...base.state, ...over.state, session: { group: "working", line: "파서를 나누는 중", unfinished: false } } } as AgentRow;
}

async function render(agents: AgentRow[]) {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const scope: AgentScope = { ...emptyScope(),
    members: agents.map((agent) => ({ pane_id: agent.pane_id, project_id: "project", checkout_id: "main" })),
    sessions: { groups: [{ group: "working", members: agents.map((_, index) => index) }] },
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
  await act(async () => root.render(<TooltipProvider><AgentSessions actions={createActions(() => true)} /></TooltipProvider>));
  return {
    container,
    cleanup: async () => {
      await act(async () => root.unmount());
      container.remove();
      useShellStore.setState(shell, true);
      useUiStore.setState(ui, true);
      vi.unstubAllGlobals();
    },
  };
}

it("draws each row's state line as the sidebar does and dims only an unknown row", async () => {
  const waiting = row("waiting", { mark: "going", group: "working", status_code: "waiting", wait: "checks", activity: "stopped" });
  waiting.state = { ...waiting.state, line: { text: "", mode: "waiting", tone: { kind: "subtle", read: false }, pull_request: 58 } };
  const { container, cleanup } = await render([
    waiting,
    row("sleeping", { mark: "asleep", status_code: "sleeping", activity: "stopped" }),
    row("waking", { mark: "asleep", status_code: "waking", activity: "stopped" }),
    row("unknown", { mark: "quiet", status_code: "unknown", activity: "unknown" }),
  ]);
  try {
    const at = (id: string) => container.querySelector(`[data-session-row="${id}"]`)!;
    expect(at("waiting").querySelector('[data-session-line="waiting"]')?.textContent).toBe("Waiting · CI · PR #58");
    expect(at("sleeping").querySelector("[data-session-line]")).toBeNull();
    expect(at("waking").querySelector('[data-session-line="waking"]')?.textContent).toBe("Waking…");
    expect(at("unknown").querySelector('[data-session-line="unknown"]')?.textContent).toBe("Status unknown");
    expect(at("unknown").className).toContain("opacity-(--opacity-dimmed)");
    for (const id of ["waiting", "sleeping", "waking"]) expect(at(id).className).not.toContain("opacity-(--opacity-dimmed)");
  } finally {
    await cleanup();
  }
});
