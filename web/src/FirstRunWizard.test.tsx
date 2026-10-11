// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { createActions } from "./actions";
import { FirstRunWizard, useSetupWalk } from "./FirstRunWizard";
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
  chosen: false,
  skill: { state: "off", reason: null, location: null },
  hook: null,
  doc_url: "https://example.test",
});

const provider = (id: string, label: string, agentId: string, ready: boolean) => ({ id, label, agent: agentId, state: ready ? "ready" : "needs_login", selectable: ready });

/** `pending`: a first run on its agent step; otherwise an install whose wizard ended. */
const state = (pending: boolean, agents: KitAgent[], providers: ReturnType<typeof provider>[] = [], kit: { busy?: boolean } = {}, revision = 1) => ({
  connection: "live" as const,
  revision,
  rest: {
    ui_state: { agent_onboarding: pending ? "pending" : "done", first_run: { version: 1, step: pending ? "agents" : "done" } },
    navigator: { devices: [{ id: "local", kit: { agents, components: [{ id: "cli", label: "hide", state: "off", reason: null, location: "/Users/me/.local/bin/hide" }], busy: kit.busy ?? false } }] },
    status: { background_ai: { provider: null, chosen: false, providers } },
  },
});

/** What the gate sent for the operator's choice: the sign-in probe it asks for while open is not part of it. */
const choices = (events: Parameters<DispatchFn>[0][]) => events.filter((event) => event.kind !== "ai_settings");

const tile = (id: string, on: string) => document.querySelector(`[data-onboarding-tile="${id}:${on}"]`) as HTMLElement | null;
const continueButton = () => document.querySelector("[data-onboarding-apply]") as HTMLButtonElement;

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
    root.render(<TooltipProvider><FirstRunWizard actions={actions} /></TooltipProvider>);
  });
  return { events, unmount: async () => { await act(async () => root.unmount()); useShellStore.setState(saved, true); } };
}

const AGENTS = [agent("claude-code", "Claude Code", "available"), agent("cursor", "Cursor", "not_installed"), agent("codex", "Codex", "available")];

it("shows every agent as a tile, the set-up ones on and the others dimmed with an install link, and continues with what is left on", async () => {
  const { events, unmount } = await mount(state(true, AGENTS));
  expect(tile("claude-code", "on")).not.toBeNull();
  expect(tile("codex", "on")).not.toBeNull();
  expect(tile("cursor", "unavailable")).not.toBeNull();
  expect(tile("cursor", "unavailable")?.getAttribute("role")).toBeNull();
  // B9: the official install page of an agent that is not installed.
  expect(document.querySelector('[data-onboarding-install="cursor"]')?.getAttribute("href")).toMatch(/^https:\/\//);
  // A logo or a monogram, never nothing: Cursor has a bundled mark, an agent without one a monogram.
  expect(document.querySelectorAll("[data-agent-logo], [data-agent-monogram]").length).toBe(3);

  await act(async () => { tile("codex", "on")!.click(); });
  expect(tile("codex", "off")?.getAttribute("aria-checked")).toBe("false");
  await act(async () => { continueButton().click(); });

  // A browser tab's wizard is this step alone, so Continue ends it there (B26).
  expect(choices(events)).toEqual([{ schema_version: 2, kind: "agent_onboarding_apply", payload: { agents: ["claude-code"], finish: true } }]);
  await unmount();
});

it("holds Continue while its pass runs, and names a failed agent with its reason after it (B10, B12)", async () => {
  const { events, unmount } = await mount(state(true, AGENTS));
  await act(async () => { continueButton().click(); });
  // Held until the snapshot that takes the press, then while the kit is busy.
  expect(continueButton().disabled).toBe(true);
  await act(async () => { continueButton().click(); });
  await act(async () => { useShellStore.setState(state(true, AGENTS, [], { busy: true }, 2) as never); });
  expect(continueButton().disabled).toBe(true);
  const failed = AGENTS.map((entry) => (entry.id === "codex" ? { ...entry, enabled: true, skill: { state: "failed" as const, reason: "permission denied", location: "/x" } } : { ...entry, enabled: entry.availability === "available" }));
  await act(async () => { useShellStore.setState(state(true, failed, [], { busy: false }, 3) as never); });
  expect(continueButton().disabled).toBe(false);
  expect(document.querySelector('[data-first-run-failed="codex"]')?.textContent).toBe("Codex could not be set up: permission denied");
  expect(continueButton().textContent).toBe("Try again");
  expect(choices(events).filter((event) => event.kind === "agent_onboarding_apply")).toHaveLength(1);
  await unmount();
});

it("lists under What changes where Hide writes for each agent left on, and the hide command's link (B11)", async () => {
  const located = AGENTS.map((entry) => ({ ...entry, skill: { state: "off" as const, reason: null, location: `/Users/me/.${entry.id}/skills/hide` } }));
  const { unmount } = await mount(state(true, located));
  await act(async () => { (document.querySelector("[data-first-run-what-changes]") as HTMLElement).click(); });
  const rows = () => [...document.querySelectorAll("[data-first-run-change]")].map((row) => row.getAttribute("data-first-run-change"));
  expect(rows()).toEqual(["claude-code", "codex", "cli"]);
  expect(document.querySelector('[data-first-run-change="codex"]')?.textContent).toContain("/Users/me/.codex/skills/hide");
  await act(async () => { tile("codex", "on")!.click(); });
  expect(rows()).toEqual(["claude-code", "cli"]);
  await unmount();
});

it("reopened from Help, starts from the agents switched on now, closes with Escape, and sends only a Continue (B45, B46)", async () => {
  const enabled = AGENTS.map((entry) => ({ ...entry, enabled: entry.id === "codex" }));
  const { events, unmount } = await mount(state(false, enabled));
  expect(document.querySelector("[data-first-run]")).toBeNull();
  await act(async () => { useSetupWalk.getState().open(); });
  expect(document.querySelector("[data-first-run-mode]")?.getAttribute("data-first-run-mode")).toBe("setup");
  expect(tile("codex", "on")).not.toBeNull();
  expect(tile("claude-code", "off")).not.toBeNull();
  await act(async () => { document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); });
  expect(document.querySelector("[data-first-run]")).toBeNull();
  expect(choices(events)).toEqual([]);

  await act(async () => { useSetupWalk.getState().open(); });
  await act(async () => { continueButton().click(); });
  expect(choices(events)).toEqual([{ schema_version: 2, kind: "agent_onboarding_apply", payload: { agents: ["codex"], finish: false } }]);
  expect(useSetupWalk.getState().step).toBe("notifications");
  await act(async () => { useSetupWalk.getState().close(); });
  await unmount();
});

