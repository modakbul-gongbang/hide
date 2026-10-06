// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { createActions } from "../actions";
import { TooltipProvider } from "../components/ui/tooltip";
import type { Device, Kit, KitAgent, KitAgentSessions, KitFeatureId, KitPiece } from "../snapshot";
import { useShellStore } from "../store";
import { useUiStore } from "../ui";
import type { DispatchFn } from "../ws";
import { AgentsTab } from "./AgentsTab";

// The shell's modules reach xterm, which asks jsdom for a canvas it lacks.
vi.hoisted(() => {
  HTMLCanvasElement.prototype.getContext = () => null;
});

const piece = (state: KitPiece["state"], reason: string | null = null): KitPiece => ({ state, reason, location: null });
const LABELS: Record<string, string> = { "claude-code": "Claude Code", codex: "Codex", "gemini-cli": "Gemini CLI", grok: "Grok", opencode: "OpenCode", pi: "Pi", cursor: "Cursor" };
const FEATURES: KitFeatureId[] = ["skill", "guidance", "letters", "memory", "subagents", "herdr_integration", "sleep", "fork", "start", "titles"];

const agent = (id: string, over: Partial<KitAgent> = {}): KitAgent => ({
  id,
  label: LABELS[id]!,
  availability: "available",
  enabled: true,
  skill: piece("installed"),
  hook: piece("installed"),
  herdr: piece("installed"),
  doc_url: `https://docs.example.test/${id}`,
  ...over,
});
const sessions = (connected: number, not: KitAgentSessions["not_connected"] = [], hidden = 0): KitAgentSessions => ({ connected, not_connected: not, not_connected_hidden: hidden });
// What a Partial agent does in the fixture: the skill and the subagent count, and Herdr's own status where it has an integration.
const supports = (id: string, feature: KitFeatureId) => ["skill", "subagents"].includes(feature) || (feature === "herdr_integration" && id !== "gemini-cli");
const partial = (id: string, over: Partial<KitAgent> = {}): KitAgent =>
  agent(id, {
    enabled: false,
    skill: piece("off"),
    hook: null,
    herdr: id === "gemini-cli" ? null : piece("off"),
    partial: true,
    features: FEATURES.map((feature) => ({ id: feature, supported: supports(id, feature) })),
    sessions: null,
    ...over,
  });

const SEVEN = [
  agent("claude-code", { sessions: sessions(2) }),
  agent("codex", { sessions: sessions(0) }),
  partial("gemini-cli"),
  partial("grok"),
  partial("opencode", { availability: "not_installed" }),
  partial("pi", { availability: "not_installed" }),
  partial("cursor", { availability: "not_installed" }),
];

const kit = (agents: KitAgent[], over: Partial<Kit> = {}): Kit => ({ unavailable: null, busy: false, components: [], agents, offers_reinstall: false, shares_account_with: null, ...over });
const device = (id: string, over: Partial<Device> = {}): Device => ({ id, label: id === "local" ? "mini" : "Studio", kind: id === "local" ? "local" : "remote", state: id === "local" ? "local" : "ready", message: null, ssh_alias: id === "local" ? null : id, agent_count: 0, test: null, kit: kit(SEVEN), ...over });

const state = (devices: Device[]) => ({
  connection: "live" as const,
  rest: { navigator: { focused_device_id: "local", devices }, status: {}, ui_state: {} },
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
    root.render(<TooltipProvider><AgentsTab actions={actions} /></TooltipProvider>);
  });
  const q = (selector: string) => container.querySelector(selector) as HTMLElement | null;
  const all = (selector: string) => [...container.querySelectorAll(selector)] as HTMLElement[];
  const click = async (element: HTMLElement | null) => { await act(async () => { element?.click(); }); };
  return { events, q, all, click, text: () => container.textContent ?? "", unmount: async () => { await act(async () => root.unmount()); useShellStore.setState(saved, true); } };
}

const sent = (events: Parameters<DispatchFn>[0][], kind: string) => events.filter((event) => event.kind === kind);

