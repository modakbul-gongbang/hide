// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { createActions, type Actions } from "./actions";
import { NotificationRows, useNotificationControl } from "./NotificationControls";
import type { NotificationPermission } from "./host";
import { useShellStore } from "./store";
import type { DispatchFn } from "./ws";

vi.hoisted(() => { HTMLCanvasElement.prototype.getContext = () => null; });
afterEach(() => { document.body.innerHTML = ""; delete window.hideHost; });

function Probe({ actions, permission }: { actions: Actions; permission: NotificationPermission }) {
  const control = useNotificationControl(actions, "settings", permission);
  return control && <NotificationRows control={control} />;
}

it("keeps two independent switches, shows the permission/test row for either, and sends one event per toggle", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const notify = vi.fn();
  window.hideHost = { kind: "electron", platform: "darwin", notify } as never;
  const saved = useShellStore.getState();
  const events: Parameters<DispatchFn>[0][] = [];
  const actions = createActions((event) => { events.push(event); return true; });
  const root = createRoot(document.body.appendChild(document.createElement("div")));
  const preferences = (needsYou: boolean, done: boolean) => useShellStore.setState({ rest: { ui_state: { desktop_notifications: needsYou, desktop_notifications_done: done } } } as never);
  const needsYou = () => document.querySelector<HTMLButtonElement>("[data-notification-switch]")!;
  const done = () => document.querySelector<HTMLButtonElement>("[data-notification-done-switch]")!;
  try {
    for (const [needs, finished] of [[false, false], [true, false], [false, true], [true, true]] as const) {
      await act(async () => { preferences(needs, finished); root.render(<Probe actions={actions} permission="granted" />); });
      expect(needsYou().getAttribute("aria-checked")).toBe(String(needs));
      expect(done().getAttribute("aria-checked")).toBe(String(finished));
      expect(document.querySelector("[data-notification-test]") !== null).toBe(needs || finished);
    }
    await act(async () => { preferences(false, false); });
    await act(async () => { done().click(); });
    await act(async () => { needsYou().click(); });
    expect(events).toEqual([
      { schema_version: 2, kind: "desktop_notifications_done_set", payload: { enabled: true } },
      { schema_version: 2, kind: "desktop_notifications_set", payload: { enabled: true } },
    ]);
    expect(notify).toHaveBeenCalledTimes(2);
    await act(async () => { preferences(false, true); root.render(<Probe actions={actions} permission="denied" />); });
    expect(document.querySelector("[data-notification-open-settings]")).not.toBeNull();
    await act(async () => { root.render(<Probe actions={actions} permission="no_service" />); });
    expect(needsYou().disabled).toBe(true);
    expect(done().disabled).toBe(true);
    expect(done().getAttribute("aria-checked")).toBe("false");
    expect(document.querySelector("[data-notification-test]")).toBeNull();
  } finally {
    await act(async () => root.unmount());
    useShellStore.setState(saved, true);
  }
});