it("has Continue as its only way out: Escape and a second button do nothing (the outside click is the e2e's), and it is not shown once decided", async () => {
  const shown = await mount(state(true, AGENTS));
  expect(document.querySelector("[data-onboarding-later]")).toBeNull();
  expect(document.querySelectorAll("[data-first-run] button:not([role=switch]):not([data-first-run-what-changes])").length).toBe(1);
  await act(async () => { document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); });
  expect(document.querySelector("[data-first-run]")).not.toBeNull();
  expect(choices(shown.events)).toEqual([]);
  await shown.unmount();

  const decided = await mount(state(false, AGENTS));
  expect(document.querySelector("[data-first-run]")).toBeNull();
  await decided.unmount();

  const empty = await mount(state(true, []));
  expect(document.querySelector("[data-first-run]")).toBeNull();
  await empty.unmount();
});

it("draws what Continue sends: an agent that becomes available while it is open shows on, and the operator's flips survive the refresh", async () => {
  const { events, unmount } = await mount(state(true, AGENTS));
  await act(async () => { tile("codex", "on")!.click(); });
  // A later poll finds Cursor set up on the machine.
  await act(async () => {
    useShellStore.setState(state(true, AGENTS.map((entry) => (entry.id === "cursor" ? { ...entry, availability: "available" as const } : entry))) as never);
  });
  expect(tile("cursor", "on")).not.toBeNull();
  expect(tile("codex", "off")).not.toBeNull();
  await act(async () => { continueButton().click(); });
  expect(choices(events)).toEqual([{ schema_version: 2, kind: "agent_onboarding_apply", payload: { agents: ["claude-code", "cursor"], finish: true } }]);
  await unmount();
});

it("names under Continue the agent Hide AI will use, and says on each checked tile whether it is signed in (B8, B13)", async () => {
  const providers = [provider("claude", "Claude Code", "claude-code", true), provider("codex", "Codex", "codex", true)];
  const { unmount } = await mount(state(true, AGENTS, providers));
  expect(tile("claude-code", "on")?.querySelector("[data-onboarding-status]")?.getAttribute("data-onboarding-status")).toBe("ready");
  const line = () => document.querySelector("[data-onboarding-hide-ai]")?.textContent ?? null;
  expect(line()).toBe("Hide AI uses Claude Code");
  await act(async () => { tile("claude-code", "on")!.click(); });
  expect(line()).toBe("Hide AI uses Codex");
  await act(async () => { tile("codex", "on")!.click(); });
  expect(line()).toBeNull();
  await unmount();

  // A switched-on agent that is not signed in is passed over; with nobody signed in there is no line.
  const signedOut = await mount(state(true, AGENTS, [provider("claude", "Claude Code", "claude-code", false), provider("codex", "Codex", "codex", false)]));
  expect(document.querySelector("[data-onboarding-hide-ai]")).toBeNull();
  expect(tile("codex", "on")?.querySelector("[data-onboarding-status]")?.textContent).toBe("Not signed in");
  await signedOut.unmount();
});

it("asks for the sign-in probe while it is open and stops when it goes away", async () => {
  const { events, unmount } = await mount(state(true, AGENTS));
  expect(events.filter((event) => event.kind === "ai_settings")).toEqual([{ schema_version: 2, kind: "ai_settings", payload: { observing: true } }]);
  await unmount();
  expect(events.filter((event) => event.kind === "ai_settings").at(-1)).toEqual({ schema_version: 2, kind: "ai_settings", payload: { observing: false } });
});
