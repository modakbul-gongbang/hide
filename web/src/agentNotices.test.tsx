// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { createActions, type Actions } from "./actions";
import { receiveAgentNotice, useAgentNotifications } from "./agentNotices";
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

  await act(async () => open?.("agent:node|w1:p1"));
  expect(events.some((event) => event.kind === "focus_pane" && event.payload.pane_id === "w1:p1")).toBe(true);

  // The switch off: the window stops asking, and a late frame shows nothing.
  await act(async () => {
    useShellStore.setState({ rest: { ui_state: { desktop_notifications: false }, navigator: { devices: [] } } } as never);
  });
  expect(events.filter((event) => event.kind === "desktop_notices").at(-1)?.payload).toEqual({ wanted: false });
  await act(async () => receiveAgentNotice({ notices: [{ tag: "node|w1:p2", device_id: "node", pane_id: "w1:p2", title: "x", state: "done", place: "" }], clear: [] }));
  expect(shown).toHaveLength(1);
  await act(async () => root.unmount());
  useShellStore.setState(saved, true);
});
