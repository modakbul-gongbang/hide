// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { createActions } from "../actions";
import { TooltipProvider } from "../components/ui/tooltip";
import type { AiProvider, BackgroundAi } from "../snapshot";
import { useShellStore } from "../store";
import type { DispatchFn } from "../ws";
import { HideAiTab } from "./HideAiTab";

// The shell's modules reach xterm, which asks jsdom for a canvas it lacks.
vi.hoisted(() => {
  HTMLCanvasElement.prototype.getContext = () => null;
});

const provider = (id: string, label: string, over: Partial<AiProvider> = {}): AiProvider => ({
  id,
  label,
  agent: id === "claude" ? "claude-code" : id,
  state: "ready",
  headline: "",
  message: null,
  installed: true,
  selectable: true,
  retry_at_ms: null,
  model: "m",
  models: ["m"],
  models_fixed: false,
  cli_default: false,
  models_unavailable_reason: null,
  ...over,
});

const snapshot = (ai: Partial<BackgroundAi>, kitAgents: unknown[] = []) => ({
  connection: "live" as const,
  rest: {
    status: { background_ai: { enabled: true, provider: "claude", chosen: true, providers: [provider("claude", "Claude Code"), provider("codex", "Codex")], fallback: [], refusal: null, unavailable_reason: null, ...ai } },
    navigator: { devices: [{ id: "local", kit: { agents: kitAgents } }] },
    ui_state: { issue_settings: { ai_worktree_name: true, closes_instruction: true } },
  },
});

afterEach(() => {
  document.body.innerHTML = "";
});

async function mount(next: ReturnType<typeof snapshot>) {
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
    root.render(<TooltipProvider><HideAiTab actions={actions} /></TooltipProvider>);
  });
  return { events, text: () => container.textContent ?? "", q: (selector: string) => container.querySelector(selector) as HTMLElement | null, unmount: async () => { await act(async () => root.unmount()); useShellStore.setState(saved, true); } };
}

it("names who is answering and why while another agent stands in (B41), and says nothing once Runs on answers again", async () => {
  const retry = new Date(2026, 9, 6, 15, 10).getTime();
  const standing = await mount(snapshot({ refusal: { provider: "claude", reason: "usage_limited", retry_at_ms: retry, using: "codex" } }));
  expect(standing.q("[data-ai-using='codex']")?.textContent).toMatch(/^.*Using Codex · Claude Code is out of usage until .*3:10.*PM/);
  // The consequence line is for the case nothing answers; here Codex does.
  expect(standing.q("[data-ai-paused]")).toBeNull();
  await standing.unmount();

  const recovered = await mount(snapshot({ refusal: null }));
  expect(recovered.q("[data-ai-using]")).toBeNull();
  await recovered.unmount();
});

it("puts the reason on the Runs on row, and no banner, when the chosen agent cannot answer and nobody is listed (B42)", async () => {
  const { q, text, unmount } = await mount(snapshot({ refusal: { provider: "claude", reason: "needs_login", retry_at_ms: null, using: null }, providers: [provider("claude", "Claude Code", { state: "needs_login", selectable: false }), provider("codex", "Codex")] }));
  expect(q("[data-ai-paused]")?.textContent).toBe("Claude Code is signed out · Hide AI is paused until Claude Code can answer.");
  // The chosen agent stays in the list with its state instead of vanishing (B34).
  expect(q("[data-ai-state='claude:needs_login']")?.textContent).toContain("Not signed in");
  expect(q("[data-ai-using]")).toBeNull();
  expect(text()).not.toContain("Using ");
  await unmount();
});

it("asks to sign in to the first agent that is on when nobody is signed in, and not before every row has been read (B47)", async () => {
  const signedOut = [provider("claude", "Claude Code", { state: "needs_login", selectable: false }), provider("codex", "Codex", { state: "needs_login", selectable: false })];
  const agents = [
    { id: "claude-code", label: "Claude Code", availability: "available", enabled: false },
    { id: "codex", label: "Codex", availability: "available", enabled: true },
  ];
  const none = await mount(snapshot({ provider: null, chosen: false, providers: signedOut }, agents));
  expect(none.q("[data-ai-sign-in]")?.textContent).toBe("Sign in to Codex to use Hide AI");
  // Hide's other features keep working: nothing is disabled here but the choice that has no option.
  expect(none.q("[data-ai-provider]")?.hasAttribute("disabled")).toBe(true);
  await none.unmount();

  const reading = await mount(snapshot({ provider: null, chosen: false, providers: signedOut.map((row) => ({ ...row, state: "unread" })) }, agents));
  expect(reading.q("[data-ai-sign-in]")).toBeNull();
  await reading.unmount();

  const nothingOn = await mount(snapshot({ provider: null, chosen: false, providers: signedOut }, [{ id: "claude-code", label: "Claude Code", availability: "available", enabled: false }]));
  expect(nothingOn.q("[data-ai-sign-in]")?.textContent).toBe("Turn on an agent in Agents to use Hide AI");
  await nothingOn.unmount();
});

it("takes the rest out of reach while Use Hide AI is off and sends one event for the switch (B33)", async () => {
  const { events, q, unmount } = await mount(snapshot({ enabled: false }));
  expect(q("[data-hide-ai-body]")?.hasAttribute("inert")).toBe(true);
  expect(q("[data-hide-ai-tab]")?.getAttribute("data-ai-enabled")).toBe("false");
  await act(async () => { q("[data-ai-use]")?.click(); });
  expect(events.filter((event) => event.kind === "ai_settings" && !("observing" in (event.payload as object)))).toEqual([{ schema_version: 2, kind: "ai_settings", payload: { enabled: true } }]);
  await unmount();
});

it("marks a fixed list, keeps the current model when the list failed, and offers CLI default (B36)", async () => {
  const fixed = await mount(snapshot({ providers: [provider("claude", "Claude Code", { models_fixed: true, models: ["auto", "pro"], model: "auto", models_unavailable_reason: "fixed" })] }));
  expect(fixed.q("[data-ai-models-fixed]")).not.toBeNull();
  expect(fixed.q("[data-ai-models-failed]")).toBeNull();
  await fixed.unmount();

  const failed = await mount(snapshot({ providers: [provider("claude", "Claude Code", { models: [], model: "sonnet", models_unavailable_reason: "claude_models_unreadable:exit=2" })] }));
  expect(failed.q("[data-ai-models-failed]")?.textContent).toBe("Couldn't load the model list. Keeping the current model.");
  expect(failed.q("[data-ai-model]")?.textContent).toContain("sonnet");
  await failed.unmount();

  const cli = await mount(snapshot({ providers: [provider("claude", "Claude Code", { models: ["a"], model: "", cli_default: true })] }));
  expect(cli.q("[data-ai-model]")?.textContent).toBe("CLI default");
  await cli.unmount();
});

it("shows no fallback group before an agent is chosen, since there is nothing to fall back from", async () => {
  const { q, unmount } = await mount(snapshot({ provider: null, chosen: false }));
  expect(q('[data-settings-group="hide-ai-fallback"]')).toBeNull();
  await unmount();
});
