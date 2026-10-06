import { useState } from "react";
import type { Actions } from "../actions";
import { Button } from "../components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../components/ui/select";
import { Group, Note, Row, Status, Value } from "../components/settings-rows";
import { useInterfaceTranslation } from "../i18n/client";
import { formatDateTime } from "../i18n/format";
import { INTERFACE_LANGUAGES, LANGUAGE_NAMES, isInterfaceLanguage, requireInterfaceLanguage } from "../i18n/locale";
import { diagnosticsText, environmentTone, herdrLine, shownIn } from "../settings";
import { useShellStore } from "../store";
import { useErrorSince } from "./useErrorSince";

/** The displayed choice always comes from the core, never an optimistic edit. */
function InterfaceLanguageRow({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const choice = useShellStore((state) => state.rest?.ui_state?.interface_language ?? null);
  const connected = useShellStore((state) => state.connection === "live");
  const [changedAt, setChangedAt] = useState<number | null>(null);
  const error = useErrorSince(changedAt, ["interface_language."]);
  return (
    <Group title={t("common.language")} note={t("common.languageDescription")}>
      <Row label={t("common.language")} detail={error ? <Note tone="error">{t("settings.notSaved", { reason: error })}</Note> : null}>
        <Select value={choice ?? "system"} disabled={!connected} onValueChange={(value) => {
          if (value !== "system" && !isInterfaceLanguage(value)) throw new Error("invalid_interface_language");
          const language = value === "system" ? null : value;
          if (language === choice) return;
          setChangedAt(Date.now());
          actions.setInterfaceLanguage(language);
        }}>
          <SelectTrigger aria-label={t("common.language")} data-interface-language={choice ?? "system"}><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="system" data-language-option="system">{t("common.systemLanguage")}</SelectItem>
            {INTERFACE_LANGUAGES.map((language) => <SelectItem key={language} value={language} data-language-option={language}>{LANGUAGE_NAMES[language]}</SelectItem>)}
          </SelectContent>
        </Select>
      </Row>
    </Group>
  );
}

export function GeneralTab({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const daemon = useShellStore((s) => s.daemon);
  const connection = useShellStore((s) => s.connection);
  const herdr = useShellStore((s) => s.rest?.status?.herdr);
  const environment = useShellStore((s) => s.rest?.status?.environment);
  const diagnostics = useShellStore((s) => s.rest?.status?.diagnostics);
  const lastError = useShellStore((s) => s.rest?.status?.last_error);
  const [copy, setCopy] = useState<"idle" | "copied" | "failed">("idle");
  const line = herdrLine(herdr, t);
  const copyDiagnostics = () => {
    const text = diagnosticsText({
      daemon,
      connection,
      herdr,
      environment: environment ?? [],
      diagnostics: diagnostics ?? [],
      lastError,
      userAgent: navigator.userAgent,
    });
    void navigator.clipboard.writeText(text).then(
      () => setCopy("copied"),
      () => setCopy("failed"),
    );
  };
  return (
    <>
      <InterfaceLanguageRow actions={actions} />
      <Group title={t("settings.daemon")} note={t("settings.daemonDescription")}>
        <Row label={t("settings.version")}>
          <Value>{daemon ? `hided ${daemon.version}` : t("settings.unavailable")}</Value>
        </Row>
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
      </Group>
      <Group title={t("settings.herdrRuntime")}>
        <Row
          label={t("settings.connection")}
          detail={
            line.tone !== "ok" && herdr?.message ? (
              <Note tone={line.tone === "error" ? "error" : "warn"} data-herdr-message="true">
                {herdr.message}
              </Note>
            ) : null
          }
        >
          <Status tone={line.tone} data-herdr-state={herdr?.state ?? "unavailable"}>
            {line.text}
          </Status>
        </Row>
        <Row label={t("settings.version")}>
          <Value>{shownIn(herdr?.received_version, t)}</Value>
        </Row>
        <Row label={t("settings.protocol")}>
          <Value>{herdr?.received_protocol != null ? t("settings.protocolValue", { received: String(herdr.received_protocol), expected: shownIn(herdr.expected_protocol, t) }) : t("settings.unavailable")}</Value>
        </Row>
        <Row label={t("settings.socket")}>
          <Value>{shownIn(herdr?.socket_path ?? daemon?.herdr_socket_path, t)}</Value>
        </Row>
        <Row label={t("settings.binary")}>
          <Value>{shownIn(daemon?.herdr_bin_path, t)}</Value>
        </Row>
      </Group>
      {environment && environment.length > 0 ? (
        <Group title={t("settings.environment")}>
          {environment.map((row) => (
            <Row key={row.key} label={<span className="font-mono">{row.key}</span>} detail={row.message ? <Note>{row.message}</Note> : null}>
              <Status tone={environmentTone(row.state, row.required)}>{row.state.replace(/_/g, " ")}</Status>
            </Row>
          ))}
        </Group>
      ) : null}
      <Group
        title={t("settings.diagnostics")}
        note={t("settings.diagnosticsDescription")}
      >
        <Row label={t("settings.recent")} detail={<DiagnosticList />}>
          <Button variant="secondary" onClick={copyDiagnostics} data-copy-diagnostics="true">
            {t("settings.copyDiagnostics")}
          </Button>
          {copy === "copied" ? <Status tone="ok">{t("common.copied")}</Status> : null}
          {copy === "failed" ? <Status tone="error">{t("settings.clipboardRefused")}</Status> : null}
        </Row>
      </Group>
      <Group title={t("settings.authentication")}>
        <Row label={<span className="text-body text-subtle-foreground">{t("settings.authenticationDescription")}</span>} />
      </Group>
    </>
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
