import { afterEach, expect, it } from "vitest";
import { createActions } from "./actions";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";

// A session record is drawn only on the Work tool's Sessions lens, so the
// pane head's record button and the Overview's `lane에서 보기` must not leave
// the lane on PRs (PRD overview-device-tasks B18, B22, B23).

type Sent = { kind: string; payload: Record<string, unknown> };
const session = { device_id: "local", agent: "claude", session_id: "s-1" };

function frontWorkspace(tool: "files" | "work" | null, lens: "sessions" | "prs") {
  useShellStore.setState({
    rest: { navigator: { devices: [{ id: "local", kind: "local" }] }, workspace_view: { device_id: "local", path: "/p", tool, work: { lens, only_checkout: false }, pinned: true, tree: true, lane_width: null, tree_width: null } } as never,
  });
}

function recorder() {
  const sent: Sent[] = [];
  const actions = createActions((event) => {
    sent.push(event as Sent);
    return true;
  });
  return { sent, actions };
}

afterEach(() => {
  useShellStore.setState({ rest: null });
  useUiStore.setState({ sessionWorkTarget: null, drawnLane: { mode: "closed", tool: null } });
});

it("a record opened while Work shows PRs turns Work to Sessions in one lane event", () => {
  frontWorkspace("work", "prs");
  const { sent, actions } = recorder();
  actions.togglePaneRecord(session);
  expect(sent.filter((event) => event.kind === "workspace_view").map((event) => event.payload)).toEqual([{ tool: "work", work_lens: "sessions" }]);
  expect(sent.at(-1)).toEqual({ schema_version: 2, kind: "session_record_open", payload: { session } });
});

it("a record opened from another tool brings Work up on Sessions", () => {
  frontWorkspace("files", "sessions");
  const { sent, actions } = recorder();
  actions.openSessionRecord(session);
  expect(sent.filter((event) => event.kind === "workspace_view").map((event) => event.payload)).toEqual([{ tool: "work", work_lens: "sessions" }]);
});

it("a second press on the record shown goes back to the list, but not while Work shows PRs", () => {
  frontWorkspace("work", "sessions");
  useUiStore.setState({ sessionWorkTarget: session, drawnLane: { mode: "docked", tool: "work" } });
  const shown = recorder();
  shown.actions.togglePaneRecord(session);
  expect(shown.sent.map((event) => event.kind)).toEqual(["session_record_close"]);

  frontWorkspace("work", "prs");
  useUiStore.setState({ sessionWorkTarget: session });
  const hidden = recorder();
  hidden.actions.togglePaneRecord(session);
  expect(hidden.sent.map((event) => event.kind)).toEqual(["workspace_view", "session_record_open"]);
});

it("`lane에서 보기` sends the lane event even when the checkout left behind already showed Sessions", () => {
  // The view read before the switch is the old checkout's; the core applies the event to the new front one (B18).
  frontWorkspace("work", "sessions");
  const { sent, actions } = recorder();
  actions.viewRecordInLane("local", "w2", "c2", session);
  const kinds = sent.map((event) => event.kind);
  expect(sent.filter((event) => event.kind === "workspace_view").map((event) => event.payload)).toEqual([{ tool: "work", work_lens: "sessions" }]);
  expect(kinds.indexOf("focus_checkout")).toBeGreaterThan(-1);
  expect(kinds.indexOf("workspace_view")).toBeGreaterThan(kinds.indexOf("focus_checkout"));
  expect(kinds.at(-1)).toBe("session_record_open");
});
