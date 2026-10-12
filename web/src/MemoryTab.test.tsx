// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterAll, afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createActions } from "./actions";
import { TooltipProvider } from "./components/ui/tooltip";
import { Tools } from "./Tools";
import type { MemoryRow, ProjectMemory, SnapshotRest } from "./snapshot";
import { useShellStore } from "./store";
import type { DispatchFn } from "./ws";

const browserCanvas = vi.hoisted(() => {
  const original = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = () => null;
  return { restore: () => { HTMLCanvasElement.prototype.getContext = original; } };
});
afterAll(() => browserCanvas.restore());
// jsdom lays nothing out; the card shown in full asks to be scrolled to.
Element.prototype.scrollIntoView = () => {};

// The front checkout `c` belongs to Project `p` (design v7, PRD
// project-memory-v2 B19-B32).
const rest = {
  navigator: {
    focused_device_id: "local", focused_checkout_id: "c", focused_workspace_id: "p",
    devices: [{ id: "local", label: "This Mac", kind: "local", state: "local" }],
    workspaces: [{ id: "p", label: "herdr-ide", device_id: "local", checkouts: [{ id: "c", workspace_id: "p", path: "/p", label: "main", tabs: [], active_tab_id: null, strip: [] }] }],
  },
} as unknown as SnapshotRest;
const row = (number: number, overrides: Partial<MemoryRow> = {}): MemoryRow => ({
  number, title: `card ${number}`, kind: "rule", human_source_count: 1, provided_session_count: 2,
  pull_count: 3, updated_at_unix_ms: Date.now(), promotion: null, ...overrides,
});
const memory = (overrides: Partial<ProjectMemory> = {}): ProjectMemory => ({
  workspace_id: "p", unavailable: null, loading: false, enabled: true, capacity_reached: false,
  rows: [row(3), row(7)], questions: [], analysis: "idle", forgotten: null, pending_action: null,
  expanded: null, search: null, ...overrides,
});

