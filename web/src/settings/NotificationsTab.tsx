import type { Actions } from "../actions";
import { Group, Note, Row } from "../components/settings-rows";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../components/ui/select";
import { hostBridge } from "../host";
import { useInterfaceTranslation } from "../i18n/client";
import { PUSH_CHOICES, type PushMode } from "../mobileSettings";
import { NotificationRows, useNotificationControl, usePermission } from "../NotificationControls";
import { thisMachineKey } from "../screenMachine";
import { useShellStore } from "../store";

/**
 * Every notification in one page (PRD settings-left-nav B6): this machine's
 * desktop notifications, which only the desktop app has (PRD
 * first-run-onboarding D-14, D-21), and the push to a paired phone.
 */
export function NotificationsTab({ actions }: { actions: Actions }) {
  return (
    <>
      {hostBridge() ? <DesktopGroup actions={actions} /> : null}
      <PhoneGroup actions={actions} />
    </>
  );
}

/** Notify me when an agent needs me, and what the system allows, read while this page is open (B18, B30). */
function DesktopGroup({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const control = useNotificationControl(actions, "settings", usePermission());
  if (!control) return null;
  const sends = control.enabled && control.state !== "no_service";
  return (
    <Group title={t(thisMachineKey(null))} note={sends ? t("firstRun.settings.sentWhen") : undefined} data-settings-group="notifications">
      <NotificationRows control={control} />
    </Group>
  );
}

/** The phone push mode, the daemon's `mobile` frame's (PRD mobile-companion B59). */
function PhoneGroup({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const mobile = useShellStore((s) => s.mobile);
  return (
    <Group title={t("settings.notifications.phone")} data-settings-group="phone">
      {mobile ? <PushModeSelect mode={mobile.push_mode} actions={actions} /> : <Note tone="pending">{t("mobileSetup.connecting")}</Note>}
    </Group>
  );
}

/** Push to the phone in one dropdown; only the chosen item's description shows (B59). */
function PushModeSelect({ mode, actions }: { mode: PushMode; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const chosen = PUSH_CHOICES.find((choice) => choice.id === mode);
  return (
    <Row label={t("mobileSetup.pushTo")} detail={chosen ? <Note data-push-detail={chosen.id}>{t(chosen.detail)}</Note> : null}>
      <Select value={mode} onValueChange={(value) => actions.setPushMode(value as PushMode)}>
        <SelectTrigger aria-label={t("mobileSetup.push")} data-push-select="true">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {PUSH_CHOICES.map((choice) => (
            <SelectItem key={choice.id} value={choice.id} data-push-choice={choice.id}>
              {t(choice.label)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </Row>
  );
}
