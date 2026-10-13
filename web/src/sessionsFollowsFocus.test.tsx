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

function row(id: string, over: Partial<AgentRow> = {}): AgentRow {
  return {
    ...legacyAgentRow({ id, pane_id: id, identity_label: `작업 ${id}`, agent_kind: "claude", symbol: "●", group: "working", demand: "none", unread: false, status_code: "working", emphasized: false, changed_at_unix_ms: 1, ...over }),
    state: { ...legacyAgentRow({}).state, session: { group: "working", line: "", unfinished: false } },
  } as AgentRow;
}

/** Three working roots in one checkout; the last runs a delegated child its closed tree hides. */
function catalog(): { rest: SnapshotRest; agents: AgentRow[] } {
  const agents = [
    row("first"), row("second"), row("parent", { lineage_child_pane_ids: ["child"] }),
    row("child", { delegated: true, lineage_parent_pane_id: "parent" }),
  ];
  const roots = agents.slice(0, 3);
  const scope: AgentScope = { ...emptyScope(),
    members: roots.map((agent) => ({ pane_id: agent.pane_id, project_id: "project", checkout_id: "main" })),
    sessions: { groups: [{ group: "working", members: [0, 1, 2] }] },
  };
  const checkout = { id: "main", workspace_id: "project", label: "main", branch: "main", path: "/fixture", agent_scope: scope, tabs: [{ id: "tab", panes: agents.map((agent) => ({ id: agent.pane_id })) }] } as unknown as Checkout;
  const project = { id: "project", device_id: "local", label: "프로젝트", path: "/fixture", is_git: true, agent_scope: scope, checkouts: [checkout] } as Workspace;
  const rest = { navigator: { devices: [{ id: "local", kind: "local", label: "Local", agent_scope: scope }], focused_device_id: "local", focused_checkout_id: "main", workspaces: [project], agents } } as unknown as SnapshotRest;
  return { rest, agents };
}

// docs/UI_BEHAVIOR.md, Sessions: the task list: the row standing for the focus is selected as in the sidebar, and the
// list brings it into view when it changes, by the sidebar's rule.
it("brings the Sessions row standing for the focus into view when it changes, and only then", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  // jsdom lays nothing out: the list is a 100-high box, and every row stands where `rowTop` says.
  let rowTop = 500;
  const rect = (top: number, height: number) => ({ top, bottom: top + height, left: 0, right: 0, width: 0, height, x: 0, y: top, toJSON() {} }) as DOMRect;
  const layout = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    return this.matches("[data-session-list]") ? rect(0, 100) : rect(rowTop, 28);
  });
  const scrolled: Element[] = [];
  const scrollIntoView = Element.prototype.scrollIntoView;
  Element.prototype.scrollIntoView = function (this: Element) { scrolled.push(this); };
  const shell = useShellStore.getState();
  const ui = useUiStore.getState();
  const focus = async (pane: string) => {
    const { rest, agents } = catalog();
    await act(async () => useShellStore.setState({ rest, agents, focusedPaneId: pane }));
  };
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const sessionRow = (pane: string) => container.querySelector(`[data-session-row="${pane}"]`);
  const rowButton = (pane: string) => sessionRow(pane)?.querySelector('[data-session-focus="row"]');
  try {
    useUiStore.setState({ screen: { kind: "workspace" }, overviewOpen: false });
    const first = catalog();
    useShellStore.setState({ rest: first.rest, agents: first.agents, connection: "live", focusedPaneId: "first" });
    await act(async () => root.render(<TooltipProvider><AgentSessions actions={createActions(() => true)} /></TooltipProvider>));

    // The focused root's row is selected and brought into view whole, its second line with its title.
    expect(rowButton("first")?.getAttribute("aria-current")).toBe("true");
    expect(scrolled).toEqual([sessionRow("first")]);

    // A snapshot that keeps the focus does not scroll a list the operator may have scrolled.
    await focus("first");
    expect(scrolled).toHaveLength(1);

    // A delegated child the closed tree hides: its parent's row carries the selection and is brought in.
    await focus("child");
    expect(container.querySelector('[data-session-child="child"]')).toBeNull();
    expect(rowButton("parent")?.getAttribute("aria-current")).toBe("true");
    expect(scrolled.at(-1)).toBe(sessionRow("parent"));

    // Another screen in front and back, the focus where it was: the list stays where the operator left it.
    const revealedSoFar = scrolled.length;
    await act(async () => useUiStore.setState({ screen: { kind: "main" } }));
    expect(container.querySelector('[aria-current="true"]')).toBeNull();
    await act(async () => useUiStore.setState({ screen: { kind: "workspace" } }));
    expect(rowButton("parent")?.getAttribute("aria-current")).toBe("true");
    expect(scrolled).toHaveLength(revealedSoFar);

    // A row already in view stays where it is when the focus moves to it.
    rowTop = 40;
    await focus("second");
    expect(rowButton("second")?.getAttribute("aria-current")).toBe("true");
    expect(scrolled).toHaveLength(revealedSoFar);
  } finally {
    await act(async () => root.unmount());
    container.remove();
    layout.mockRestore();
    Element.prototype.scrollIntoView = scrollIntoView;
    useShellStore.setState(shell, true);
    useUiStore.setState(ui, true);
    vi.unstubAllGlobals();
  }
});
