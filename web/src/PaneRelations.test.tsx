// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { createActions } from "./actions";
import { usePaneMenu } from "./PaneRelations";
import type { PaneRow } from "./snapshot";

// The shell's modules reach xterm, which asks jsdom for a canvas it lacks.
vi.hoisted(() => {
  HTMLCanvasElement.prototype.getContext = () => null;
});

afterEach(() => {
  document.body.innerHTML = "";
});

const row = (over: Partial<PaneRow> = {}) => ({ id: "w1:p2", children: null, lineage_path: [], ...over }) as unknown as PaneRow;

/** The menu a pane header opens, kept open while the pane's row and name change under it. */
async function openMenu(first: PaneRow, firstTitle: string) {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} unobserve() {} });
  const actions = createActions(() => true);
  let open: (x: number, y: number) => void = () => {};
  function Header({ pane, title }: { pane: PaneRow; title: string }) {
    const menu = usePaneMenu(pane, title, actions);
    open = menu.openAt;
    return menu.menu;
  }
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const render = (pane: PaneRow, title: string) => act(async () => root.render(<Header pane={pane} title={title} />));
  await render(first, firstTitle);
  await act(async () => open(10, 10));
  const labels = () => Array.from(document.querySelectorAll('[role="menuitem"]')).map((item) => item.textContent);
  return { render, labels, unmount: () => act(async () => root.unmount()) };
}

it("names the pane as its header does while the menu is open", async () => {
  const menu = await openMenu(row(), "Claude");
  expect(menu.labels()).toContain("Close pane Claude");
  // The core's label for the session arrives after the menu opened.
  await menu.render(row(), "Agent two");
  expect(menu.labels()).toContain("Close pane Agent two");
  expect(menu.labels()).not.toContain("Close pane Claude");
  await menu.unmount();
});

it("offers what the core now says about the pane while the menu is open", async () => {
  const menu = await openMenu(row({ fork: { available: false, reason: "This agent has not reported its conversation yet" } }), "Agent two");
  const fork = () => Array.from(document.querySelectorAll('[role="menuitem"]')).find((item) => item.textContent?.startsWith("Fork agent"));
  expect(fork()?.hasAttribute("data-disabled")).toBe(true);
  await menu.render(row({ fork: { available: true } }), "Agent two");
  expect(fork()?.hasAttribute("data-disabled")).toBe(false);
  await menu.unmount();
});
