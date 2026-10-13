// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { createActions, type Actions } from "./actions";
import { needsYouCount, receiveAgentNotice, useAgentNotifications } from "./agentNotices";
import type { AgentScope } from "./agentScope";
import { clientI18n } from "./i18n/translator";
import { useShellStore } from "./store";
import type { DispatchFn } from "./ws";

// The shell's modules reach xterm, which asks jsdom for a canvas it lacks.
vi.hoisted(() => {
  HTMLCanvasElement.prototype.getContext = () => null;
});

afterEach(() => {
  document.body.innerHTML = "";
  delete window.hideHost;
});

function Probe({ actions }: { actions: Actions }) {
  useAgentNotifications(actions);
  return null;
}

it("asks for agent notices while the switch is on, words each one, withdraws a seen agent's, and opens a clicked one's pane (B31, B34, B35)", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const shown: unknown[][] = [];
  const withdrawn: string[][] = [];
  let open: ((id: string) => void) | null = null;
  window.hideHost = {
    kind: "electron",
    platform: "darwin",
    notify: (...args: unknown[]) => shown.push(args),
    withdrawNotifications: (ids: string[]) => withdrawn.push(ids),
    reportBadge: () => undefined,
    onNotificationOpen: (listener: (id: string) => void) => {
      open = listener;
      return () => (open = null);
    },
  } as never;
  const events: Parameters<DispatchFn>[0][] = [];
  const actions = createActions((event) => {
    events.push(event);
    return true;
  });
  const saved = useShellStore.getState();
  const root = createRoot(document.body.appendChild(document.createElement("div")));
  await act(async () => {
    useShellStore.setState({ connection: "live", rest: { ui_state: { desktop_notifications: true }, navigator: { devices: [] } } } as never);
    root.render(<Probe actions={actions} />);
  });
  expect(events.filter((event) => event.kind === "desktop_notices").map((event) => event.payload)).toEqual([{ wanted: true }]);

  await act(async () => {
    receiveAgentNotice({ notices: [{ tag: "node|w1:p1", device_id: "node", pane_id: "w1:p1", title: "Add a dark mode switch", state: "needs_you", place: "my-app" }], clear: [] });
    receiveAgentNotice({ notices: [], clear: ["node|w1:p1"] });
  });
  expect(shown).toEqual([["agent:node|w1:p1", "Add a dark mode switch", "Needs you · my-app", { unlessFocused: true }]]);
  expect(withdrawn).toEqual([["agent:node|w1:p1"]]);

  // The notification contract uses the row's own status word with detail,
  // and retains the original words with no detail (including old frames).
  await act(async () => {
    for (const [status_code, detail] of [["question", "배포할 환경을 골라 주세요"], ["approval", "계획 승인 대기"], ["error", "로그인이 필요해요"], ["done", "배포 완료"], ["stopped", "테스트 세 개 남음"]] as const) {
      receiveAgentNotice({ notices: [{ tag: "node|w1:p1", device_id: "node", pane_id: "w1:p1", title: "Add a dark mode switch", state: status_code === "done" || status_code === "stopped" ? "done" : "needs_you", status_code, detail, place: "my-app" }], clear: [] });
    }
    receiveAgentNotice({ notices: [{ tag: "node|w1:p1", device_id: "node", pane_id: "w1:p1", title: "Add a dark mode switch", state: "done", status_code: "done", detail: null, place: "" }], clear: [] });
  });
  expect(shown.slice(1).map((notice) => notice.slice(1, 3))).toEqual([
    ["Add a dark mode switch", "Question · 배포할 환경을 골라 주세요 · my-app"],
    ["Add a dark mode switch", "Approval · 계획 승인 대기 · my-app"],
    ["Add a dark mode switch", "Blocked · 로그인이 필요해요 · my-app"],
    ["Add a dark mode switch", "Done · 배포 완료 · my-app"],
    ["Add a dark mode switch", "Stopped · 테스트 세 개 남음 · my-app"],
    ["Add a dark mode switch", "Done"],
  ]);
  await act(async () => { await clientI18n.changeLanguage("ko"); });
  await act(async () => receiveAgentNotice({ notices: [{ tag: "node|w1:p1", device_id: "node", pane_id: "w1:p1", title: "Add a dark mode switch", state: "needs_you", status_code: "question", detail: "Choose staging?", place: "" }], clear: [] }));
  expect(shown.at(-1)?.slice(1, 3)).toEqual(["Add a dark mode switch", "질문 · Choose staging?"]);
  await act(async () => {
    receiveAgentNotice({ notices: [{ tag: "node|w1:p1", device_id: "node", pane_id: "w1:p1", title: "Add a dark mode switch", state: "done", status_code: "done", detail: null, place: "my-app" }], clear: [] });
    receiveAgentNotice({ notices: [{ tag: "node|w1:p1", device_id: "node", pane_id: "w1:p1", title: "Add a dark mode switch", state: "needs_you", status_code: "approval", detail: null, place: "my-app" }], clear: [] });
  });
  expect(shown.slice(-2).map((notice) => notice[2])).toEqual(["끝 · my-app", "내 확인 대기 · my-app"]);
  await act(async () => { await clientI18n.changeLanguage("en"); });

  await act(async () => open?.("agent:node|w1:p1"));
  expect(events.some((event) => event.kind === "focus_pane" && event.payload.pane_id === "w1:p1")).toBe(true);

  // The switch off: the window stops asking, and a late frame shows nothing.
  await act(async () => {
    useShellStore.setState({ rest: { ui_state: { desktop_notifications: false }, navigator: { devices: [] } } } as never);
  });
  expect(events.filter((event) => event.kind === "desktop_notices").at(-1)?.payload).toEqual({ wanted: false });
  await act(async () => receiveAgentNotice({ notices: [{ tag: "node|w1:p2", device_id: "node", pane_id: "w1:p2", title: "x", state: "done", place: "" }], clear: [] }));
  expect(shown).toHaveLength(10);
  await act(async () => root.unmount());
  useShellStore.setState(saved, true);
});