it("lists the installed agents in the supported order with a switch each, and folds the rest under Not installed with an install link (B8, B9, B12)", async () => {
  const { q, all, text, unmount } = await mount(state([device("local")]));
  expect(all("[data-agent-row]").map((row) => row.getAttribute("data-agent-row"))).toEqual([
    "local:claude-code:on",
    "local:codex:on",
    "local:gemini-cli:off",
    "local:grok:off",
    "opencode:not-installed",
    "pi:not-installed",
    "cursor:not-installed",
  ]);
  expect(text()).toContain("Installed 4");
  expect(text()).toContain("Not installed 3");
  // Folded until opened; no subtitle on any row.
  expect(q("[data-agents-not-installed]")?.hasAttribute("open")).toBe(false);
  expect(text()).not.toMatch(/Skill and session hook|Skill only/);
  expect(all("[data-agent-install]").map((link) => link.getAttribute("href"))).toEqual([
    "https://opencode.ai/docs/",
    "https://pi.dev/",
    "https://cursor.com/docs/cli/installation",
  ]);
  // Docs is on each installed row (shown on hover or focus) and points at the kit's page.
  expect(q('[data-agent-docs="codex"]')?.getAttribute("href")).toBe("https://docs.example.test/codex");
  // One machine: no switch at the top.
  expect(q("[data-agents-machines]")).toBeNull();
  await unmount();
});

it("shows a status only for an agent that is on: Ready, connected, not connected, and nothing for Partial agents (B16, B19)", async () => {
  const set = [
    agent("claude-code", { sessions: sessions(2) }),
    agent("codex", { sessions: sessions(0) }),
    partial("gemini-cli", { enabled: true, sessions: null }),
    partial("grok", { sessions: sessions(3) }),
  ];
  const { q, unmount } = await mount(state([device("local", { kit: kit(set) })]));
  expect(q('[data-agent-status="local:claude-code:connected"]')?.textContent).toContain("2 connected");
  expect(q('[data-agent-status="local:codex:ready"]')?.textContent).toContain("Ready");
  // Partial: the chip only, on or off, never a count.
  expect(q('[data-agent-partial="gemini-cli"]')).not.toBeNull();
  expect(q('[data-agent-partial="grok"]')).not.toBeNull();
  expect(q('[data-agent-row="local:grok:off"] [data-agent-status]')).toBeNull();
  expect(q('[data-agent-row="local:gemini-cli:on"] [data-agent-status]')).toBeNull();
  // A Full agent never wears the chip.
  expect(q('[data-agent-partial="codex"]')).toBeNull();
  await unmount();
});

it("expands the sessions that run without Hide, counts the ones past the list, and goes to a pane (B17, B31)", async () => {
  const set = [
    agent("codex", {
      sessions: sessions(1, [{ pane_id: "w1:p3", title: "Run overnight task safely", project: "browser-control", reason: "codex_shared_server" }], 13),
    }),
  ];
  const { q, click, events, text, unmount } = await mount(state([device("local", { kit: kit(set) })]));
  const chip = q('[data-agent-status="local:codex:not-connected"]')!;
  expect(chip.textContent).toContain("14 not connected");
  expect(chip.getAttribute("aria-expanded")).toBe("false");
  expect(q("[data-agent-sessions]")).toBeNull();
  await click(chip);
  expect(chip.getAttribute("aria-expanded")).toBe("true");
  expect(q("[data-agent-session='w1:p3']")?.textContent).toContain("Run overnight task safely");
  expect(q("[data-agent-session='w1:p3']")?.textContent).toContain("browser-control · w1:p3");
  expect(q("[data-agent-sessions-more]")?.textContent).toBe("+13 more");
  // Settings has no Reopen: that is the pane's own chip (B31).
  expect(text()).not.toContain("Reopen");
  useUiStore.getState().openOverlay("settings");
  await click(q("[data-agent-go-to-pane='w1:p3']"));
  expect(sent(events, "focus_pane")).toHaveLength(1);
  expect(sent(events, "focus_pane")[0]?.payload).toMatchObject({ pane_id: "w1:p3" });
  // The pane shows behind the sheet, so the sheet closes.
  expect(useUiStore.getState().overlay).not.toBe("settings");
  await unmount();
});

it("shows a failed or removed part on its own row with Reinstall, and nowhere else (B13, B20)", async () => {
  const set = [agent("claude-code", { hook: piece("removed"), herdr: piece("failed", "herdr refused") }), agent("codex")];
  const { q, all, click, events, unmount } = await mount(state([device("local", { kit: kit(set) })]));
  const line = q('[data-agent-problem="local:claude-code"]')!;
  expect(line.textContent).toContain("Herdr integration: Failed: herdr refused");
  expect(line.textContent).toContain("Hook: Removed");
  expect(all("[data-agent-problem]")).toHaveLength(1);
  await click(q('[data-hook-reinstall="local:claude-code"]'));
  expect(sent(events, "kit_reinstall")[0]?.payload).toEqual({ device_id: "local", components: [], agents: ["claude-code"] });
  await unmount();
});

