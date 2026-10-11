// The desktop notification controls the wizard's second step and Settings >
// General share (PRD first-run-onboarding B14-B20, B30): the operator's
// switch, which the core keeps (`ui_state.desktop_notifications`), and what
// the system says about Hide's permission, which the desktop host reads.
// A browser tab has neither (D-14).

import { useEffect, useRef, useState } from "react";
import type { Actions } from "./actions";
import { Button } from "./components/ui/button";
import { Status } from "./components/settings-rows";
import { Switch } from "./components/ui/switch";
import { notificationState, PERMISSION_POLL_MS, type NotificationState } from "./firstRun";
import { hostBridge, type NotificationPermission } from "./host";
import { useInterfaceTranslation } from "./i18n/client";
import { useShellStore } from "./store";

/** The test notification's id: a second test replaces the first rather than stacking. */
const TEST_ID = "test";

/**
 * Hide's notification permission while a screen that shows it is open: read
 * now and every couple of seconds, one read at a time, so a change in System
 * Settings shows within seconds (B16) and nothing is read once the screen
 * closes (B18). Null until the first answer, and in a browser tab.
 */
export function usePermission(): NotificationPermission | null {
  const bridge = hostBridge();
  const [permission, setPermission] = useState<NotificationPermission | null>(null);
  useEffect(() => {
    if (!bridge) return;
    let stopped = false;
    let timer: number | undefined;
    const read = async () => {
      // The host logs why a read failed; the screen says only that it could not read (B17).
      const answer = await bridge.notificationPermission().catch((): NotificationPermission => "unknown");
      if (stopped) return;
      setPermission(answer);
      // A system with nothing to read is not asked again.
      if (answer !== "unsupported") timer = window.setTimeout(() => void read(), PERMISSION_POLL_MS);
    };
    void read();
    return () => {
      stopped = true;
      window.clearTimeout(timer);
    };
  }, [bridge]);
  return permission;
}

/** The desktop notification controls over `permission`, which the screen showing them reads with `usePermission`. */
export function NotificationControls({ actions, place, permission }: { actions: Actions; place: "wizard" | "settings"; permission: NotificationPermission | null }) {
  const { t } = useInterfaceTranslation();
  const bridge = hostBridge();
  const enabled = useShellStore((s) => s.rest?.ui_state?.desktop_notifications === true);
  const state = notificationState(enabled, permission, place);
  const sendTest = () => bridge?.notify(TEST_ID, t("firstRun.notifications.testTitle"), t("firstRun.notifications.testBody"));
  // Allowing in macOS's dialog shows the test the press asked for (B15); it replaces the one the dialog held.
  const before = useRef<NotificationState>(state);
  useEffect(() => {
    if (before.current === "asking" && state === "on") sendTest();
    before.current = state;
    // `sendTest` words the test in the language now; the state change is the one trigger.
  }, [state]);
  if (!bridge) return null;
  // The first notification is what makes macOS ask (B14); elsewhere it is the one test (B19, B20).
  const turnOn = () => {
    actions.setDesktopNotifications(true);
    sendTest();
  };
  return (
    <div className="flex flex-col gap-sm" data-notification-state={state} data-notification-place={place}>
      {place === "settings" ? (
        <label className="flex items-center justify-between gap-md text-subhead text-foreground">
          <span className="flex min-w-0 flex-col gap-xxs">
            <span>{t("firstRun.settings.switch")}</span>
            <span className="text-body text-muted-foreground">{t("firstRun.settings.switchDetail")}</span>
          </span>
          <Switch checked={enabled} onCheckedChange={(next) => (next ? turnOn() : actions.setDesktopNotifications(false))} data-notification-switch="true" />
        </label>
      ) : state === "off" ? (
        <Button className="self-start" onClick={turnOn} data-notification-turn-on="true">
          {t("firstRun.notifications.turnOn")}
        </Button>
      ) : null}
      {state === "asking" ? (
        <Status tone="pending" data-notification-line="asking">
          {t("firstRun.notifications.asking")}
        </Status>
      ) : state === "on" || state === "on_unchecked" ? (
        <Status tone="ok" data-notification-line="on">
          {t("firstRun.notifications.on")}
        </Status>
      ) : state === "blocked" ? (
        <Status tone="warn" data-notification-line="blocked">
          {t("firstRun.notifications.blocked")}
        </Status>
      ) : state === "unreadable" ? (
        <Status tone="muted" data-notification-line="unreadable">
          {t("firstRun.notifications.unreadable")}
        </Status>
      ) : null}
      <div className="flex flex-wrap gap-sm">
        {state === "blocked" ? (
          <Button variant="secondary" onClick={() => bridge.openNotificationSettings()} data-notification-open-settings="true">
            {t("firstRun.notifications.openSettings")}
          </Button>
        ) : null}
        {state === "on" || state === "on_unchecked" || state === "unreadable" ? (
          <Button variant="secondary" onClick={sendTest} data-notification-test="true">
            {t("firstRun.notifications.test")}
          </Button>
        ) : null}
      </div>
    </div>
  );
}
