// The web Settings sheet (PRD S5 B1-B10, B18-B22): the native sheet's sections
// less Pet, each row reading a value the core or the daemon reported and each
// edit sent as the one core event that owns it. Nothing here decides a value;
// a pending edit shows as pending until the snapshot says it landed.

import { useInterfaceTranslation } from "./i18n/client";
import { formatDateTime } from "./i18n/format";
import { INTERFACE_LANGUAGES, LANGUAGE_NAMES, isInterfaceLanguage, requireInterfaceLanguage } from "./i18n/locale";
import { TriangleAlertIcon, XIcon } from "lucide-react";
import { useEffect, useState, type KeyboardEvent } from "react";
import type { Actions } from "./actions";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "./components/ui/alert-dialog";
import { Button } from "./components/ui/button";
import { Dialog, DialogBody, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "./components/ui/dialog";
import { Input } from "./components/ui/input";
import { Kbd } from "./components/ui/kbd";
import { RadioGroup, RadioGroupItem } from "./components/ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./components/ui/select";
import { Slider } from "./components/ui/slider";
import { Switch } from "./components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "./components/ui/tabs";
import { ToggleGroup, ToggleGroupItem } from "./components/ui/toggle-group";
import { Hint } from "./components/ui/tooltip";
import { Group, Note, Row, Status, Value } from "./components/settings-rows";
import { ACCENT_STORED_HEX } from "./generated/accents";
import { THEME_CHOICES, accentNameOf, readTheme, type AccentName, type ThemeChoice } from "./theme";
import {
  ACCENT_CHOICES,
  FONT_SIZE_BASE,
  FONT_SIZE_MAX,
  FONT_SIZE_MIN,
  SETTINGS_TABS,
  SLEEP_AFTER_CHOICES,
  githubAccess,
  githubAccessLine,
  issueSourceChoices,
  shownIn,
  sleepAfterChoice,
  sleepAfterLabel,
  sleepingCount,
  aliasProblem,
  canRetryDevice,
  deviceFacts,
  deviceRemovalLines,
  deviceIdFor,
  unstoredDeviceDrafts,
  draftExported,
  deviceLine,
  deviceProblemLine,
  diagnosticsText,
  kitConsentTerms,
  kitRemovalLine,
  herdrLine,
  hostLine,
  kitAgentGets,
  kitAgentLine,
  kitAgentMachines,
  kitAgentNeedsReinstall,
  kitAgentSwitch,
  kitPartLine,
  kitPartSwitch,
  offeredModels,
  providerLine,
  environmentTone,
  socketProblem,
  usableAccent,
  usableFontSize,
  type IssueSourceChoice,
  type SettingsTab,
} from "./settings";
import {
  AREA_COMMANDS,
  EDITABLE_PANE_COMMANDS,
  bindingProblem,
  chordEquals,
  chordFromEvent,
  defaultChord,
  displayChord,
  hostChord,
  resolvedRegistry,
  serializeStoredChord,
  storedBindings,
  storedKey,
  type BindingProblem,
  type Chord,
  type CommandId,
  type KeySystem,
  sheetRows,
} from "./shortcuts";
import { commandTitle } from "./commandTitle";
import { bindingProblemText, commandLabel, sheetRowTitle } from "./shortcutLabels";
import type { Device, IssueSettings } from "./snapshot";
import { latestDraft } from "./editor/draft";
import { MobileTab } from "./MobileTab";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { hostKind, keySystem, type HostKind } from "./host";

/** The core's newest error, if it arrived after `since` and is one of `kinds`' prefixes. */
function useErrorSince(since: number | null, prefixes: readonly string[]): string | null {
  const error = useShellStore((s) => s.rest?.status?.last_error ?? null);
  if (since === null || !error || error.occurred_at < since) return null;
  return prefixes.some((prefix) => error.kind.startsWith(prefix)) ? error.message : null;
}

export function SettingsGate({ actions }: { actions: Actions }) {
  const open = useUiStore((s) => s.overlay === "settings");
  return open ? <SettingsSheet actions={actions} /> : null;
}

function SettingsSheet({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const close = () => useUiStore.getState().closeOverlay("settings");
  const [tab, setTab] = useState<SettingsTab>(() => useUiStore.getState().settingsTab);
  const subtitle = t(`settings.tabs.${tab}Description`);
  const daemon = useShellStore((s) => s.daemon);
  const selected = useShellStore((s) => {
    const id = s.rest?.navigator?.focused_device_id ?? "local";
    return s.rest?.navigator?.devices?.find((device) => device.id === id) ?? null;
  });
  return (
    <Dialog open onOpenChange={(next) => { if (!next) close(); }}>
      <DialogContent data-settings="true" className="w-(--size-settings-sheet-w) h-(--size-settings-sheet-h-max)">
        <header className="flex items-start gap-md border-b border-border px-xl py-lg">
          <div className="min-w-0 flex-1">
            <DialogTitle className="text-headline">{t("common.settings")}</DialogTitle>
            <DialogDescription>{subtitle}</DialogDescription>
            <p className="mt-xxs text-caption text-muted-foreground" data-settings-owner={daemon?.host_name ?? "unknown"}>
              {t("settings.owner", { host: daemon?.host_name ?? t("settings.daemonMachine") })}
              {selected?.kind === "remote" ? ` ${t("settings.ownerRemote", { device: selected.label })}` : ""}
            </p>
          </div>
          <Hint label={t("settings.close")} shortcut="Esc">
            <Button variant="ghost" size="icon-sm" aria-label={t("settings.close")} onClick={close} data-settings-close="true">
              <XIcon />
            </Button>
          </Hint>
        </header>
        {/* Radix Tabs owns the roving tabindex and Left/Right (plus Home/End)
            arrow-key navigation the hand-built tablist used to implement. */}
        <Tabs value={tab} onValueChange={(value) => setTab(value as SettingsTab)} className="min-h-0 flex-1 flex-col gap-none">
          <TabsList aria-label={t("settings.section")} className="w-full flex-wrap justify-start gap-xs rounded-none border-b border-border bg-sidebar px-xl py-sm">
            {SETTINGS_TABS.map((row) => (
              <TabsTrigger key={row} value={row} data-settings-tab={row}>
                {t(`settings.tabs.${row}`)}
              </TabsTrigger>
            ))}
          </TabsList>
          <TabsContent value={tab} className="min-h-0 flex-1 overflow-auto px-xl py-lg">
            {tab === "general" ? <GeneralTab actions={actions} /> : null}
            {tab === "appearance" ? <AppearanceTab actions={actions} /> : null}
            {tab === "agents" ? <AgentsTab actions={actions} /> : null}
            {tab === "issues" ? <IssuesTab actions={actions} /> : null}
            {tab === "devices" ? <DevicesTab actions={actions} /> : null}
            {tab === "mobile" ? <MobileTab actions={actions} /> : null}
            {tab === "performance" ? <PerformanceTab actions={actions} /> : null}
            {tab === "shortcuts" ? <ShortcutsTab actions={actions} /> : null}
          </TabsContent>
        </Tabs>
      </DialogContent>
    </Dialog>
  );
}

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

// --- General -----------------------------------------------------------------

function GeneralTab({ actions }: { actions: Actions }) {
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

// --- Appearance ----------------------------------------------------------------

/**
 * Sleep idle agents (PRD agent-sleep B1-B3): one choice, and how many of this
 * machine's agents sleep now so the effect of the choice is visible.
 */
function PerformanceTab({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const choice = useShellStore((s) => sleepAfterChoice(s.rest?.ui_state?.agent_sleep_after_hours));
  const sleeping = useShellStore((s) => sleepingCount(s.rest?.navigator?.agents));
  const [changedAt, setChangedAt] = useState<number | null>(null);
  const error = useErrorSince(changedAt, ["agent_sleep."]);
  return (
    <Group
      title={t("settings.idleAgents")}
      note={t("settings.idleAgentsDescription")}
      data-settings-group="idle-agents"
    >
      <Row label={t("settings.sleepAfter")} detail={error ? <Note tone="error" data-agent-sleep-error="true">{t("settings.notSaved", { reason: error })}</Note> : null}>
        {sleeping > 0 ? <Value>{t("settings.sleeping", { count: sleeping })}</Value> : null}
        <Select
          value={choice}
          onValueChange={(value) => {
            const next = SLEEP_AFTER_CHOICES.find((row) => row.id === value);
            if (!next || value === choice) return;
            setChangedAt(Date.now());
            actions.setAgentSleepAfter(next.hours);
          }}
        >
          <SelectTrigger aria-label={t("settings.sleepAfter")} data-agent-sleep-after={choice}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {SLEEP_AFTER_CHOICES.map((row) => (
              <SelectItem key={row.id} value={row.id} data-agent-sleep-option={row.id}>
                {sleepAfterLabel(row, t)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </Row>
    </Group>
  );
}

function AppearanceTab({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const accent = useShellStore((s) => usableAccent(s.rest?.ui_state?.accent_hex));
  const theme = useShellStore((s) => readTheme(s.rest?.ui_state?.theme).choice);
  const fontSize = useShellStore((s) => usableFontSize(s.rest?.ui_state?.font_size)) ?? FONT_SIZE_BASE;
  const textLarger = useShellStore((s) => commandLabel("text_larger", s.rest?.ui_state));
  const textSmaller = useShellStore((s) => commandLabel("text_smaller", s.rest?.ui_state));
  const [changedAt, setChangedAt] = useState<number | null>(null);
  const error = useErrorSince(changedAt, ["ui_state."]);
  const [draftSize, setDraftSize] = useState(fontSize);
  useEffect(() => setDraftSize(fontSize), [fontSize]);
  const chosenAccent = accentNameOf(accent);
  const commitSize = (size: number) => {
    if (size === fontSize) return;
    setChangedAt(Date.now());
    actions.setFontSize(size);
  };
  return (
    <>
      <Group title={t("settings.theme")} note={t("settings.themeDescription")}>
        <Row label={t("settings.tabs.appearance")}>
          <ToggleGroup
            type="single"
            value={theme}
            aria-label={t("settings.theme")}
            data-theme-choice={theme}
            onValueChange={(value) => {
              // A second press on the chosen item would clear it; a theme is always chosen.
              if (!value || value === theme) return;
              setChangedAt(Date.now());
              actions.setTheme(value as ThemeChoice);
            }}
          >
            {THEME_CHOICES.map((choice) => (
              <ToggleGroupItem key={choice.id} value={choice.id} aria-label={t(`settings.theme.${choice.id}`)} data-theme-option={choice.id}>
                {t(`settings.theme.${choice.id}`)}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </Row>
        <Row label={t("settings.accent")} detail={error ? <Note tone="error" data-appearance-error="true">{t("settings.notSaved", { reason: error })}</Note> : null}>
          <RadioGroup
            aria-label={t("settings.accent")}
            value={chosenAccent ?? ""}
            className="flex items-center gap-sm"
            onValueChange={(name) => {
              const hex = ACCENT_STORED_HEX[name as AccentName];
              setChangedAt(Date.now());
              actions.setAccent(hex.toUpperCase());
            }}
          >
            {ACCENT_CHOICES.map((choice) => (
              <Hint key={choice.id} label={t("settings.accentName", { name: t(`settings.accent.${choice.id}`) })}>
                <RadioGroupItem
                  value={choice.id}
                  data-accent={choice.id}
                  className={`border-0 ${choice.swatch} ring-1 ring-border data-[state=checked]:ring-2 data-[state=checked]:ring-foreground [&_svg]:hidden`}
                />
              </Hint>
            ))}
          </RadioGroup>
          <Value>{accent ? accent.toUpperCase() : t("settings.defaultChoice")}</Value>
        </Row>
      </Group>
      <Group
        title={t("settings.density")}
        note={t("settings.densityDescription", { larger: textLarger, smaller: textSmaller })}
      >
        <Row label={t("settings.interfaceFont")}>
          <Slider
            min={FONT_SIZE_MIN}
            max={FONT_SIZE_MAX}
            step={1}
            value={[draftSize]}
            aria-label={t("settings.interfaceFontSize")}
            data-font-size="true"
            className="w-(--size-settings-control-w)"
            onValueChange={([size]) => size !== undefined && setDraftSize(size)}
            onValueCommit={([size]) => size !== undefined && commitSize(size)}
          />
          <Value>{t("settings.fontPoints", { size: draftSize })}</Value>
        </Row>
      </Group>
    </>
  );
}

// --- Agents --------------------------------------------------------------------

function AgentsTab({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const ai = useShellStore((s) => s.rest?.status?.background_ai);
  const hooks = useShellStore((s) => s.rest?.status?.agent_hooks);
  const devices = useShellStore((s) => s.rest?.navigator?.devices);
  const [changedAt, setChangedAt] = useState<number | null>(null);
  const aiError = useErrorSince(changedAt, ["ai_settings."]);
  const [pressedAt, setPressedAt] = useState<number | null>(null);
  const kitError = useErrorSince(pressedAt, ["kit."]);

  // The provider probe and the hook diagnosis run only while a page shows
  // this tab (B8). A hidden browser tab is not looking either; the daemon
  // releases this page's demand if the socket drops.
  // A reconnect is a new connection whose demand starts empty, so the
  // demand is declared again each time the page is live.
  const live = useShellStore((s) => s.connection === "live");
  useEffect(() => {
    if (!live) return;
    const report = () => actions.observeAgents(document.visibilityState === "visible");
    report();
    actions.checkKit();
    document.addEventListener("visibilitychange", report);
    return () => {
      document.removeEventListener("visibilitychange", report);
      actions.observeAgents(false);
    };
  }, [actions, live]);

  const selected = ai?.providers.find((provider) => provider.id === ai.provider) ?? null;
  const machines = kitAgentMachines(devices ?? []);

  return (
    <>
      <Group title={t("settings.agentClis")} note={t("settings.agentClisDescription")}>
        {(ai?.providers ?? []).length === 0 ? <Row label={<Note>{t("settings.notRead")}</Note>} /> : null}
        {(ai?.providers ?? []).map((provider) => {
          const line = providerLine(provider);
          return (
            <Row key={provider.id} label={<span className="font-semibold">{provider.label}</span>}>
              <Status tone={line.tone} data-cli-state={`${provider.id}:${provider.state}`}>
                {line.text}
              </Status>
            </Row>
          );
        })}
      </Group>
      <Group
        title={t("settings.backgroundAi")}
        note={ai?.unavailable_reason ?? t("settings.backgroundAiDescription")}
      >
        <Row label={t("common.agent")} detail={aiError ? <Note tone="error" data-ai-error="true">{t("settings.notSaved", { reason: aiError })}</Note> : null}>
          <Select
            value={ai?.provider ?? undefined}
            disabled={!ai || ai.providers.length === 0}
            onValueChange={(value) => {
              setChangedAt(Date.now());
              actions.chooseAi(value);
            }}
          >
            <SelectTrigger aria-label={t("settings.backgroundAgent")} data-ai-provider="true">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {(ai?.providers ?? []).map((provider) => (
                <SelectItem key={provider.id} value={provider.id}>
                  {provider.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Status tone="muted">{ai?.chosen ? t("settings.chosen") : t("settings.defaultChoice")}</Status>
        </Row>
        <Row
          label={t("common.model")}
          detail={selected?.models_unavailable_reason ? <Note>{t("settings.modelsUnavailable", { reason: selected.models_unavailable_reason })}</Note> : null}
        >
          <Select
            value={selected?.model ?? undefined}
            disabled={!selected || offeredModels(selected).length < 2}
            onValueChange={(value) => {
              if (!selected) return;
              setChangedAt(Date.now());
              actions.chooseAi(selected.id, value);
            }}
          >
            <SelectTrigger aria-label={t("settings.backgroundModel")} data-ai-model="true">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {(selected ? offeredModels(selected) : []).map((model) => (
                <SelectItem key={model} value={model}>
                  {model}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Row>
        <Row label={t("settings.agentSummary")} detail={<Note>{t("settings.agentSummaryDescription")}</Note>}>
          <Switch
            checked={ai?.agent_summary ?? true}
            disabled={!ai}
            onCheckedChange={(checked) => {
              setChangedAt(Date.now());
              actions.setAgentSummary(checked);
            }}
            aria-label={t("settings.agentSummary")}
            data-ai-agent-summary={String(ai?.agent_summary ?? true)}
          />
        </Row>
        {selected && selected.state !== "ready" && selected.state !== "unread" ? (
          <Row label={<Note tone="warn" data-ai-degraded="true">{t("settings.backgroundDegraded", { agent: selected.label, status: selected.headline || selected.state })}</Note>} />
        ) : null}
      </Group>
      <Group
        title={t("settings.agentHooks")}
        note={t("settings.agentHooksDescription")}
        data-agent-hooks="true"
      >
        {machines.map(({ device, setUp, others, unavailable }) => (
          <div key={device.id} data-hook-machine={device.id}>
            <Row
              label={<span className="font-semibold">{device.id === "local" ? t("common.thisMac") : device.label}</span>}
              detail={unavailable ? <Note data-hook-unavailable={device.id}>{unavailable}</Note> : device.kit?.components.length === 0 ? <Note>{t("settings.notChecked")}</Note> : null}
            />
            {setUp.map((agent) => {
              const line = kitAgentLine(agent, t);
              const switched = kitAgentSwitch(agent);
              return (
                <Row
                  key={agent.id}
                  label={
                    <span className="flex min-w-0 flex-col pl-md">
                      <span>{agent.label}</span>
                      <span className="text-caption text-muted-foreground">{kitAgentGets(agent, t)}</span>
                    </span>
                  }
                  detail={line.reason ? <Note tone={line.tone === "error" ? "error" : "muted"}>{line.reason}</Note> : null}
                >
                  <Status tone={line.tone} data-agent-state={`${device.id}:${agent.id}:${agent.enabled ? "on" : "off"}:${line.tone}`}>
                    {line.text}
                  </Status>
                  {kitAgentNeedsReinstall(agent) ? (
                    <Button
                      variant="secondary"
                      disabled={device.kit?.busy === true}
                      onClick={() => {
                        setPressedAt(Date.now());
                        actions.reinstallKit(device.id, [], [agent.id]);
                      }}
                      data-hook-reinstall={`${device.id}:${agent.id}`}
                    >
                      {device.kit?.busy ? t("settings.reinstalling") : t("settings.reinstall")}
                    </Button>
                  ) : null}
                  {switched ? (
                    <Switch
                      checked={switched.on}
                      disabled={device.kit?.busy === true}
                      onCheckedChange={(checked) => {
                        setPressedAt(Date.now());
                        actions.setKitAgent(device.id, agent.id, checked);
                      }}
                      aria-label={t(switched.on ? "devices.kitSwitchOff" : "devices.kitSwitchOn", { part: agent.label })}
                      data-agent-switch={`${device.id}:${agent.id}:${switched.on ? "on" : "off"}`}
                    />
                  ) : null}
                </Row>
              );
            })}
            {others.length > 0 ? (
              <Row label={<Note data-agents-not-set-up={device.id}>{t("settings.agentsNotSetUp", { agents: others.join(", ") })}</Note>} />
            ) : null}
          </div>
        ))}
        {kitError ? <Row label={<Note tone="error" data-hook-error="true">{kitError}</Note>} /> : null}
        {hooks?.last_report_failure ? <Row label={<Note tone="error">{hooks.last_report_failure}</Note>} /> : null}
        {(hooks?.sessions_predating_install ?? []).map((pane) => (
          <Row key={pane.pane_id} label={<Note tone="warn">{`${pane.label} (${pane.pane_id}): ${pane.message}`}</Note>} />
        ))}
      </Group>
    </>
  );
}

// --- Issues --------------------------------------------------------------------

/**
 * Where each local project's issues live and how work starts from one. The
 * core resolves every source; this tab names what it resolved and sends the
 * operator's choice back as one event.
 */
function IssuesTab({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const workspaces = useShellStore((s) => s.rest?.navigator?.workspaces);
  const stored = useShellStore((s) => s.rest?.ui_state?.project_issue_sources);
  const settings = useShellStore((s) => s.rest?.ui_state?.issue_settings);
  const [sourceAt, setSourceAt] = useState<number | null>(null);
  const sourceError = useErrorSince(sourceAt, ["issue_source."]);
  const [settingsAt, setSettingsAt] = useState<number | null>(null);
  const settingsError = useErrorSince(settingsAt, ["issue_settings."]);
  // The projects the boards draw issues for: this Mac's, each with its source;
  // a device's projects keep their issues on that device, and the Home is no project.
  const projects = (workspaces ?? []).filter((workspace) => !workspace.remote_target_id && !workspace.is_home && workspace.tasks?.source != null);
  const access = githubAccess(projects);
  const accessLine = access ? githubAccessLine(access, t) : null;
  const change = (patch: Partial<IssueSettings>) => {
    setSettingsAt(Date.now());
    actions.setIssueSettings(patch);
  };
  return (
    <div data-settings-issues="true">
      <Group title={t("issueSettings.sources")} data-settings-group="issue-sources">
        <Row
          label="GitHub"
          detail={access?.state === "failed" && access.reason ? <Note tone="warn" data-issue-github-reason="true">{access.reason}</Note> : null}
        >
          <span className="text-body text-muted-foreground">{t("issueSettings.githubAccess")}</span>
          {accessLine ? (
            <Status tone={accessLine.tone} data-issue-source-github={access?.state === "failed" ? access.category : "connected"}>
              {accessLine.text}
            </Status>
          ) : null}
        </Row>
        <Row label={t("issueSettings.local")}>
          <span className="text-body text-muted-foreground">{t("issueSettings.localDescription")}</span>
        </Row>
      </Group>
      <Group
        title={t("issueSettings.projects")}
        note={t("issueSettings.projectsDescription")}
        data-settings-group="project-issue-sources"
      >
        {projects.length === 0 ? <Row label={<Note>{t("issueSettings.noProjects")}</Note>} /> : null}
        {projects.map((workspace) => {
          const { value, options } = issueSourceChoices(workspace, stored?.[workspace.path], t);
          const failure = workspace.tasks?.source?.failure ?? null;
          return (
            <Row key={workspace.id} label={<span className="break-words">{workspace.label}</span>}>
              {failure ? (
                <Hint label={failure}>
                  <span className="text-warning" tabIndex={0} data-issue-source-failure={workspace.path}>
                    <TriangleAlertIcon aria-hidden="true" className="size-(--size-icon)" />
                  </span>
                </Hint>
              ) : null}
              <Select
                value={value}
                onValueChange={(next) => {
                  if (next === value) return;
                  setSourceAt(Date.now());
                  actions.setIssueSource(workspace.path, next as IssueSourceChoice);
                }}
              >
                <SelectTrigger
                  aria-label={t("issueSettings.projectSource", { name: workspace.label })}
                  className="w-auto min-w-(--size-settings-control-w)"
                  data-issue-source-project={workspace.path}
                  data-issue-source={value}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {options.map((option) => (
                    <SelectItem key={option.id} value={option.id} data-issue-source-option={option.id}>
                      {option.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Row>
          );
        })}
        {sourceError ? <Row label={<Note tone="error" data-issue-source-error="true">{t("settings.notSaved", { reason: sourceError })}</Note>} /> : null}
      </Group>
      <Group title={t("issueSettings.startWork")} data-settings-group="issue-start">
        {settings ? (
          <>
            <Row label={t("issueSettings.aiNames")}>
              <Switch
                checked={settings.ai_worktree_name}
                onCheckedChange={(checked) => change({ ai_worktree_name: checked })}
                aria-label={t("issueSettings.aiNames")}
                data-issue-ai-worktree-name={String(settings.ai_worktree_name)}
              />
            </Row>
            <Row label={t("issueSettings.closesInstruction")}>
              <Switch
                checked={settings.closes_instruction}
                onCheckedChange={(checked) => change({ closes_instruction: checked })}
                aria-label={t("issueSettings.closesInstruction")}
                data-issue-closes-instruction={String(settings.closes_instruction)}
              />
            </Row>
          </>
        ) : (
          <Row label={<Note>{t("settings.notRead")}</Note>} />
        )}
        {settingsError ? <Row label={<Note tone="error" data-issue-settings-error="true">{t("settings.notSaved", { reason: settingsError })}</Note>} /> : null}
      </Group>
    </div>
  );
}

// --- Devices -------------------------------------------------------------------

function DevicesTab({ actions }: { actions: Actions }) {
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

function KitTerms({ helperRoot, cliDir }: { helperRoot: string | null; cliDir: string | null }) {
  const { t } = useInterfaceTranslation();
  return (
    <ul className="list-disc space-y-xs pl-md text-body text-subtle-foreground" data-kit-terms="true">
      {kitConsentTerms(helperRoot, cliDir, t).map((term) => (
        <li key={term}>{term}</li>
      ))}
    </ul>
  );
}

/**
 * Each part of Hide's kit on one machine, in the same form for This Mac and
 * every device (PRD device-parity B7): a mark, the part, and where it is or
 * why it is not. A machine whose kit does not run says why instead.
 */
function MachineKit({ device, actions }: { device: Device; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const kit = device.kit;
  if (!kit) return null;
  if (kit.unavailable) {
    return (
      <div className="mt-xs" data-machine-kit={`${device.id}:unavailable`}>
        <Note>{kit.unavailable}</Note>
      </div>
    );
  }
  if (kit.components.length === 0) {
    return (
      <div className="mt-xs" data-machine-kit={`${device.id}:${kit.busy ? "busy" : "unread"}`}>
        <Status tone="pending">{kit.busy ? t("devices.installingKit") : t("devices.kitOnConnection")}</Status>
      </div>
    );
  }
  return (
    <div className="mt-xs grid grid-cols-[auto_auto_minmax(0,1fr)_auto] gap-x-xs gap-y-xxs" data-machine-kit={`${device.id}:${kit.busy ? "busy" : "read"}`}>
      {kit.components.map((part) => {
        const line = kitPartLine(part, t);
        const switched = kitPartSwitch(part);
        const mark = part.state === "installed" ? "✓" : part.state === "absent" ? "–" : part.state === "off" ? "○" : part.state === "failed" ? "✕" : "!";
        const markTone = line.tone === "ok" ? "text-success" : line.tone === "muted" ? "text-muted-foreground" : line.tone === "error" ? "text-destructive" : "text-warning";
        return (
          <div key={part.id} className="col-span-4 grid grid-cols-subgrid text-caption" data-kit-part={`${device.id}:${part.id}:${part.state}`}>
            <span className={markTone} aria-hidden="true">
              {mark}
            </span>
            <span className="whitespace-nowrap text-foreground">{part.label}</span>
            {part.state === "installed" ? <span className="sr-only">{line.text}</span> : null}
            <span className="min-w-0 break-words text-subtle-foreground">
              {part.state === "installed" ? <span className="break-all font-mono">{part.location}</span> : `${line.text}${part.reason ? `: ${part.reason}` : ""}`}
              {/* An installed part can still carry a reason, such as a setting that applies to newly opened sessions. */}
              {part.state === "installed" && part.reason ? <span className="block text-muted-foreground" data-kit-part-note="">{part.reason}</span> : null}
            </span>
            {switched ? (
              <Switch
                checked={switched.on}
                disabled={kit.busy}
                onCheckedChange={(checked) => actions.setKitComponent(device.id, part.id, checked)}
                aria-label={t(switched.on ? "devices.kitSwitchOff" : "devices.kitSwitchOn", { part: part.label })}
                data-kit-part-switch={`${device.id}:${part.id}:${switched.on ? "on" : "off"}`}
              />
            ) : (
              <span aria-hidden="true" />
            )}
          </div>
        );
      })}
      {kit.agents
        .filter((agent) => kitAgentSwitch(agent) !== null)
        .map((agent) => {
          const line = kitAgentLine(agent, t);
          const switched = kitAgentSwitch(agent);
          const mark = !agent.enabled ? "○" : line.tone === "ok" ? "✓" : line.tone === "error" ? "✕" : "!";
          const markTone = line.tone === "ok" ? "text-success" : line.tone === "muted" ? "text-muted-foreground" : line.tone === "error" ? "text-destructive" : "text-warning";
          return (
            <div key={agent.id} className="col-span-4 grid grid-cols-subgrid text-caption" data-kit-agent={`${device.id}:${agent.id}:${agent.enabled ? "on" : "off"}`}>
              <span className={markTone} aria-hidden="true">
                {mark}
              </span>
              <span className="whitespace-nowrap text-foreground">{agent.label}</span>
              <span className="min-w-0 break-words text-subtle-foreground">{`${line.text}${line.reason ? `: ${line.reason}` : ""}`}</span>
              {switched ? (
                <Switch
                  checked={switched.on}
                  disabled={kit.busy}
                  onCheckedChange={(checked) => actions.setKitAgent(device.id, agent.id, checked)}
                  aria-label={t(switched.on ? "devices.kitSwitchOff" : "devices.kitSwitchOn", { part: agent.label })}
                  data-kit-agent-switch={`${device.id}:${agent.id}:${switched.on ? "on" : "off"}`}
                />
              ) : (
                <span aria-hidden="true" />
              )}
            </div>
          );
        })}
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

function AddDevice({ actions, devices, helperRoot, cliDir }: { actions: Actions; devices: Device[]; helperRoot: string | null; cliDir: string | null }) {
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

// --- Shortcuts -----------------------------------------------------------------

function ShortcutsTab({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  // Each host edits its own set: the desktop app the macOS set, the browser
  // its own (user decision 2026-09-26).
  const host = hostKind();
  const system = keySystem();
  const stored = useShellStore((s) => storedBindings(s.rest?.ui_state, host));
  const { registry, diagnostic } = resolvedRegistry(stored, host, system);
  const [sentAt, setSentAt] = useState<number | null>(null);
  const [sent, setSent] = useState<Record<string, string> | null>(null);
  const saveError = useErrorSince(sentAt, ["ui_state."]);
  const saving = sent !== null && JSON.stringify(sent) !== JSON.stringify(stored ?? {}) && saveError === null;
  const apply = (bindings: Record<string, string>) => {
    setSent(bindings);
    setSentAt(Date.now());
    actions.setPaneShortcuts(host, bindings);
  };
  const current = (): Record<string, string> => Object.fromEntries(Object.entries(diagnostic && !diagnostic.includes("retired and was ignored") ? {} : stored ?? {}).filter(([key]) => !["project_home", "toggle_sidebar_view"].includes(key)));
  const rowsFor = (ids: readonly CommandId[]) =>
    ids.map((id) => (
      <ShortcutRow
        key={id}
        id={id}
        host={host}
        system={system}
        registry={registry}
        overridden={!diagnostic && stored?.[storedKey(id, host)] !== undefined}
        onApply={(chord) => {
          const next = current();
          const fallback = defaultChord(id, host, system);
          const text = serializeStoredChord(chord, host, system);
          if ((fallback && chordEquals(fallback, chord)) || text === null) delete next[storedKey(id, host)];
          else next[storedKey(id, host)] = text;
          apply(next);
        }}
        onClear={() => {
          const next = current();
          next[storedKey(id, host)] = "none";
          apply(next);
        }}
        onReset={() => {
          const next = current();
          delete next[storedKey(id, host)];
          apply(next);
        }}
      />
    ));
  return (
    <>
      <Group
        title={t("settings.shortcuts.paneAndNavigation")}
        note={t(host === "electron" ? (system === "mac" ? "settings.shortcuts.desktopMac" : "settings.shortcuts.desktopPc") : system === "mac" ? "settings.shortcuts.browserMac" : "settings.shortcuts.browserPc")}
      >
        {rowsFor(EDITABLE_PANE_COMMANDS.filter((id) => !(AREA_COMMANDS as readonly CommandId[]).includes(id)))}
        <Row label={<span className="text-subtle-foreground">{t("settings.shortcuts.toggleConversation")}</span>}>
          <Status tone="muted">{t("settings.shortcuts.conversationUnavailable")}</Status>
        </Row>
      </Group>
      <Group
        title={t("settings.shortcuts.areaTitle")}
        note={t("settings.shortcuts.areaDescription")}
        data-settings-group="area-commands"
      >
        {rowsFor(AREA_COMMANDS)}
      </Group>
      <Group
        title={t("settings.shortcuts.numberedTitle")}
        note={
          host === "electron"
            ? t("settings.shortcuts.numberedDesktop", { modifiers: t(system === "mac" ? "settings.shortcuts.numberedModifiersMac" : "settings.shortcuts.numberedModifiersPc") })
            : t(system === "mac" ? "settings.shortcuts.numberedBrowserMac" : "settings.shortcuts.numberedBrowserPc")
        }
        data-settings-group="numbered-chords"
      >
        {sheetRows("Tabs", registry, host, system)
          .concat(sheetRows("Navigate", registry, host, system))
          .filter((row) => row.id.startsWith("select_"))
          .map((row) => (
            <Row key={row.id} label={sheetRowTitle(row, t)}>
              <Kbd data-shortcut-effective={row.id}>{row.chord ?? "-"}</Kbd>
              {row.chord === null ? <Status tone="muted">{t("settings.shortcuts.notOnHost")}</Status> : null}
            </Row>
          ))}
      </Group>
      {diagnostic ? <Note tone="warn" data-shortcut-diagnostic="true">{diagnostic}</Note> : null}
      {saving ? <Note tone="pending">{t("workspace.saving")}</Note> : null}
      {saveError ? <Note tone="error" data-shortcut-save-error="true">{t("settings.notSaved", { reason: saveError })}</Note> : null}
      <div className="mt-sm flex justify-end">
        <Button variant="secondary" disabled={!stored || Object.keys(stored).length === 0} onClick={() => apply({})} data-shortcut-reset-all="true">
          {t("settings.shortcuts.restoreDefaults")}
        </Button>
      </div>
    </>
  );
}

function ShortcutRow({
  id,
  host,
  system,
  registry,
  overridden,
  onApply,
  onReset,
  onClear,
}: {
  id: CommandId;
  host: HostKind;
  system: KeySystem;
  registry: ReturnType<typeof resolvedRegistry>["registry"];
  overridden: boolean;
  onApply: (chord: Chord) => void;
  onReset: () => void;
  onClear: () => void;
}) {
  const { t } = useInterfaceTranslation();
  const command = registry.find((row) => row.id === id);
  const [recording, setRecording] = useState(false);
  const [draft, setDraft] = useState<Chord | null>(null);
  const [problem, setProblem] = useState<BindingProblem | "altgr" | null>(null);
  const setRecordingFlag = useUiStore((s) => s.setRecordingShortcut);
  useEffect(() => {
    if (!recording) return;
    setRecordingFlag(true);
    return () => setRecordingFlag(false);
  }, [recording, setRecordingFlag]);
  if (!command) return null;
  const title = commandTitle(id, t);
  const effective = hostChord(command, host);
  const record = (event: KeyboardEvent<HTMLButtonElement>) => {
    // IME composition and lone modifiers are not chords; the recorder waits.
    if (event.nativeEvent.isComposing || event.keyCode === 229) return;
    if (["Meta", "Alt", "Shift", "Control", "CapsLock"].includes(event.key)) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "Escape" && !event.metaKey && !event.altKey && !event.ctrlKey) {
      setRecording(false);
      setProblem(null);
      return;
    }
    const chord = chordFromEvent(event.nativeEvent);
    // AltGr types a character on Windows and Linux layouts (Windows reports it
    // as Ctrl+Alt), so a key pressed with it is never a chord, as the window
    // listener already treats it.
    const reason: BindingProblem | "altgr" | null =
      system === "pc" && event.nativeEvent.getModifierState?.("AltGraph") ? "altgr" : bindingProblem(id, chord, registry, host, system);
    setRecording(false);
    setProblem(reason);
    setDraft(reason ? null : chord);
  };
  return (
    <Row
      label={title}
      detail={
        problem ? (
          <Note tone="error" data-shortcut-problem={id}>
            {problem === "altgr" ? t("settings.shortcuts.altGr") : bindingProblemText(problem, t)} {effective ? t("settings.shortcuts.previousChord", { chord: displayChord(effective, system) }) : ""}
          </Note>
        ) : null
      }
    >
      <Kbd data-shortcut-effective={id}>{effective ? displayChord(effective, system) : "-"}</Kbd>
      {draft ? (
        <>
          <span className="text-body text-subtle-foreground">→</span>
          <Kbd className="text-foreground" data-shortcut-draft={id}>
            {displayChord(draft, system)}
          </Kbd>
          <Button
            onClick={() => {
              onApply(draft);
              setDraft(null);
            }}
            data-shortcut-apply={id}
          >
            {t("common.apply")}
          </Button>
          <Button variant="ghost" onClick={() => setDraft(null)}>
            {t("common.cancel")}
          </Button>
        </>
      ) : (
        <Button
          variant={recording ? "default" : "secondary"}
          aria-label={recording ? t("settings.shortcuts.recordAria", { command: title }) : t("settings.shortcuts.changeAria", { command: title })}
          onKeyDown={recording ? record : undefined}
          onBlur={() => setRecording(false)}
          onClick={() => {
            setProblem(null);
            setRecording(true);
          }}
          data-shortcut-record={id}
        >
          {recording ? t("settings.shortcuts.pressChord") : t("settings.shortcuts.change")}
        </Button>
      )}
      {effective && !draft && !recording ? <Button variant="ghost" onClick={onClear} data-shortcut-clear={id}>{t("settings.shortcuts.clear")}</Button> : null}
      {overridden && !draft ? (
        <Button variant="ghost" onClick={onReset} data-shortcut-reset={id}>
          {t("common.default")}
        </Button>
      ) : null}
    </Row>
  );
}
