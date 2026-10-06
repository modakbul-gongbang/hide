// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { createActions } from "./actions";
import { AgentOnboardingGate } from "./AgentOnboarding";
import { TooltipProvider } from "./components/ui/tooltip";
import type { KitAgent } from "./snapshot";
import { useShellStore } from "./store";
import type { DispatchFn } from "./ws";

// The shell's modules reach xterm, which asks jsdom for a canvas it lacks.
vi.hoisted(() => {
  HTMLCanvasElement.prototype.getContext = () => null;
});

const agent = (id: string, label: string, availability: KitAgent["availability"]): KitAgent => ({
  id,
  label,
  availability,
  enabled: false,
  skill: { state: "off", reason: null, location: null },
  hook: null,
  doc_url: "https://example.test",
});

const state = (pending: boolean, agents: KitAgent[]) => ({
  connection: "live" as const,
  rest: { ui_state: { agent_onboarding: pending ? "pending" : "done" }, navigator: { devices: [{ id: "local", kit: { agents } }] } },
});

afterEach(() => {
  document.body.innerHTML = "";
});

async function mount(next: ReturnType<typeof state>) {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} unobserve() {} });
  const events: Parameters<DispatchFn>[0][] = [];
  const actions = createActions((event) => { events.push(event); return true; });
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const saved = useShellStore.getState();
  await act(async () => {
    useShellStore.setState(next as never);
    root.render(<TooltipProvider><AgentOnboardingGate actions={actions} /></TooltipProvider>);
  });
  return { events, unmount: async () => { await act(async () => root.unmount()); useShellStore.setState(saved, true); } };
}

const AGENTS = [agent("claude-code", "Claude Code", "available"), agent("cursor", "Cursor", "not_installed"), agent("codex", "Codex", "available")];

it("shows every agent as a tile, the set-up ones on and the others dimmed with no switch, and applies what is left on", async () => {
  const { events, unmount } = await mount(state(true, AGENTS));
  const tile = (id: string, on: string) => document.querySelector(`[data-onboarding-tile="${id}:${on}"]`);
  expect(tile("claude-code", "on")).not.toBeNull();
  expect(tile("codex", "on")).not.toBeNull();
  expect(tile("cursor", "unavailable")).not.toBeNull();
  expect(document.querySelector('[data-onboarding-tile="cursor:unavailable"]')?.getAttribute("role")).toBeNull();
  // A logo or a monogram, never nothing: Cursor has a bundled mark, an agent without one a monogram.
  expect(document.querySelectorAll("[data-agent-logo], [data-agent-monogram]").length).toBe(3);

  await act(async () => { (tile("codex", "on") as HTMLElement).click(); });
  expect(tile("codex", "off")?.getAttribute("aria-checked")).toBe("false");
  await act(async () => { (document.querySelector("[data-onboarding-apply]") as HTMLElement).click(); });

  expect(events).toEqual([{ schema_version: 2, kind: "agent_onboarding_apply", payload: { agents: ["claude-code"] } }]);
  await unmount();
});

it("has Apply as its only way out: Escape and a second button do nothing (the outside click is the e2e's), and it is not shown once decided", async () => {
  const shown = await mount(state(true, AGENTS));
  expect(document.querySelector("[data-onboarding-later]")).toBeNull();
  expect(document.querySelectorAll("[data-agent-onboarding] button:not([role=switch])").length).toBe(1);
  await act(async () => { document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); });
  expect(document.querySelector("[data-agent-onboarding]")).not.toBeNull();
  expect(shown.events).toEqual([]);
  await shown.unmount();

  const decided = await mount(state(false, AGENTS));
  expect(document.querySelector("[data-agent-onboarding]")).toBeNull();
  await decided.unmount();

  const empty = await mount(state(true, []));
  expect(document.querySelector("[data-agent-onboarding]")).toBeNull();
  await empty.unmount();
});

it("draws what Apply sends: an agent that becomes available while it is open shows on, and the operator's flips survive the refresh", async () => {
  const { events, unmount } = await mount(state(true, AGENTS));
  const tile = (id: string, on: string) => document.querySelector(`[data-onboarding-tile="${id}:${on}"]`);
  await act(async () => { (tile("codex", "on") as HTMLElement).click(); });
  // A later poll finds Cursor set up on the machine.
  await act(async () => {
    useShellStore.setState(state(true, AGENTS.map((entry) => (entry.id === "cursor" ? { ...entry, availability: "available" as const } : entry))) as never);
  });
  expect(tile("cursor", "on")).not.toBeNull();
  expect(tile("codex", "off")).not.toBeNull();
  await act(async () => { (document.querySelector("[data-onboarding-apply]") as HTMLElement).click(); });
  expect(events).toEqual([{ schema_version: 2, kind: "agent_onboarding_apply", payload: { agents: ["claude-code", "cursor"] } }]);
  await unmount();
});