/** A device whose sidebar Needs You group holds `shown` and `more` agents, and Done holds one. */
function device(id: string, shown: number, more: number) {
  const refs = (n: number, at: number) => Array.from({ length: n }, (_, i) => ({ pane_id: `${id}:p${at + i}`, occurrence: 0 }));
  const raised: AgentScope["raised"] = [{ group: "done", shown: refs(1, 90), more: [] }];
  if (shown + more > 0) raised.unshift({ group: "needs_you", shown: refs(shown, 0), more: refs(more, shown) });
  return { id, agent_scope: { raised } };
}

it("counts every device's Needs You group for the app icon, not Done", () => {
  expect(needsYouCount(null)).toBe(0);
  expect(needsYouCount({ navigator: { devices: [device("node", 5, 2), device("mini", 1, 0), device("box", 0, 0)] } } as never)).toBe(8);
});

it.each([
  [false, false, [], 0, false],
  [true, false, ["Needs you · my-app"], 2, true],
  [false, true, ["Done · my-app"], 0, true],
  [true, true, ["Needs you · my-app", "Done · my-app"], 2, true],
] as const)("with Needs You=%s and Done=%s sends only the chosen groups and counts Needs You", async (needsYou, done, bodies, badge, wanted) => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const shown: string[] = [];
  const reported: number[] = [];
  window.hideHost = {
    kind: "electron", platform: "linux",
    notify: (_id: string, _title: string, body: string) => shown.push(body),
    reportBadge: (count: number) => reported.push(count),
    withdrawNotifications: () => undefined,
    onNotificationOpen: () => () => undefined,
  } as never;
  const events: Parameters<DispatchFn>[0][] = [];
  const actions = createActions((event) => { events.push(event); return true; });
  const saved = useShellStore.getState();
  const root = createRoot(document.body.appendChild(document.createElement("div")));
  try {
    await act(async () => {
      useShellStore.setState({ connection: "live", rest: { ui_state: { desktop_notifications: needsYou, desktop_notifications_done: done }, navigator: { devices: [device("node", 2, 0)] } } } as never);
      root.render(<Probe actions={actions} />);
    });
    await act(async () => receiveAgentNotice({ notices: [
      { tag: "node|w1:p1", device_id: "node", pane_id: "w1:p1", title: "One", state: "needs_you", place: "my-app" },
      { tag: "node|w1:p2", device_id: "node", pane_id: "w1:p2", title: "Two", state: "done", place: "my-app" },
    ], clear: [] }));
    expect(shown).toEqual(bodies);
    expect(reported).toEqual([badge]);
    expect(events.filter((event) => event.kind === "desktop_notices").map((event) => event.payload)).toEqual(wanted ? [{ wanted: true }] : []);
  } finally {
    await act(async () => root.unmount());
    useShellStore.setState(saved, true);
  }
});

it("reports the app icon's count once the page is live, on each change, and none while the switch is off", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const reported: number[] = [];
  window.hideHost = {
    kind: "electron",
    platform: "win32",
    reportBadge: (count: number) => reported.push(count),
    onNotificationOpen: () => () => undefined,
  } as never;
  const actions = createActions(() => true);
  const saved = useShellStore.getState();
  const root = createRoot(document.body.appendChild(document.createElement("div")));
  const rest = (on: boolean, needsYou: number) => ({ ui_state: { desktop_notifications: on }, navigator: { devices: [device("node", needsYou, 0)] } });
  await act(async () => {
    useShellStore.setState({ connection: "connecting", rest: rest(true, 2) } as never);
    root.render(<Probe actions={actions} />);
  });
  expect(reported).toEqual([]);
  await act(async () => useShellStore.setState({ connection: "live" } as never));
  await act(async () => useShellStore.setState({ rest: rest(true, 2) } as never));
  await act(async () => useShellStore.setState({ rest: rest(true, 3) } as never));
  await act(async () => useShellStore.setState({ rest: rest(false, 3) } as never));
  expect(reported).toEqual([2, 3, 0]);
  await act(async () => root.unmount());
  useShellStore.setState(saved, true);
});
