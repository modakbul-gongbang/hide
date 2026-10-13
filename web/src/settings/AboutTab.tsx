import { useState } from "react";
import { Button } from "../components/ui/button";
import { Disclosure, Group, Note, Row, Status, Value } from "../components/settings-rows";
import { useInterfaceTranslation } from "../i18n/client";
import { formatDateTime } from "../i18n/format";
import { requireInterfaceLanguage } from "../i18n/locale";
import { diagnosticsInputOf, diagnosticsText, environmentTone, herdrLine, herdrProtocolText, shownIn } from "../settings";
import { useShellStore } from "../store";

/** What this hide is: its version, the Herdr behind it, and the details folded away unless something is wrong. */
export function AboutTab() {
  const { t } = useInterfaceTranslation();
  const daemon = useShellStore((s) => s.daemon);
  const connection = useShellStore((s) => s.connection);
  const herdr = useShellStore((s) => s.rest?.status?.herdr);
  const environment = useShellStore((s) => s.rest?.status?.environment);
  const [copy, setCopy] = useState<"idle" | "copied" | "failed">("idle");
  const line = herdrLine(herdr, t);
  const protocol = herdrProtocolText(herdr, t);
  const copyDiagnostics = () => {
    const text = diagnosticsText(diagnosticsInputOf(useShellStore.getState(), navigator.userAgent));
    void navigator.clipboard.writeText(text).then(
      () => setCopy("copied"),
      () => setCopy("failed"),
    );
  };
  return (
    <Group data-settings-group="about">
      <Row label={t("settings.hideName")}>
        <Value>{daemon ? daemon.version : t("settings.unavailable")}</Value>
      </Row>
      <Row label="Herdr">
        <Status tone={line.tone} data-herdr-state={herdr?.state ?? "unavailable"}>
          {herdr?.state === "connected" && herdr.received_version ? `${line.text} · ${herdr.received_version}` : line.text}
        </Status>
      </Row>
      <Disclosure title={t("settings.details")} data-settings-details="true">
        <Row label={t("settings.process")}>
          <Value>{daemon ? t("settings.processValue", { pid: String(daemon.pid), schema: String(daemon.schema_version) }) : t("settings.unavailable")}</Value>
        </Row>
        <Row label={t("settings.lifetime")}>
          <Value mono={false}>{daemon ? (daemon.keep_alive ? t("settings.keptAlive") : t("settings.exitsAfter", { minutes: Math.round(daemon.idle_secs / 60) })) : t("settings.unavailable")}</Value>
        </Row>
        <Row label={t("settings.stateFile")}>
          <Value>{shownIn(daemon?.core_state_path, t)}</Value>
        </Row>
        <Row label={t("settings.pageConnection")}>
          <Status tone={connection === "live" ? "ok" : "warn"} data-page-connection={connection}>
            {t(`settings.connection.${connection}`)}
          </Status>
        </Row>
        <Row
          label={t("settings.herdrRuntime")}
          detail={
            line.tone !== "ok" && herdr?.message ? (
              <Note tone={line.tone === "error" ? "error" : "warn"} data-herdr-message="true">
                {herdr.message}
              </Note>
            ) : null
          }
        />
        <Row label={t("settings.version")}>
          <Value data-herdr-version="true">{shownIn(herdr?.received_version, t)}</Value>
        </Row>
        <Row label={t("settings.protocol")}>
          <Value data-herdr-protocol={protocol.matches ? "matches" : "mismatch"}>{protocol.text}</Value>
        </Row>
        <Row label={t("settings.socket")}>
          <Value>{shownIn(herdr?.socket_path ?? daemon?.herdr_socket_path, t)}</Value>
        </Row>
        <Row label={t("settings.binary")}>
          <Value>{shownIn(daemon?.herdr_bin_path, t)}</Value>
        </Row>
        {(environment ?? []).map((row) => (
          <Row key={row.key} label={<span className="font-mono">{row.key}</span>} detail={row.message ? <Note>{row.message}</Note> : null}>
            <Status tone={environmentTone(row.state, row.required)}>{row.state.replace(/_/g, " ")}</Status>
          </Row>
        ))}
        <Row label={t("settings.recent")} detail={<DiagnosticList />}>
          <Button variant="secondary" onClick={copyDiagnostics} data-copy-diagnostics="true">
            {t("settings.copyDiagnostics")}
          </Button>
          {copy === "copied" ? <Status tone="ok">{t("common.copied")}</Status> : null}
          {copy === "failed" ? <Status tone="error">{t("settings.clipboardRefused")}</Status> : null}
        </Row>
      </Disclosure>
    </Group>
  );
}

function DiagnosticList() {
  const { t, i18n } = useInterfaceTranslation();
  const language = requireInterfaceLanguage(i18n.language);
  const diagnostics = useShellStore((s) => s.rest?.status?.diagnostics);
  const rows = (diagnostics ?? []).slice(-8).reverse();
  if (rows.length === 0) return <Note>{t("settings.noDiagnostics")}</Note>;
  return (
    <ul className="space-y-xxs" data-diagnostics={rows.length}>
      {rows.map((row) => (
        <li key={`${row.occurred_at}-${row.kind}`} className="break-words font-mono text-caption text-subtle-foreground">
          <span className="text-muted-foreground">{formatDateTime(language, row.occurred_at, { timeStyle: "medium" })}</span> {row.kind}: {row.message}
        </li>
      ))}
    </ul>
  );
}
