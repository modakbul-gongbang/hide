// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { createActions } from "./actions";
import { TooltipProvider } from "./components/ui/tooltip";
import { DiskCleanupSheet } from "./DiskCleanupSheet";
import type { Workspace } from "./snapshot";
import { useShellStore } from "./store";

// The shell's modules reach xterm, which asks jsdom for a canvas it lacks.
vi.hoisted(() => {
  HTMLCanvasElement.prototype.getContext = () => null;
});

const project = (id: string, phase: string | null) =>
  ({
    agent_scope: { has_working: false },
    id,
    label: id,
    path: `/${id}`,
    device_id: "local",
    is_git: true,
    registered: true,
    temporary: false,
    pinned: false,
    checkouts: [],
    inactive_checkouts: { expanded: false, checkout_ids: [] },
    cleanup: phase
      ? { id: 7, workspace_id: id, repository_root: `/${id}`, phase, main_head: null, message: null, usage_error: null, usage_ready: true, free_bytes: null, progress: null, cell_results: [], free_before: null, free_after: null, rows: [] }
      : null,
  }) as unknown as Workspace;

/** This machine's project `mini`, and a MacBook node whose project `app` is in `phase`. */
const state = (phase: string) => ({
  connection: "live" as const,
  rest: {
    navigator: { workspaces: [project("mini", null)] },
    status: { remote: [{ target_id: "mbp", state: "connected", session: { workspaces: [project("app", phase)] } }] },
    ui_state: {},
  },
});

afterEach(() => {
  document.body.innerHTML = "";
});

it("waits on a node project's cleanup, and reviews once it ends (B20)", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} unobserve() {} });
  const reviewed: string[] = [];
  const actions = createActions(() => true);
  actions.reviewDiskCleanup = (id: string) => { reviewed.push(id); };
  const root = createRoot(document.body.appendChild(document.createElement("div")));
  const saved = useShellStore.getState();
  const mini = project("mini", null);
  await act(async () => {
    useShellStore.setState(state("removing") as never);
    root.render(<TooltipProvider><DiskCleanupSheet actions={actions} workspace={mini} filter="all" onClose={() => {}} /></TooltipProvider>);
  });
  // The core would refuse a review while the node's cleanup runs, and leave this sheet reviewing for good.
  expect(document.querySelector("[data-disk-sheet=mini]")?.getAttribute("data-disk-state")).toBe("busy");
  expect(reviewed).toEqual([]);
  await act(async () => { useShellStore.setState(state("complete") as never); });
  expect(reviewed).toEqual(["mini"]);
  await act(async () => root.unmount());
  useShellStore.setState(saved, true);
});
