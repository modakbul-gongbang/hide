import { useEffect, useState } from "react";
import type { Actions } from "../actions";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { Group, Note, Row } from "../components/settings-rows";
import { useInterfaceTranslation } from "../i18n/client";
import { aliasProblem, deviceIdFor, socketProblem } from "../settings";
import type { Device } from "../snapshot";
import { KitTerms } from "./MachineKit";
import { useErrorSince } from "./useErrorSince";

export function AddDevice({ actions, devices, helperRoot, cliDir }: { actions: Actions; devices: Device[]; helperRoot: string | null; cliDir: string | null }) {
  const { t } = useInterfaceTranslation();
  const [label, setLabel] = useState("");
  const [alias, setAlias] = useState("");
  const [socket, setSocket] = useState("");
  const [submitted, setSubmitted] = useState<{ id: string; at: number } | null>(null);
  const error = useErrorSince(submitted?.at ?? null, ["device."]);
  const problem = alias ? aliasProblem(alias, t) : null;
  const added = submitted ? devices.some((device) => device.id === submitted.id) : false;
  useEffect(() => {
    if (!added) return;
    setLabel("");
    setAlias("");
    setSocket("");
    setSubmitted(null);
  }, [added]);
  const pending = submitted !== null && !added && error === null;
  const blocked = pending || !label.trim() || !alias.trim() || problem !== null || socketProblem(socket, t) !== null;
  // Adding is where the whole kit is agreed to, once (PRD device-parity
  // D-12): the terms are on the form and there is one way to add.
  const submit = () => {
    if (blocked) return;
    const id = deviceIdFor(alias, devices.map((device) => device.id));
    setSubmitted({ id, at: Date.now() });
    actions.registerDevice(id, label.trim(), alias.trim(), { hostConsent: true, herdrSocketPath: socket.trim() || null });
  };
  return (
    <Group title={t("devices.addTitle")} note={t("devices.addDescription")}>
      <Row label={t("devices.label")}>
        <Input value={label} disabled={pending} placeholder={t("devices.labelPlaceholder")} aria-label={t("devices.labelAria")} className="w-(--size-settings-control-w)" onChange={(event) => setLabel(event.target.value)} data-device-label="true" />
      </Row>
      <Row label={t("devices.sshAlias")} detail={problem ? <Note tone="warn">{problem}</Note> : error ? <Note tone="error" data-add-device-error="true">{error}</Note> : null}>
        <Input
          mono
          value={alias}
          disabled={pending}
          placeholder="studio"
          aria-label={t("devices.sshAlias")}
          className="w-(--size-settings-control-w)"
          onChange={(event) => setAlias(event.target.value)}
          data-device-alias="true"
        />
      </Row>
      <Row label={t("devices.herdrSocket")} detail={socketProblem(socket, t) ? <Note tone="warn">{socketProblem(socket, t)}</Note> : <Note>{t("devices.socketDescription")}</Note>}>
        <Input
          mono
          value={socket}
          disabled={pending}
          placeholder={t("devices.defaultServer")}
          aria-label={t("devices.socketAria")}
          className="w-(--size-settings-control-w)"
          onChange={(event) => setSocket(event.target.value)}
          data-device-socket="true"
        />
      </Row>
      <Row label={<span className="text-subtle-foreground">{t("devices.whatInstalls")}</span>} detail={<KitTerms helperRoot={helperRoot} cliDir={cliDir} />} />
      <Row label="">
        <Button disabled={blocked} onClick={submit} data-add-device="true">
          {pending ? t("devices.adding") : t("common.add")}
        </Button>
      </Row>
    </Group>
  );
}