describe("the Memory tab", () => {
  let container: HTMLDivElement;
  let root: Root;
  let saved: ReturnType<typeof useShellStore.getState>;
  let events: Parameters<DispatchFn>[0][];

  beforeEach(() => {
    saved = useShellStore.getState();
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    events = [];
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    useShellStore.setState(saved, true);
    vi.unstubAllGlobals();
  });

  async function render(project: ProjectMemory | null, tool: "memory" | "explorer" = "memory") {
    useShellStore.setState({ connection: "live", rest, memory: { project } });
    const actions = createActions((event) => { events.push(event); return true; });
    await act(async () => root.render(<TooltipProvider><Tools tool={tool} actions={actions} /></TooltipProvider>));
  }
  const sent = (kind: string) => events.filter((event) => event.kind === kind).map((event) => event.payload);
  const click = async (selector: string) => {
    // A dialog renders in a portal beside the tab.
    const element = document.body.querySelector<HTMLElement>(selector);
    if (!element) throw new Error(`missing ${selector}`);
    await act(async () => element.click());
  };

  it("opens the front checkout's Project and reads until the core answers for that Project", async () => {
    await render(memory({ workspace_id: "another" }));
    expect(sent("memory_open")).toEqual([{ workspace_id: "p" }]);
    expect(container.querySelector("[data-memory]")).toBeNull();
    expect(container.textContent).toContain("Reading");

    // Another window named another Project; this one names its own again
    // when it comes forward.
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(sent("memory_open")).toHaveLength(2);
  });

  it("opens the Project and shows the tab's dot from any tool while its Project has a question", async () => {
    await render(memory({ questions: [{ kind: "failed", number: 7, title: "card 7", start_failure: null }] }), "explorer");
    expect(sent("memory_open")).toEqual([{ workspace_id: "p" }]);
    expect(container.querySelector('[data-tool-dot="memory"]')).not.toBeNull();
  });

  it("asks about a suggested card and a failed promotion, and sends the card's number", async () => {
    await render(memory({
      questions: [
        { kind: "promote", number: 3, title: "card 3", human_source_count: 3, pulled_session_count: 9 },
        { kind: "failed", number: 7, title: "card 7", start_failure: null },
      ],
    }));
    expect(container.querySelector("[data-memory-questions]")?.textContent).toContain("Operator's words 3 · pulled in 9 sessions in 14 days");
    // Both cards are asked about, so no list repeats them.
    expect(container.querySelector("[data-memory-cards]")).toBeNull();
    await click("[data-memory-promote='3']");
    await click("[data-memory-dismiss='3']");
    await click("[data-memory-retry='7']");
    expect(sent("memory_action")).toEqual([
      { workspace_id: "p", action: "promote", number: 3 },
      { workspace_id: "p", action: "dismiss", number: 3 },
      { workspace_id: "p", action: "promote", number: 7 },
    ]);
  });

  it("says in a failed card's status why its agent never started, with the question its shell stopped at", async () => {
    await render(memory({
      questions: [
        { kind: "failed", number: 7, title: "card 7", start_failure: null },
        { kind: "failed", number: 8, title: "card 8", start_failure: { code: "shell_starting", detail: "Update now? [Y/n]" } },
        { kind: "failed", number: 9, title: "card 9", start_failure: { code: "agent_not_started", detail: null } },
      ],
    }));
    const status = (number: number) => container.querySelector(`[data-memory-card='${number}'] [data-memory-failure]`)?.textContent;
    expect(status(7)).toBe("Ended without a PR");
    expect(status(8)).toBe("The shell stopped at a question: Update now? [Y/n]");
    expect(status(9)).toBe("The agent didn't start");
    // Retry stays the one action; nothing else is offered.
    expect(container.querySelectorAll("[data-memory-retry]")).toHaveLength(3);
  });

  it("shows a promotion's agent, its pull request, and a merged card dimmed with its file", async () => {
    await render(memory({
      rows: [
        row(7, { promotion: { state: "running", agent: { name: "memory-promote-m7", pane_id: "w1:p2" }, pr_number: null, target_path: null, github_failure: null } }),
        row(9, { promotion: { state: "pr_open", agent: null, pr_number: 924, target_path: null, github_failure: null } }),
        row(8, { promotion: { state: "merged", agent: null, pr_number: 924, target_path: "AGENTS.md", github_failure: null } }),
        row(4, { promotion: { state: "merged", agent: null, pr_number: 931, target_path: null, github_failure: null } }),
        row(6, { promotion: { state: "running", agent: null, pr_number: null, target_path: null, github_failure: null } }),
        row(5, { promotion: { state: "unconfirmed", agent: null, pr_number: null, target_path: null, github_failure: "not_logged_in" } }),
      ],
    }));
    // GitHub could not say whether the gone agent's pull request came: a quiet
    // line with its reason, no spinner and nothing to press.
    const unconfirmed = container.querySelector("[data-memory-card='5']");
    expect(unconfirmed?.querySelector("[data-memory-unconfirmed]")?.textContent).toBe("Couldn't check for a PR·gh not logged in");
    expect(unconfirmed?.querySelector(".animate-spin")).toBeNull();
    expect(unconfirmed?.textContent).not.toContain("Retry");
    // A merged card no file was named for shows the pull request it went in by.
    expect(container.querySelector("[data-memory-card='4'] [data-pr-mark]")?.textContent).toContain("931");
    expect(container.querySelector("[data-memory-running='7']")?.textContent).toBe("An agent is promoting it·memory-promote-m7");
    // An agent not seen yet: no name, and nothing it would follow.
    expect(container.querySelector("[data-memory-running='6']")?.textContent).toBe("An agent is promoting it");
    expect(container.querySelector("[data-memory-card='9'] [data-pr-mark]")?.textContent).toContain("924");
    const merged = container.querySelector("[data-memory-card='8']");
    expect(merged?.className).toContain("opacity-");
    expect(merged?.textContent).toContain("AGENTS.md");
    expect(merged?.textContent).toContain("In the file · not injected");
  });

  it("deletes the Project's data only after the confirmation names the count", async () => {
    await render(memory({ enabled: false }));
    await click("[data-memory-delete='link']");
    expect(sent("memory_action")).toEqual([]);
    expect(document.body.textContent).toContain("Delete herdr-ide's Memory data?");
    await click("[data-memory-delete='confirm']");
    expect(sent("memory_action")).toEqual([{ workspace_id: "p", action: "delete", number: null }]);
  });

  it("offers a forgotten card back and a replaced text's undo by card number", async () => {
    await render(memory({
      forgotten: 9,
      expanded: { number: 3, body: "new text", cited_paths: ["docs/TESTING.md"], sources: [{ provider_label: "Codex", at_unix_ms: 1, human: true }], previous: { body: "old text", changed_at_unix_ms: Date.now() } },
    }));
    expect(container.querySelector("[data-memory-forgotten]")?.textContent).toContain("m9 forgotten");
    await click("[data-memory-forgotten] button");
    await click("[data-memory-previous='3']");
    expect(container.querySelector("[data-memory-detail='3']")?.textContent).toContain("old text");
    await click("[data-memory-undo='3']");
    expect(sent("memory_action")).toEqual([
      { workspace_id: "p", action: "undo", number: 9 },
      { workspace_id: "p", action: "undo", number: 3 },
    ]);
  });

  it("sends one search for one edit while two windows show the Project's shared answer", async () => {
    // Two windows on one Project share the core's search; the second follows it.
    const other = document.createElement("div");
    document.body.append(other);
    const otherRoot = createRoot(other);
    const dispatch: DispatchFn = (event) => {
      events.push(event);
      if (event.kind === "memory_search") {
        const { query } = event.payload as { query: string };
        const project = useShellStore.getState().memory?.project ?? null;
        // The core trims the words it searches by.
        useShellStore.setState({ memory: { project: project && { ...project, search: { query: query.trim(), numbers: [7] } } } });
      }
      return true;
    };
    vi.useFakeTimers();
    try {
      useShellStore.setState({ connection: "live", rest, memory: { project: memory() } });
      const tools = <TooltipProvider><Tools tool="memory" actions={createActions(dispatch)} /></TooltipProvider>;
      await act(async () => root.render(tools));
      await act(async () => otherRoot.render(tools));
      const field = container.querySelector<HTMLInputElement>("[data-memory-search]")!;
      await act(async () => {
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(field, "lint ");
        field.dispatchEvent(new Event("input", { bubbles: true }));
      });
      await act(async () => { vi.advanceTimersByTime(5_000); });
      expect(sent("memory_search")).toEqual([{ workspace_id: "p", query: "lint " }]);
      // The space typed before the next word stays in the field.
      expect(field.value).toBe("lint ");
      expect(other.querySelector<HTMLInputElement>("[data-memory-search]")!.value).toBe("lint");
    } finally {
      vi.useRealTimers();
      await act(async () => otherRoot.unmount());
      other.remove();
    }
  });

  it("says a device's Project keeps no Memory here", async () => {
    await render(memory({ unavailable: { reason: "remote_device", device_name: "mini" } }));
    expect(container.textContent).toContain("This pane runs on mini.");
    expect(container.querySelector("[data-memory-cards]")).toBeNull();
  });
});
