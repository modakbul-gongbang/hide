// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { afterAll, expect, it, vi } from "vitest";
import { createActions } from "./actions";
import { AgentMark } from "./AgentMark";
import { AGENT_ADAPTERS, agentAdapter } from "./agentAdapters";
import { PROVIDER_KINDS } from "./agentPicker";
import { TooltipProvider } from "./components/ui/tooltip";
import { LinkSessions } from "./LinkSessions";
import { disarmKeyTarget } from "./keyTarget";
import { resumeProvider } from "./linkPanel";
import { NO_CHOICE, START_KINDS, selectionOf } from "./mobile/start";
import type { Checkout, LinkPanel, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import type { DispatchFn } from "./ws";
import { emptyScope } from "../test/legacyAgentScope";

// Match the browser's lack of a canvas capability in jsdom, without a probe log.
const canvas = vi.hoisted(() => {
  const original = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = () => null;
  return { restore: () => { HTMLCanvasElement.prototype.getContext = original; } };
});
afterAll(() => canvas.restore());

it.each([
  [" CLAUDE-CODE ", "claude"], [" CLAUDE ", "claude"], [" CLAUDE_CODE ", "claude"], [" CODEX ", "codex"],
])("renders %s with its existing branded mark and canonical resume provider", (kind, canonical) => {
  expect(renderToStaticMarkup(<AgentMark kind={kind} />)).toContain(`data-agent-mark="${canonical}"`);
  expect(resumeProvider(kind)).toBe(canonical);
});

it.each(["opencode", "grok", "omp", "cursor", "future-agent"])("keeps %s neutral and ineligible for resume", (kind) => {
  expect(renderToStaticMarkup(<AgentMark kind={kind} />)).toContain('data-agent-mark="neutral"');
  expect(resumeProvider(kind)).toBeNull();
});

it("keeps Pi's neutral mark while enabling its completed exact resume reader", () => {
  expect(renderToStaticMarkup(<AgentMark kind="pi" />)).toContain('data-agent-mark="neutral"');
  expect(resumeProvider(" PI ")).toBe("pi");
});

it("derives all seven supported phone starts and the existing default from the generated projection", () => {
  expect(START_KINDS).toEqual([
    { id: "claude", label: "Claude" }, { id: "codex", label: "Codex" },
    { id: "grok", label: "Grok" }, { id: "opencode", label: "OpenCode" },
    { id: "pi", label: "Pi" }, { id: "omp", label: "omp" },
    { id: "cursor", label: "Cursor" },
  ]);
  expect(START_KINDS.map((row) => row.id)).toEqual(PROVIDER_KINDS);
  expect(START_KINDS).toEqual(AGENT_ADAPTERS.filter((row) => row.can_start).map((row) => ({ id: row.herdr_kind, label: row.picker_label })));
  expect(selectionOf(null, NO_CHOICE).kind).toBe("claude");
  expect(agentAdapter("future-agent")).toBeUndefined();
});

it.each([[" CLAUDE_CODE ", "claude"], [" CODEX ", "codex"], [" PI ", "pi"]])("dispatches a canonical resume from the actual %s session button", async (kind, canonical) => {
  const saved = useShellStore.getState();
  const checkout = { id: "checkout", workspace_id: "project", path: "/project/task", branch: "task", exists: true } as Checkout;
  const project = { id: "project", device_id: "local", path: "/project", label: "Project", checkouts: [checkout] } as Workspace;
  const panel: LinkPanel = {
    workspace_id: project.id, target: { kind: "pr", number: 1 }, loading: false, failure: null,
    pr: null, prs: [], total: 1, sessions: [{
      agent: kind, id: "session", ids: ["session"], device_id: "local", role: "worked", pr: 1,
      request: "Resume this work", started_at_unix_ms: null, ended_at_unix_ms: null,
      path: "/sessions/session", cwd: checkout.path, file: "present", parent: null,
    }],
  };
  const events: Parameters<DispatchFn>[0][] = [];
  const actions = createActions((event) => { events.push(event); return true; });
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  try {
    useShellStore.setState({ rest: { navigator: { workspaces: [project], devices: [{ agent_scope: emptyScope(), id: "local", kind: "local", label: "Local", state: "local", message: null, ssh_alias: null, agent_count: 0, test: null }] } } });
    await act(async () => root.render(<TooltipProvider><LinkSessions panel={panel} project={project} branchOf={() => "task"} onRetry={() => undefined} empty={null} actions={actions} /></TooltipProvider>));
    const button = container.querySelector<HTMLButtonElement>('[data-link-button="resume"]');
    expect(button).not.toBeNull();
    expect(button?.getAttribute("aria-disabled")).not.toBe("true");
    await act(async () => button!.click());
    expect(events.filter((event) => event.kind === "agent_start_in_checkout")).toHaveLength(1);
    expect(events.find((event) => event.kind === "agent_start_in_checkout")?.payload).toMatchObject({
      provider: canonical, checkout_path: checkout.path, resume_session_id: "session",
      ...(canonical === "pi" ? { resume_session_path: "/sessions/session" } : {}),
    });
  } finally {
    await act(async () => root.unmount());
    container.remove();
    useShellStore.setState(saved, true);
    disarmKeyTarget();
    vi.unstubAllGlobals();
  }
});
