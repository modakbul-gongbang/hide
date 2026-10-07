import { emptyScope } from "../test/legacyAgentScope";
// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterAll, afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createActions } from "./actions";
import { TooltipProvider } from "./components/ui/tooltip";
import { ProjectSessions } from "./ProjectSessions";
import type { ProjectSessions as History, SessionRow, SessionSearch, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import type { DispatchFn } from "./ws";

// Real actions import xterm's browser capability probe. jsdom's unsupported
// canvas returns null with an error log; report that capability without noise.
const browserCanvas = vi.hoisted(() => {
  const original = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = () => null;
  return { restore: () => { HTMLCanvasElement.prototype.getContext = original; } };
});
afterAll(() => browserCanvas.restore());

// Expectations come from the server-session-search handoff: independent
// history/search arrival, truthful incomplete outcomes, metadata fallback,
// and a query scoped to the selected Project rather than the front Workspace.
const project: Workspace = { agent_scope: emptyScope(),
  id: "project:studio", label: "Studio", path: "/projects/studio", device_id: "local",
  registered: true, temporary: false, pinned: false, checkouts: [],
  inactive_checkouts: { expanded: false, checkout_ids: [] },
};
const row: SessionRow = {
  id: "session:codex", provider: "codex", provider_label: "Codex",
  locator: "/sessions/codex.jsonl", checkout_path: project.path,
  first_human_request: "Repair the metadata fallback", title: "Metadata fallback",
  started_at_unix_ms: null, updated_at_unix_ms: 0, unavailable_reason: null,
};
const history = (overrides: Partial<History> = {}): History => ({
  workspace_id: project.id, device_id: project.device_id,
  loading: false, failure: null, unavailable_reason: null, rows: [row], detail: null,
  ...overrides,
});
const search = (overrides: Partial<SessionSearch> = {}): SessionSearch => ({
  workspace_id: project.id, device_id: project.device_id, provider: "all", query: "검색",
  loading: false, indexing: false, indexed: 1, total: 1, days: 90, policy_loaded: true,
  control_failure: null, failure: null, page: { hits: [], limited: false, stale: false },
  ...overrides,
});

describe("Project Sessions content-search outcomes", () => {
  let container: HTMLDivElement;
  let root: Root;
  let saved: ReturnType<typeof useShellStore.getState>;
  let events: Parameters<DispatchFn>[0][];

  beforeEach(() => {
    saved = useShellStore.getState();
    // Only the browser's clock and React's browser-test flag are substituted.
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    useShellStore.setState({ connection: "live", projectSessions: history(), sessionSearch: null, rest: null });
    events = [];
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    useShellStore.setState(saved, true);
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  async function render() {
    const actions = createActions((event) => { events.push(event); return true; });
    await act(async () => root.render(<TooltipProvider><ProjectSessions workspace={project} actions={actions} /></TooltipProvider>));
  }
  function button(label: string) {
    const found = [...container.querySelectorAll<HTMLButtonElement>("button")].find((element) => element.textContent?.trim() === label);
    if (!found) throw new Error(`Missing button: ${label}`);
    return found;
  }
  async function enterQuery(query: string) {
    const input = container.querySelector<HTMLInputElement>('input[aria-label="Search sessions"]');
    if (!input) throw new Error("Missing session search input");
    await act(async () => {
      // Use a native input event, exercising React's real change handling.
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
      if (!setter) throw new Error("Browser input value setter unavailable");
      setter.call(input, query);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
  }
  async function settleQuery() {
    await act(async () => { await vi.advanceTimersByTimeAsync(500); });
  }
  async function deliverSearch(next: SessionSearch) {
    await act(async () => {
      const state = useShellStore.getState();
      state.applyFrame({ type: "delta", payload: { revision: state.revision + 1, session_search: next } });
    });
  }

  it.each([
    ["missing", null],
    ["another Project", search({ workspace_id: "project:other" })],
    ["another device", search({ device_id: "mini" })],
  ] as const)("renders a named loading Project with %s content-search state, then its history delta", async (_label, content) => {
    useShellStore.getState().applyFrame({
      type: "snapshot", payload: {
        revision: 1, rest: {}, project_sessions: history({ loading: true, rows: [] }),
        ...(content ? { session_search: content } : {}),
      },
    });
    await render();
    expect(container.textContent).toContain("Loading sessions");
    expect(container.textContent).toContain("Searching conversations");
    expect(container.querySelector('input[aria-label="Search sessions"]')).not.toBeNull();

    await act(async () => useShellStore.getState().applyFrame({ type: "delta", payload: { revision: 2, project_sessions: history() } }));
    expect(container.textContent).toContain("Repair the metadata fallback");
    await enterQuery("검색");
    expect(container.textContent).toContain("Searching conversation contents");
    expect(container.textContent).not.toContain("No matching sessions");
  });

  it.each([
    ["pending query", search({ loading: true })],
    ["partial indexing", search({ indexing: true, indexed: 0, total: 2 })],
    ["obsolete query", search({ query: "old query" })],
    ["obsolete provider", search({ provider: "claude" })],
  ] as const)("does not promise no matches for a %s", async (_label, content) => {
    useShellStore.setState({ sessionSearch: content });
    await render();
    await enterQuery("검색");
    expect(container.textContent).not.toContain("No matching sessions");
    expect(container.textContent).toMatch(/Searching conversation contents|Indexing continues/);

    await deliverSearch(search());
    expect(container.textContent).toContain("No matching sessions");
    expect(button("Clear filters")).toBeDefined();
  });

  it.each([
    ["failed", search({ failure: "Conversation query could not complete. Retry." })],
    ["stale", search({ page: { hits: [], limited: false, stale: true } })],
  ] as const)("keeps %s content results distinct from an authoritative no-match", async (_label, content) => {
    useShellStore.setState({ sessionSearch: content });
    await render();
    await enterQuery("검색");
    expect(container.textContent).not.toContain("No matching sessions");
    expect(container.textContent).toContain("Conversation results could not be confirmed");
    await act(async () => button("Retry").click());
    expect(events.filter((event) => event.kind === "sessions_refresh").at(-1)?.payload).toMatchObject({
      workspace_id: project.id,
    });
  });

  it("shows a successful new query after failure without retaining the failed-query message", async () => {
    const failure = "Conversation query could not complete. Retry.";
    useShellStore.setState({ sessionSearch: search({ failure }) });
    await render();
    await enterQuery("검색");
    expect(container.textContent).toContain(failure);
    await enterQuery("복구");
    await settleQuery();
    expect(events.filter((event) => event.kind === "session_search").at(-1)?.payload).toMatchObject({ query: "복구" });
    await deliverSearch(search({ query: "복구", page: {
      hits: [{ session_id: row.id, source_offset: 42, role: "assistant", at_unix_ms: 0, snippet: "복구 결과를 다시 찾았습니다" }],
      limited: false, stale: false,
    } }));
    expect(container.textContent).toContain("복구 결과를 다시 찾았습니다");
    expect(container.textContent).toContain("Assistant");
    expect(container.textContent).not.toContain(failure);
    expect(container.textContent).not.toContain("Conversation search unavailable");
  });

  it.each([
    ["failed", search({ query: "metadata", failure: "Content query failed. Retry." })],
    ["off", search({ query: "metadata", days: 0 })],
  ] as const)("preserves usable metadata search while content search is %s", async (_label, content) => {
    useShellStore.setState({ sessionSearch: content });
    await render();
    await enterQuery("metadata");
    expect(container.textContent).toContain("Repair the metadata fallback");
    const session = container.querySelector<HTMLButtonElement>('button[aria-label*="Repair the metadata fallback"]');
    if (!session) throw new Error("Metadata match is not selectable");
    await act(async () => session.click());
    expect(events.filter((event) => event.kind === "archive_open").at(-1)?.payload).toMatchObject({
      workspace_id: project.id, id: row.id, kind: "session",
    });
  });

  it("sends the changed provider with the selected Project and ignores results for the previous provider", async () => {
    const claudeRow = { ...row, id: "session:claude", provider: "claude", provider_label: "Claude Code", first_human_request: "Review the old session" };
    useShellStore.setState({
      projectSessions: history({ rows: [row, claudeRow] }),
      sessionSearch: search({ page: {
        hits: [{ session_id: claudeRow.id, source_offset: 9, role: "user", at_unix_ms: 0, snippet: "검색 from the old provider" }],
        limited: false, stale: false,
      } }),
      rest: { navigator: { focused_device_id: "mini", focused_workspace_id: "project:other" } },
    });
    await render();
    await enterQuery("검색");
    await settleQuery();
    expect(container.textContent).toContain("검색 from the old provider");
    await act(async () => button("Codex").click());
    await settleQuery();
    expect(events.filter((event) => event.kind === "session_search").at(-1)?.payload).toMatchObject({
      workspace_id: project.id, device_id: project.device_id, query: "검색", provider: "codex",
    });
    expect(container.textContent).not.toContain("검색 from the old provider");
    expect(container.textContent).not.toContain("No matching sessions");
    expect(container.textContent).toContain("Searching conversation contents");

    await deliverSearch(search({ provider: "codex", page: {
      hits: [{ session_id: row.id, source_offset: 17, role: "user", at_unix_ms: 0, snippet: "검색 in Codex" }],
      limited: false, stale: false,
    } }));
    expect(container.textContent).toContain("검색 in Codex");
    expect(container.textContent).not.toContain("Review the old session");
  });
});
