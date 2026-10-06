import { useEffect, useState } from "react";
import type { Actions } from "../actions";
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from "../components/ui/alert-dialog";
import { Button } from "../components/ui/button";
import { Dialog, DialogBody, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "../components/ui/dialog";
import { Group, Note, Row, Status } from "../components/settings-rows";
import { latestDraft } from "../editor/draft";
import { useInterfaceTranslation } from "../i18n/client";
import { formatDateTime } from "../i18n/format";
import { requireInterfaceLanguage } from "../i18n/locale";
import {
  canRetryDevice,
  deviceFacts,
  deviceLine,
  deviceProblemLine,
  deviceRemovalLines,
  draftExported,
  hostLine,
  kitRemovalLine,
  unstoredDeviceDrafts,
} from "../settings";
import type { Device } from "../snapshot";
import { useShellStore } from "../store";
import { AddDevice } from "./AddDevice";
import { KitTerms, MachineKit } from "./MachineKit";
import { useErrorSince } from "./useErrorSince";

export function DevicesTab({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const devices = useShellStore((s) => s.rest?.navigator?.devices);
  const focused = useShellStore((s) => s.rest?.navigator?.focused_device_id ?? "local");
  const remote = useShellStore((s) => s.rest?.status?.remote);
  const [removing, setRemoving] = useState<Device | null>(null);
  const [allowing, setAllowing] = useState<Device | null>(null);
  const [revoking, setRevoking] = useState<Device | null>(null);
  // The removal waits for the device's drafts to be stored (B26, B44).
  const [removalBusy, setRemovalBusy] = useState(false);
  const [actedAt, setActedAt] = useState<number | null>(null);
  const deviceError = useErrorSince(actedAt, ["device.", "remote.", "kit."]);
  // Each machine's kit is read once when this tab opens, so a part removed
  // by hand shows without a relaunch (B7).
  const live = useShellStore((s) => s.connection === "live");
  useEffect(() => {
    if (live) actions.checkKit();
  }, [actions, live]);
  const rows = devices ?? [];
  const localHost = rows.find((device) => device.kind !== "remote")?.host ?? null;
  const localRoot = localHost?.helper_root ?? null;
  const localCliDir = localHost?.cli_dir ?? null;
  const remoteRows = rows.filter((device) => device.kind === "remote");
  const registrations = useShellStore((s) => s.rest?.ui_state?.workspace_registrations);
  const editorTabs = useShellStore((s) => s.editor?.tabs);
  const recoveryDrafts = useShellStore((s) => s.recoveryDrafts);
  const exportedDrafts = useShellStore((s) => s.exportedDrafts);
  const bufferWarnings = useShellStore((s) => s.bufferWarnings);
  const released = draftExported(exportedDrafts, latestDraft);
  const removalLines = removing
    ? deviceRemovalLines(removing.id, registrations ?? [], editorTabs ?? [], recoveryDrafts, (tabId) => bufferWarnings.has(tabId) && released(tabId), t)
    : [];
  const unstored = removing ? unstoredDeviceDrafts(removing.id, editorTabs ?? [], bufferWarnings, released) : [];
  return (
    <>
      <Group title={t("settings.tabs.devices")} note={t("devices.description")}>
        {rows.map((device) => {
          const status = remote?.find((row) => row.target_id === device.id);
          const line = deviceLine(device, status, t);
          return (
            <Row
              key={device.id}
              label={
                <span className="flex min-w-0 flex-col">
                  <span className="break-words font-semibold">{device.label}</span>
                  <span className="break-all font-mono text-caption text-muted-foreground">{device.kind === "remote" ? device.ssh_alias : t("devices.localAlias")}</span>
                </span>
              }
              detail={
                <>
                  <DeviceConnection device={device} facts={deviceFacts(device, status, t)} />
                  {device.kind === "remote" ? <DeviceHelper device={device} /> : null}
                  <MachineKit device={device} actions={actions} />
                  {device.test ? <DeviceTest test={device.test} /> : null}
                </>
              }
            >
              <Status tone={line.tone} data-device-state={`${device.id}:${device.state}`}>
                {line.text}
              </Status>
              {device.kit?.offers_reinstall && !device.kit.unavailable ? (
                <Button
                  variant="secondary"
                  disabled={device.kit.busy}
                  onClick={() => {
                    setActedAt(Date.now());
                    actions.reinstallKit(device.id);
                  }}
                  data-kit-reinstall={device.id}
                >
                  {device.kit.busy ? t("settings.reinstalling") : t("settings.reinstall")}
                </Button>
              ) : null}
              {focused === device.id ? (
                <Status tone="muted">{t("devices.selected")}</Status>
              ) : (
                <Button variant="ghost" onClick={() => actions.focusDevice(device.id)} data-device-select={device.id}>
                  {t("common.select")}
                </Button>
              )}
              {device.kind === "remote" ? (
                <>
                  <Button
                    variant="secondary"
                    disabled={device.test?.state === "running"}
                    onClick={() => {
                      setActedAt(Date.now());
                      actions.testDevice(device.id);
                    }}
                    data-device-test={device.id}
                  >
                    {device.test?.state === "running" ? t("devices.testing") : t("devices.test")}
                  </Button>
                  {canRetryDevice(device, status) ? (
                    <Button
                      variant="secondary"
                      onClick={() => {
                        setActedAt(Date.now());
                        actions.retryDevice(device.id);
                      }}
                      data-device-retry={device.id}
                    >
                      {t("common.retry")}
                    </Button>
                  ) : null}
                  {device.host?.consent === "granted" && device.host.state !== "identity_changed" ? (
                    <>
                      {device.host.state === "unavailable" ? (
                        <Button
                          variant="secondary"
                          onClick={() => {
                            setActedAt(Date.now());
                            actions.retryDeviceHost(device.id);
                          }}
                          data-device-host-retry={device.id}
                        >
                          {t("devices.retryHelper")}
                        </Button>
                      ) : null}
                      <Button variant="ghost" onClick={() => setRevoking(device)} data-device-host-revoke={device.id}>
                        {t("devices.revokeHelperMenu")}
                      </Button>
                    </>
                  ) : (
                    <Button variant="secondary" onClick={() => setAllowing(device)} data-device-host-allow={device.id}>
                      {t("devices.allowInstallMenu")}
                    </Button>
                  )}
                  <Button variant="ghost" onClick={() => setRemoving(device)} data-device-remove={device.id}>
                    {t("devices.removeMenu")}
                  </Button>
                </>
              ) : null}
            </Row>
          );
        })}
        {remoteRows.length === 0 ? <Row label={<Note>{t("devices.noRemote")}</Note>} /> : null}
        {deviceError ? <Row label={<Note tone="error" data-device-error="true">{deviceError}</Note>} /> : null}
      </Group>
      <AddDevice actions={actions} devices={rows} helperRoot={localRoot} cliDir={localCliDir} />
      {allowing ? (
        <Dialog open onOpenChange={(next) => { if (!next) setAllowing(null); }}>
          <DialogContent data-device-host-allow-confirm={allowing.id}>
            <DialogHeader>
              <DialogTitle>{t("devices.installTitle", { name: allowing.label })}</DialogTitle>
            </DialogHeader>
            <DialogBody className="space-y-sm">
              {allowing.host?.state === "identity_changed" ? (
                <p className="text-body text-warning">
                  {t("devices.identityChangedDescription", { alias: allowing.ssh_alias ?? allowing.label })}
                </p>
              ) : null}
              <KitTerms helperRoot={allowing.host?.helper_root ?? localRoot} cliDir={allowing.host?.cli_dir ?? localCliDir} />
            </DialogBody>
            <DialogFooter>
              <Button variant="secondary" onClick={() => setAllowing(null)}>{t("devices.notNow")}</Button>
              <Button
                data-device-host-allow-go="true"
                onClick={() => {
                  setActedAt(Date.now());
                  actions.setDeviceHostConsent(allowing.id, true);
                  setAllowing(null);
                }}
              >
                {t("devices.allowInstall")}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      ) : null}
      {revoking ? (
        <AlertDialog open onOpenChange={(next) => { if (!next) setRevoking(null); }}>
          <AlertDialogContent data-device-host-revoke-confirm={revoking.id}>
            <AlertDialogHeader>
              <AlertDialogTitle>{t("devices.revokeTitle", { name: revoking.label })}</AlertDialogTitle>
              <AlertDialogDescription>{t("devices.revokeDescription", { alias: revoking.ssh_alias ?? revoking.label })}</AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>{t("devices.keepAllowed")}</AlertDialogCancel>
              <AlertDialogAction
                data-device-host-revoke-go="true"
                onClick={() => {
                  setActedAt(Date.now());
                  actions.setDeviceHostConsent(revoking.id, false);
                }}
              >
                {t("devices.revokeHelper")}
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      ) : null}
      {removing ? (
        <AlertDialog open onOpenChange={(next) => { if (!next && !removalBusy) setRemoving(null); }}>
          <AlertDialogContent data-device-remove-confirm={removing.id}>
            <AlertDialogHeader>
              <AlertDialogTitle>{t("devices.removeTitle", { name: removing.label })}</AlertDialogTitle>
              <AlertDialogDescription>{t("devices.removeDescription", { alias: removing.ssh_alias ?? removing.label })}</AlertDialogDescription>
            </AlertDialogHeader>
            <p className="text-body text-subtle-foreground" data-device-remove-kit={removing.id}>
              {kitRemovalLine(removing, t)}
            </p>
            {removalLines.map((line) => (
              <p key={line} className="text-body text-subtle-foreground" data-device-remove-effect="true">
                {line}
              </p>
            ))}
            {unstored.length > 0 ? (
              <Note tone="warn" data-device-remove-unstored="true">
                {t("devices.unstored", { count: unstored.length, paths: unstored.join(", ") })}
              </Note>
            ) : null}
            <AlertDialogFooter>
              {/* A removal already storing drafts goes out when they land, so it
                  cannot be kept from here. Plain Button, not AlertDialogAction:
                  it has to stay open through the async removal. */}
              <Button variant="secondary" disabled={removalBusy} onClick={() => setRemoving(null)}>{t("devices.keepDevice")}</Button>
              <Button
                variant="destructive"
                disabled={unstored.length > 0 || removalBusy}
                data-device-remove-go="true"
                onClick={() => {
                  const target = removing;
                  setActedAt(Date.now());
                  setRemovalBusy(true);
                  // A draft that failed to store while it was flushed keeps
                  // the dialog open, where its note now names it.
                  void actions
                    .removeDevice(target.id)
                    .then((kept) => {
                      if (kept.length === 0) setRemoving((current) => (current?.id === target.id ? null : current));
                    })
                    .finally(() => setRemovalBusy(false));
                }}
              >
                {removalBusy ? t("devices.storingDrafts") : t("devices.removeNamed", { name: removing.label })}
              </Button>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      ) : null}
    </>
  );
}

/**
 * What the device itself reported: its Herdr version and helper platform, and
 * when the connection failed, which step refused it and what to do (B36, B38).
 */
function DeviceConnection({ device, facts }: { device: Device; facts: string[] }) {
  const { t } = useInterfaceTranslation();
  if (device.kind !== "remote") return null;
  const problem = device.state !== "ready" ? deviceProblemLine(device.problem, device.ssh_alias, t) : null;
  return (
    <>
      {facts.length > 0 ? (
        <p className="break-words font-mono text-caption text-muted-foreground" data-device-facts={device.id}>
          {facts.join(" · ")}
        </p>
      ) : null}
      {problem ? (
        <div className="mt-xxs space-y-xxs" data-device-problem={`${device.id}:${device.problem}`}>
          <Status tone="warn">{problem.headline}</Status>
          <Note tone="warn">{problem.action}</Note>
        </div>
      ) : null}
      {device.state !== "ready" && device.message ? (
        problem ? (
          <p className="break-words font-mono text-caption text-muted-foreground">{device.message}</p>
        ) : (
          <Note tone="warn">{device.message}</Note>
        )
      ) : null}
    </>
  );
}

/** Whether file and Git work may run on a device, where its helper lives, and the identity the consent is bound to. */
function DeviceHelper({ device }: { device: Device }) {
  const { t } = useInterfaceTranslation();
  const host = device.host;
  const line = hostLine(host, t);
  return (
    <div className="mt-xs space-y-xxs" data-device-host={`${device.id}:${host?.state ?? "unknown"}`}>
      <Status tone={line.tone}>{line.text}</Status>
      {host?.consent === "granted" && host.helper_root ? <p className="break-all font-mono text-caption text-muted-foreground">{t("devices.installsTo", { path: host.helper_root })}</p> : null}
      {host?.bound_identity ? <p className="break-all font-mono text-caption text-muted-foreground">{t("devices.boundTo", { identity: host.bound_identity })}</p> : null}
      {host && host.state !== "ready" && host.message ? <Note tone={line.tone === "muted" ? "muted" : "warn"}>{host.message}</Note> : null}
    </div>
  );
}

function DeviceTest({ test }: { test: NonNullable<Device["test"]> }) {
  const { t, i18n } = useInterfaceTranslation();
  const language = requireInterfaceLanguage(i18n.language);
  const tone = test.state === "running" ? "pending" : test.state === "passed" ? "ok" : "warn";
  // A finished test names when it ran: it is that attempt's result, and it
  // stays beside the row after the connection itself has changed.
  const time = test.checked_at_unix_ms === null ? null : formatDateTime(language, test.checked_at_unix_ms, { timeStyle: "medium" });
  const headline =
    test.state === "running"
      ? t("devices.testRunning")
      : test.state === "passed"
        ? time === null ? t("devices.testPassed") : t("devices.testPassedAt", { time })
        : time === null ? t("devices.testFailed") : t("devices.testFailedAt", { time });
  return (
    <div className="mt-xs space-y-xxs" data-device-test-state={test.state}>
      <Status tone={tone}>{headline}</Status>
      {test.stages.map((stage) => (
        <div key={stage.stage} className="flex gap-xs text-caption">
          <span className={stage.state === "passed" ? "text-success" : stage.state === "pending" ? "text-muted-foreground" : "text-warning"} aria-hidden="true">
            {stage.state === "passed" ? "✓" : stage.state === "pending" ? "…" : "✕"}
          </span>
          <span className="sr-only">{stage.state === "passed" ? t("devices.stage.passed") : stage.state === "pending" ? t("devices.stage.notRun") : t("devices.stage.failed")}</span>
          <span className="w-[var(--size-device-test-stage-col)] shrink-0 font-mono text-foreground">{stage.stage}</span>
          <span className="min-w-0 break-words text-subtle-foreground">{stage.detail}</span>
        </div>
      ))}
    </div>
  );
}
