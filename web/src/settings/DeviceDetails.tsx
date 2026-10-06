import { Dialog, DialogBody, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "../components/ui/dialog";
import { Row, Value } from "../components/settings-rows";
import { useInterfaceTranslation } from "../i18n/client";
import { hostLine } from "../settings";
import type { Device } from "../snapshot";
import { KitParts } from "./MachineKit";

/**
 * Connection details (PRD settings-cleanup B55): what a healthy device row no
 * longer carries. Where its helper lives and the host key the consent is bound
 * to for a device, the folder of the hide command, and every part of Hide's
 * kit with where it is. This is the one place a working kit is listed.
 */
export function DeviceDetails({ device, onClose }: { device: Device; onClose: () => void }) {
  const { t } = useInterfaceTranslation();
  const host = device.host;
  const remote = device.kind === "remote";
  const helper = remote ? hostLine(host, t) : null;
  return (
    <Dialog open onOpenChange={(next) => { if (!next) onClose(); }}>
      <DialogContent data-device-details-dialog={device.id}>
        <DialogHeader>
          <DialogTitle>{t("devices.detailsTitle", { name: device.label })}</DialogTitle>
          <DialogDescription>{t("devices.detailsDescription")}</DialogDescription>
        </DialogHeader>
        <DialogBody className="space-y-md">
          <div className="divide-y divide-border rounded-md border border-border bg-card">
            {remote && helper ? <Row label={t("devices.detail.helper")}><Value mono={false}>{helper.text}</Value></Row> : null}
            {host?.helper_root ? <Row label={t("devices.detail.helperFolder")}><Value data-device-detail="helper-folder">{host.helper_root}</Value></Row> : null}
            {host?.helper_path ? <Row label={t("devices.detail.helperProgram")}><Value data-device-detail="helper-path">{host.helper_path}</Value></Row> : null}
            {host?.cli_dir ? <Row label={t("devices.detail.commandFolder")}><Value data-device-detail="command-folder">{host.cli_dir}</Value></Row> : null}
            {remote && host?.bound_identity ? <Row label={t("devices.detail.hostKey")}><Value data-device-detail="host-key">{host.bound_identity}</Value></Row> : null}
          </div>
          <section className="space-y-xs">
            <h4 className="text-body font-semibold text-subtle-foreground">{t("devices.detail.kit")}</h4>
            <KitParts device={device} />
          </section>
        </DialogBody>
      </DialogContent>
    </Dialog>
  );
}
