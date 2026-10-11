// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { createActions } from "./actions";
import { TooltipProvider } from "./components/ui/tooltip";
import { NODE_DEVICE } from "./gallery/coreMoveSceneData";
import { NODE_PROJECT, nodeProjectRest } from "./gallery/nodeProjectSceneData";
import { ProjectOverview } from "./ProjectOverview";
import { useShellStore } from "./store";
import { entryLens, useUiStore } from "./ui";
import { WorkspaceDialogs } from "./WorkspaceDialogs";
import type { DispatchFn } from "./ws";

// The shell's modules reach xterm, which asks jsdom for a canvas it lacks.
vi.hoisted(() => {
  HTMLCanvasElement.prototype.getContext = () => null;
});

type Sent = Parameters<DispatchFn>[0];

afterEach(() => {
  document.body.innerHTML = "";
});

/** The Overview of the MacBook's project in the MacBook's window, as the `node-project` scene draws it, with every event it sends. */
async function open({ rest, agents }: ReturnType<typeof nodeProjectRest>) {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} unobserve() {} });
  const events: Sent[] = [];
  const actions = createActions((event) => { events.push(event); return true; });
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const shell = useShellStore.getState();
  const ui = useUiStore.getState();
  const lens = { ...entryLens(null, "board"), tab: "issues" as const };
  await act(async () => {
    useShellStore.setState({ rest, agents, connection: "live" });
    useUiStore.setState({ overviewProjectId: NODE_PROJECT, overviewLens: lens, workspaceDialog: null });
    root.render(
      <TooltipProvider>
        <ProjectOverview projectId={NODE_PROJECT} lens={lens} actions={actions} />
        <WorkspaceDialogs actions={actions} />
      </TooltipProvider>,
    );
  });
  const q = (selector: string) => document.body.querySelector<HTMLElement>(selector);
  const click = async (selector: string) => {
    const element = q(selector);
    if (!element) throw new Error(`${selector} is not drawn`);
    await act(async () => element.click());
  };
  const unmount = async () => {
    await act(async () => root.unmount());
    useShellStore.setState(shell, true);
    useUiStore.setState(ui, true);
  };
  return { events, q, click, unmount };
}

const named = (events: Sent[], kind: string) => events.filter((event) => event.kind === kind).map((event) => (event.payload as { workspace_id?: string }).workspace_id);

it("reads a node's project, makes its issue and reviews its cleanup by the node project's id (D-29)", async () => {
  const { events, q, click, unmount } = await open(nodeProjectRest("local", false, "reference", Date.now()));
  expect(named(events, "overview_refresh")).toEqual([NODE_PROJECT]);
  expect(q("[data-overview-screen]")?.getAttribute("data-overview-view")).toBe("issues");
  expect([...document.body.querySelectorAll("[data-task-key]")].map((card) => card.getAttribute("data-task-key"))).toEqual(["local:/srv/web#2", "local:/srv/web#1"]);

  await click("[data-overview-new-issue]");
  const title = q("[data-new-issue-title]") as HTMLInputElement;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(title, "Show the core's machine in the strip");
    title.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await click("[data-new-issue-create]");
  expect(events.filter((event) => event.kind === "issue_create").map((event) => event.payload)).toEqual([{ workspace_id: NODE_PROJECT, title: "Show the core's machine in the strip", body: "" }]);

  await act(async () => useUiStore.getState().setWorkspaceDialog(null));
  await click("[data-disk-entrance]");
  expect(named(events, "cleanup_review")).toEqual([NODE_PROJECT]);
  await unmount();
});

it("asks nothing of a dialed device's project and offers it no issue or cleanup", async () => {
  const fixture = structuredClone(nodeProjectRest("local", false, "reference", Date.now()));
  const rest = fixture.rest;
  rest.navigator!.devices = rest.navigator!.devices!.map((device) => (device.id === NODE_DEVICE ? { ...device, dials_in: false } : device));
  for (const project of rest.status!.remote!.flatMap((row) => row.session?.workspaces ?? [])) if (project.id === NODE_PROJECT) project.tasks = { source: null, tasks: [], overflow: false };
  const { events, q, unmount } = await open(fixture);
  expect(named(events, "overview_refresh")).toEqual([]);
  expect(q("[data-overview-new-issue]")).toBeNull();
  expect(q("[data-disk-entrance]")).toBeNull();
  await unmount();
});