it("turns an agent on or off on the machine the list is for, and not while the machine installs (B13, B67)", async () => {
  const { q, click, events, unmount } = await mount(state([device("local"), device("studio")]));
  await click(q('[data-agent-switch="local:gemini-cli:off"]'));
  expect(sent(events, "kit_agent_set")[0]?.payload).toEqual({ device_id: "local", agent: "gemini-cli", enabled: true });
  await unmount();

  const busy = await mount(state([device("local", { kit: kit(SEVEN, { busy: true }) })]));
  expect(busy.q('[data-agent-switch="local:gemini-cli:off"]')?.hasAttribute("disabled")).toBe(true);
  await busy.unmount();
});

it("lists one machine at a time under a switch that appears only with a device, and shows an unreachable device as one line with Try again (B11)", async () => {
  const studio = device("studio", { kit: kit([agent("claude-code", { sessions: sessions(0) }), agent("codex", { availability: "not_installed", enabled: false })]) });
  const { q, all, click, unmount } = await mount(state([device("local"), studio]));
  expect(all("[data-agents-machine]").map((item) => item.textContent)).toEqual(["This Mac", "Studio"]);
  expect(all("[data-agent-row]")).toHaveLength(7);
  await click(q('[data-agents-machine="studio"]'));
  expect(all("[data-agent-row]").map((row) => row.getAttribute("data-agent-row"))).toEqual(["studio:claude-code:on", "codex:not-installed"]);
  await unmount();

  const down = await mount(state([device("local"), { ...studio, state: "unavailable" }]));
  await click(down.q('[data-agents-machine="studio"]'));
  expect(down.q("[data-agents-unreachable]")?.textContent).toBe("Studio can't be reached right now.");
  expect(down.all("[data-agent-row]")).toHaveLength(0);
  await click(down.q('[data-agents-retry="studio"]'));
  expect(sent(down.events, "retry_connect")[0]?.payload).toEqual({ target_id: "studio" });
  await down.unmount();
});

it("Check again reads the machine and rests as it was; a machine whose kit cannot run says why while its list stays (B10)", async () => {
  const { q, click, events, unmount } = await mount(state([device("local", { kit: kit(SEVEN, { unavailable: "A daemon outside the package installs nothing" }) })]));
  expect(q("[data-agents-unavailable]")?.textContent).toBe("A daemon outside the package installs nothing");
  expect(q('[data-agent-row="local:claude-code:on"]')).not.toBeNull();
  expect(q("[data-agents-check]")?.getAttribute("data-agents-check")).toBe("idle");
  // The tab reads the kit once as it opens (the demand hook); the button asks again.
  const opened = sent(events, "kit_check").length;
  await click(q("[data-agents-check]"));
  expect(sent(events, "kit_check")).toHaveLength(opened + 1);
  expect(q("[data-agents-check]")?.getAttribute("data-agents-check")).toBe("checking");
  expect(q("[data-agents-check]")?.hasAttribute("disabled")).toBe(true);
  await unmount();
});

it("opens the Partial popover from the chip, lists every feature with a mark and a word, and says how Gemini CLI's status is judged (B15, B18)", async () => {
  const { q, click, unmount } = await mount(state([device("local")]));
  await click(q('[data-agent-partial="gemini-cli"]'));
  const popover = document.body.querySelector('[data-agent-partial-popover="gemini-cli"]')!;
  const lines = [...popover.querySelectorAll("[data-agent-feature]")].map((line) => line.getAttribute("data-agent-feature"));
  expect(lines).toEqual(FEATURES.map((feature) => `${feature}:${supports("gemini-cli", feature) ? "yes" : "no"}`));
  expect(popover.querySelector('[data-agent-feature="letters:no"]')?.textContent).toContain("–Not available: Letters and Observer warnings");
  expect(popover.querySelector('[data-agent-feature="skill:yes"]')?.textContent).toContain("✓Works: Hide skill");
  expect(popover.textContent).toContain("Herdr has no integration for Gemini CLI, so Hide judges its status from the screen.");
  await unmount();

  // An agent with a Herdr integration does not carry that sentence.
  const grok = await mount(state([device("local")]));
  await grok.click(grok.q('[data-agent-partial="grok"]'));
  expect(document.body.querySelector('[data-agent-partial-popover="grok"]')?.textContent).not.toContain("judges its status from the screen");
  await grok.unmount();
});
