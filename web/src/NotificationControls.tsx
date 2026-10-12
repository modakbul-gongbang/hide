// The desktop notification controls the wizard's second step and Settings >
// General share (PRD first-run-onboarding B14-B20, B30): the operator's
// switch, which the core keeps (`ui_state.desktop_notifications`), and what
// the system says about Hide's permission, which the desktop host reads.
// A browser tab has neither (D-14).

import { LoaderCircleIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { Actions } from "./actions";
import { Button } from "./components/ui/button";
import { Row, Status } from "./components/settings-rows";
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
      // No notification service holds for the run; a Linux desktop says so only once a notification failed to show, so the rest are read again.
      if (answer !== "no_service") timer = window.setTimeout(() => void read(), PERMISSION_POLL_MS);
    };
    void read();
    return () => {
      stopped = true;
      window.clearTimeout(timer);
    };
  }, [bridge]);
  return permission;
}

/** What a screen showing the controls reads and does: the line's state, and the presses it offers. */
export type NotificationControl = {
  state: NotificationState;
  enabled: boolean;
  /** Turn the switch on; the first notification is what makes macOS ask (B14), elsewhere it is the one test (B19, B20). */
  turnOn: () => void;
  turnOff: () => void;
  sendTest: () => void;
};

/**
 * The switch the core keeps and what the system says about `permission`, read
 * with `usePermission` by the screen showing them. Null in a browser tab.
 */
export function useNotificationControl(actions: Actions, place: "wizard" | "settings", permission: NotificationPermission | null): NotificationControl | null {
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
  const turnOn = () => {
    actions.setDesktopNotifications(true);
    sendTest();
  };
  return { state, enabled, turnOn, turnOff: () => actions.setDesktopNotifications(false), sendTest };
}

/** The wizard's line under its description: what the system says, and the one press that answers it (B14-B20, B47). */
export function NotificationLine({ control }: { control: NotificationControl }) {
  const { t } = useInterfaceTranslation();
  const { state } = control;
  const line =
    state === "asking" ? (
      <span className="inline-flex items-center gap-xs text-body text-subtle-foreground" data-notification-line="asking">
        <LoaderCircleIcon aria-hidden="true" className="size-(--size-icon-sm) animate-spin" />
        {t("firstRun.notifications.asking")}
      </span>
    ) : state === "on" || state === "on_unchecked" ? (
      <Status tone="ok" data-notification-line="on">
        <span className="font-medium">{t("firstRun.notifications.on")}</span>
      </Status>
    ) : state === "blocked" ? (
      <Status tone="warn" data-notification-line="blocked">
        <span className="font-medium">{t("firstRun.notifications.blocked")}</span>
      </Status>
    ) : state === "unreadable" ? (
      <Status tone="muted" data-notification-line="unreadable">
        {t("firstRun.notifications.unreadable")}
      </Status>
    ) : state === "no_service" ? (
      <Status tone="muted" data-notification-line="no_service">
        {t("firstRun.notifications.noService")}
      </Status>
    ) : null;
  if (!line) return null;
  return (
    <div className="flex flex-wrap items-center gap-md" data-notification-state={state} data-notification-place="wizard">
      <span className="min-w-0 flex-1">{line}</span>
      <NotificationAction control={control} />
    </div>
  );
}

/** Open System Settings while macOS blocks Hide, Send a test once notifications can go, nothing otherwise. */
function NotificationAction({ control }: { control: NotificationControl }) {
  const { t } = useInterfaceTranslation();
  const bridge = hostBridge();
  if (control.state === "blocked") {
    return (
      <Button variant="secondary" onClick={() => bridge?.openNotificationSettings()} data-notification-open-settings="true">
        {t("firstRun.notifications.openSettings")}
      </Button>
    );
  }
  if (control.state === "on" || control.state === "on_unchecked" || control.state === "unreadable") {
    return (
      <Button variant="secondary" onClick={control.sendTest} data-notification-test="true">
        {t("firstRun.notifications.test")}
      </Button>
    );
  }
  return null;
}

/**
 * Settings > General's rows (B30): the switch, and while it is on, what macOS
 * allows with the press that answers it, or a test row where the system has
 * nothing to read. Where no notification service answers, the switch is
 * locked off and says why (B48).
 */
export function NotificationRows({ control }: { control: NotificationControl }) {
  const { t } = useInterfaceTranslation();
  const { state, enabled } = control;
  const noService = state === "no_service";
  const permission =
    state === "on" ? (
      <Status tone="ok" data-notification-line="on">{t("firstRun.settings.allowed")}</Status>
    ) : state === "blocked" ? (
      <Status tone="warn" data-notification-line="blocked">{t("firstRun.settings.blocked")}</Status>
    ) : state === "asking" ? (
      <Status tone="pending" data-notification-line="asking">{t("firstRun.notifications.asking")}</Status>
    ) : state === "unreadable" ? (
      <Status tone="muted" data-notification-line="unreadable">{t("firstRun.notifications.unreadable")}</Status>
    ) : null;
  return (
    <div className="divide-y divide-border" data-notification-state={state} data-notification-place="settings">
      <Row
        label={<label htmlFor="notification-switch">{t("firstRun.settings.switch")}</label>}
        detail={noService ? <Status tone="muted" data-notification-line="no_service">{t("firstRun.notifications.noService")}</Status> : undefined}
      >
        <Switch
          id="notification-switch"
          checked={enabled && !noService}
          disabled={noService}
          onCheckedChange={(next) => (next ? control.turnOn() : control.turnOff())}
          data-notification-switch="true"
        />
      </Row>
      {enabled && state === "on_unchecked" ? (
        <Row label={t("firstRun.settings.testRow")}>
          <NotificationAction control={control} />
        </Row>
      ) : enabled && !noService && state !== "off" ? (
        <Row label={t("firstRun.settings.permission")}>
          {permission}
          <NotificationAction control={control} />
        </Row>
      ) : null}
    </div>
  );
}
