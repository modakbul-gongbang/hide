import { emptyScope } from "../test/legacyAgentScope";
import { legacyAgentRow } from "../test/legacyAgentRow";
// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterAll, expect, it, vi } from "vitest";
import tokensText from "../../design/tokens.json?raw";
import { createActions } from "./actions";
import { TooltipProvider } from "./components/ui/tooltip";
import { Sidebar } from "./sidebar";
import type { AgentRow, SnapshotRest, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";

// This flow does not render a terminal. Answer xterm's canvas capability probe at the browser boundary, before the
// real sidebar imports it.
const browserCanvas = vi.hoisted(() => {
  const original = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = () => null;
  return { restore: () => { HTMLCanvasElement.prototype.getContext = original; } };
});
afterAll(() => browserCanvas.restore());

const LOCAL = { id: "local", label: "This Mac", kind: "local", state: "local", message: null };
const MINI = { id: "mini", label: "Mac mini", kind: "remote", state: "ready", message: null };

/** One project with one checkout whose only tab runs one Codex, on `device`, with pane ids as the core scopes them. */
function project(device: string): { workspace: Workspace; agent: AgentRow; checkoutId: string } {
  const scope = device === "local" ? "" : `remote:${device}:`;
  const checkoutId = `${scope}checkout:w3Y`;
  const paneId = `${scope}pane:w3Y:p7`;
  const workspace = { agent_scope: emptyScope(),
    id: `${scope}workspace:w3Y`, label: "modakbul", path: "/fixture/modakbul", device_id: device, is_git: true, registered: true,
    temporary: false, pinned: false, remote_target_id: device === "local" ? null : device,
    inactive_checkouts: { expanded: false, checkout_ids: [] },
    checkouts: [{ agent_scope: emptyScope(),
      id: checkoutId, workspace_id: `${scope}workspace:w3Y`, label: "modakbul", path: "/fixture/modakbul", branch: "main",
      is_primary: true, is_worktree: false, exists: true, active_tab_id: `${scope}tab:w3Y:t7`, strip: [],
      worktree: { branch: "main", head_sha: null, last_commit_unix_seconds: null },
      tabs: [{ id: `${scope}tab:w3Y:t7`, label: "Codex", panes: [{ id: paneId }] }],
    }],
  } as unknown as Workspace;
  const agent = legacyAgentRow({
    id: `${scope}agent:w3Y:p7`, pane_id: paneId, identity_label: "인사에 답하기", agent_kind: "codex", symbol: "○", group: "seen",
    status_code: "idle", changed_at_unix_ms: null, emphasized: false, unread: false, demand: "none", activity: "stopped",
  }) as AgentRow;
  return { workspace, agent, checkoutId };
}

// docs/UI_BEHAVIOR.md, Projects: both folds are this machine's, so a selected SSH device's tree is drawn with nothing
// folded, while this Mac's checkouts start closed until the operator opens one.
it("draws a selected SSH device's agent rows open under their checkout, with no fold, and this Mac's closed", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("ResizeObserver", class { observe() {} unobserve() {} disconnect() {} });
  // jsdom loads no stylesheet, and the sidebar reads its width tokens from the document: take them from the token source.
  const tokens = (JSON.parse(tokensText) as { tokens: Record<string, { value: number }> }).tokens;
  const widthTokens = ["--size-sidebar-ideal", "--size-sidebar-min", "--size-sidebar-max"];
  for (const name of widthTokens) document.documentElement.style.setProperty(name, `${tokens[name]?.value}px`);
  const shell = useShellStore.getState();
  const ui = useUiStore.getState();
  const remote = project("mini");
  const local = project("local");
  const device: SnapshotRest = {
    navigator: { devices: [LOCAL, MINI], workspaces: [], agents: [], focused_device_id: "mini" },
    status: {
      remote: [{
        target_id: "mini", state: "connected", message: null, herdr_version: "0.9.1",
        catalog: { state: "ready", refused: [] },
        session: {
          workspaces: [remote.workspace], agents: [remote.agent], active_tab_ids: {}, focused_workspace_id: null,
          focused_checkout_id: null, focused_tab_id: null, focused_pane_id: null, pane_layouts: [],
        },
      }],
    },
  } as unknown as SnapshotRest;
  const thisMac = {
    navigator: { devices: [LOCAL, MINI], workspaces: [local.workspace], agents: [local.agent], focused_device_id: "local" },
  } as unknown as SnapshotRest;
  useShellStore.setState({ rest: device, agents: [], connection: "live" });
  useUiStore.setState({ sidebarMode: "projects" });
  const actions = createActions(() => true);
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const open = (checkoutId: string) => container.querySelector(`[data-checkout-agents-open="${checkoutId}"]`);
  const toggle = (checkoutId: string) => container.querySelector(`[data-checkout-toggle="${checkoutId}"]`);
  try {
    await act(async () => root.render(<TooltipProvider><Sidebar actions={actions} /></TooltipProvider>));
    expect(open(remote.checkoutId)?.textContent).toContain("인사에 답하기");
    expect(toggle(remote.checkoutId)).toBeNull();

    await act(async () => useShellStore.setState({ rest: thisMac, agents: [local.agent] }));
    expect(open(local.checkoutId)).toBeNull();
    expect(toggle(local.checkoutId)?.getAttribute("aria-label")).toContain("main");
  } finally {
    await act(async () => root.unmount());
    container.remove();
    useShellStore.setState(shell, true);
    useUiStore.setState(ui, true);
    for (const name of widthTokens) document.documentElement.style.removeProperty(name);
    vi.unstubAllGlobals();
  }
});
