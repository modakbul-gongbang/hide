// The web Settings sheet (PRD S5 B1-B10, B18-B22): the native sheet's sections
// less Pet, each row reading a value the core or the daemon reported and each
// edit sent as the one core event that owns it. Nothing here decides a value;
// a pending edit shows as pending until the snapshot says it landed.

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
  sleepAfterChoice,
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
  kitHookMachines,
  kitPartLine,
  kitPartNeedsReinstall,
  offeredModels,
  ownerLine,
  providerLine,
  environmentTone,
  shown,
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
  type Chord,
  type CommandId,
  type KeySystem,
  sheetRows,
} from "./shortcuts";
import { commandLabel } from "./shortcutLabels";
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
  const close = () => useUiStore.getState().closeOverlay("settings");
  const [tab, setTab] = useState<SettingsTab>(() => useUiStore.getState().settingsTab);
  const subtitle = SETTINGS_TABS.find((row) => row.id === tab)?.subtitle ?? "";
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
            <DialogTitle className="text-headline">Settings</DialogTitle>
            <DialogDescription>{subtitle}</DialogDescription>
            <p className="mt-xxs text-caption text-muted-foreground" data-settings-owner={daemon?.host_name ?? "unknown"}>
              {ownerLine(daemon, selected)}
            </p>
          </div>
          <Hint label="Close Settings" shortcut="Esc">
            <Button variant="ghost" size="icon-sm" aria-label="Close Settings" onClick={close} data-settings-close="true">
              <XIcon />
            </Button>
          </Hint>
        </header>
        {/* Radix Tabs owns the roving tabindex and Left/Right (plus Home/End)
            arrow-key navigation the hand-built tablist used to implement. */}
        <Tabs value={tab} onValueChange={(value) => setTab(value as SettingsTab)} className="min-h-0 flex-1 flex-col gap-none">
          <TabsList aria-label="Settings section" className="w-full flex-wrap justify-start gap-xs rounded-none border-b border-border bg-sidebar px-xl py-sm">
            {SETTINGS_TABS.map((row) => (
              <TabsTrigger key={row.id} value={row.id} data-settings-tab={row.id}>
                {row.title}
              </TabsTrigger>
            ))}
          </TabsList>
          <TabsContent value={tab} className="min-h-0 flex-1 overflow-auto px-xl py-lg">
            {tab === "general" ? <GeneralTab /> : null}
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

// --- General -----------------------------------------------------------------

function GeneralTab() {
  const daemon = useShellStore((s) => s.daemon);
  const connection = useShellStore((s) => s.connection);
  const herdr = useShellStore((s) => s.rest?.status?.herdr);
  const environment = useShellStore((s) => s.rest?.status?.environment);
  const diagnostics = useShellStore((s) => s.rest?.status?.diagnostics);
  const lastError = useShellStore((s) => s.rest?.status?.last_error);
  const [copy, setCopy] = useState<"idle" | "copied" | "failed">("idle");
  const line = herdrLine(herdr);
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
      <Group title="Daemon" note="This page is a client of the daemon above; its settings and registrations live in that state file.">
        <Row label="Version">
          <Value>{daemon ? `hided ${daemon.version}` : "unavailable"}</Value>
        </Row>
        <Row label="Process">
          <Value>{daemon ? `pid ${daemon.pid} · schema ${daemon.schema_version}` : "unavailable"}</Value>
        </Row>
        <Row label="Lifetime">
          <Value mono={false}>{daemon ? (daemon.keep_alive ? "Kept alive" : `Exits ${Math.round(daemon.idle_secs / 60)} min after the last page closes`) : "unavailable"}</Value>
        </Row>
        <Row label="State file">
          <Value>{shown(daemon?.core_state_path)}</Value>
        </Row>
        <Row label="Page connection">
          <Status tone={connection === "live" ? "ok" : "warn"} data-page-connection={connection}>
            {connection}
          </Status>
        </Row>
      </Group>
      <Group title="Herdr runtime">
        <Row
          label="Connection"
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
        <Row label="Version">
          <Value>{shown(herdr?.received_version)}</Value>
        </Row>
        <Row label="Protocol">
          <Value>{herdr?.received_protocol != null ? `${herdr.received_protocol} (expects ${shown(herdr.expected_protocol)})` : "unavailable"}</Value>
        </Row>
        <Row label="Socket">
          <Value>{shown(herdr?.socket_path ?? daemon?.herdr_socket_path)}</Value>
        </Row>
        <Row label="Binary">
          <Value>{shown(daemon?.herdr_bin_path)}</Value>
        </Row>
      </Group>
      {environment && environment.length > 0 ? (
        <Group title="Environment">
          {environment.map((row) => (
            <Row key={row.key} label={<span className="font-mono">{row.key}</span>} detail={row.message ? <Note>{row.message}</Note> : null}>
              <Status tone={environmentTone(row.state, row.required)}>{row.state.replace(/_/g, " ")}</Status>
            </Row>
          ))}
        </Group>
      ) : null}
      <Group
        title="Diagnostics"
        note="Copy carries versions, paths, states and the core's recent diagnostics. It never carries the page token, terminal output or anything typed into a pane."
      >
        <Row label="Recent" detail={<DiagnosticList />}>
          <Button variant="secondary" onClick={copyDiagnostics} data-copy-diagnostics="true">
            Copy diagnostics
          </Button>
          {copy === "copied" ? <Status tone="ok">Copied</Status> : null}
          {copy === "failed" ? <Status tone="error">The browser refused the clipboard</Status> : null}
        </Row>
      </Group>
      <Group title="Authentication">
        <Row label={<span className="text-body text-subtle-foreground">Hide delegates authentication to Herdr, SSH and the agent CLIs on the daemon's machine. It has no credential, token or passphrase field.</span>} />
      </Group>
    </>
  );
}

function DiagnosticList() {
  const diagnostics = useShellStore((s) => s.rest?.status?.diagnostics);
  const rows = (diagnostics ?? []).slice(-8).reverse();
  if (rows.length === 0) return <Note>No diagnostics recorded yet.</Note>;
  return (
    <ul className="space-y-xxs" data-diagnostics={rows.length}>
      {rows.map((row) => (
        <li key={`${row.occurred_at}-${row.kind}`} className="break-words font-mono text-caption text-subtle-foreground">
          <span className="text-muted-foreground">{new Date(row.occurred_at).toLocaleTimeString()}</span> {row.kind}: {row.message}
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
  const choice = useShellStore((s) => sleepAfterChoice(s.rest?.ui_state?.agent_sleep_after_hours));
  const sleeping = useShellStore((s) => sleepingCount(s.rest?.navigator?.agents));
  const [changedAt, setChangedAt] = useState<number | null>(null);
  const error = useErrorSince(changedAt, ["agent_sleep."]);
  return (
    <Group
      title="Idle agents"
      note="Working agents, unread results and the tab on screen never sleep. Delegated agents sleep too; opening one resumes its conversation."
      data-settings-group="idle-agents"
    >
      <Row label="Sleep idle agents after" detail={error ? <Note tone="error" data-agent-sleep-error="true">Not saved: {error}</Note> : null}>
        {sleeping > 0 ? <Value>{sleeping} sleeping</Value> : null}
        <Select
          value={choice}
          onValueChange={(value) => {
            const next = SLEEP_AFTER_CHOICES.find((row) => row.id === value);
            if (!next || value === choice) return;
            setChangedAt(Date.now());
            actions.setAgentSleepAfter(next.hours);
          }}
        >
          <SelectTrigger aria-label="Sleep idle agents after" data-agent-sleep-after={choice}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {SLEEP_AFTER_CHOICES.map((row) => (
              <SelectItem key={row.id} value={row.id} data-agent-sleep-option={row.id}>
                {row.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </Row>
    </Group>
  );
}

function AppearanceTab({ actions }: { actions: Actions }) {
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
      <Group title="Theme" note="System follows macOS as it changes. Accent tints primary buttons, focus rings and the editor caret; agent status colors keep their meaning whatever the accent.">
        <Row label="Appearance">
          <ToggleGroup
            type="single"
            value={theme}
            aria-label="Theme"
            data-theme-choice={theme}
            onValueChange={(value) => {
              // A second press on the chosen item would clear it; a theme is always chosen.
              if (!value || value === theme) return;
              setChangedAt(Date.now());
              actions.setTheme(value as ThemeChoice);
            }}
          >
            {THEME_CHOICES.map((choice) => (
              <ToggleGroupItem key={choice.id} value={choice.id} aria-label={choice.label} data-theme-option={choice.id}>
                {choice.label}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </Row>
        <Row label="Accent" detail={error ? <Note tone="error" data-appearance-error="true">Not saved: {error}</Note> : null}>
          <RadioGroup
            aria-label="Accent"
            value={chosenAccent ?? ""}
            className="flex items-center gap-sm"
            onValueChange={(name) => {
              const hex = ACCENT_STORED_HEX[name as AccentName];
              setChangedAt(Date.now());
              actions.setAccent(hex.toUpperCase());
            }}
          >
            {ACCENT_CHOICES.map((choice) => (
              <Hint key={choice.name} label={`Accent ${choice.name}`}>
                <RadioGroupItem
                  value={choice.name.toLowerCase()}
                  data-accent={choice.name.toLowerCase()}
                  className={`border-0 ${choice.swatch} ring-1 ring-border data-[state=checked]:ring-2 data-[state=checked]:ring-foreground [&_svg]:hidden`}
                />
              </Hint>
            ))}
          </RadioGroup>
          <Value>{accent ? accent.toUpperCase() : "default"}</Value>
        </Row>
      </Group>
      <Group
        title="Density"
        note={`Terminal and editor text keep their own size (${textLarger} and ${textSmaller} in a pane or document).`}
      >
        <Row label="Interface font">
          <Slider
            min={FONT_SIZE_MIN}
            max={FONT_SIZE_MAX}
            step={1}
            value={[draftSize]}
            aria-label="Interface font size"
            data-font-size="true"
            className="w-(--size-settings-control-w)"
            onValueChange={([size]) => size !== undefined && setDraftSize(size)}
            onValueCommit={([size]) => size !== undefined && commitSize(size)}
          />
          <Value>{draftSize} pt</Value>
        </Row>
      </Group>
    </>
  );
}

// --- Agents --------------------------------------------------------------------

function AgentsTab({ actions }: { actions: Actions }) {
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
  const machines = kitHookMachines(devices ?? []);

  return (
    <>
      <Group title="Agent CLIs" note="Asked on the daemon's machine while this tab is open: installed, signed in, or why not.">
        {(ai?.providers ?? []).length === 0 ? <Row label={<Note>Not read yet.</Note>} /> : null}
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
        title="Background AI"
        note={
          ai?.unavailable_reason ??
          "Pane labels and other background features use this agent. Hide never asks for an API key and does not turn on Memory from here."
        }
      >
        <Row label="Agent" detail={aiError ? <Note tone="error" data-ai-error="true">Not saved: {aiError}</Note> : null}>
          <Select
            value={ai?.provider ?? undefined}
            disabled={!ai || ai.providers.length === 0}
            onValueChange={(value) => {
              setChangedAt(Date.now());
              actions.chooseAi(value);
            }}
          >
            <SelectTrigger aria-label="Background AI agent" data-ai-provider="true">
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
          <Status tone="muted">{ai?.chosen ? "chosen" : "default"}</Status>
        </Row>
        <Row
          label="Model"
          detail={
            selected?.models_unavailable_reason ? (
              <Note>Models could not be listed ({selected.models_unavailable_reason}); only the configured one is offered.</Note>
            ) : null
          }
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
            <SelectTrigger aria-label="Background AI model" data-ai-model="true">
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
        {selected && selected.state !== "ready" && selected.state !== "unread" ? (
          <Row label={<Note tone="warn" data-ai-degraded="true">{selected.label}: {selected.headline || selected.state}. Background requests go to the other agent until {selected.label} can answer.</Note>} />
        ) : null}
      </Group>
      <Group
        title="Agent hooks"
        note="Hide installs its hook on every machine it runs agents on, This Mac and each device, and never touches another tool's entries. A hook you removed stays removed until you press Reinstall."
        data-agent-hooks="true"
      >
        {machines.map(({ device, parts, unavailable }) => (
          <div key={device.id} data-hook-machine={device.id}>
            <Row
              label={<span className="font-semibold">{device.id === "local" ? "This Mac" : device.label}</span>}
              detail={unavailable ? <Note data-hook-unavailable={device.id}>{unavailable}</Note> : parts.length === 0 ? <Note>Not checked yet.</Note> : null}
            />
            {parts.map((part) => {
              const line = kitPartLine(part);
              return (
                <Row
                  key={part.id}
                  label={
                    <span className="flex min-w-0 flex-col pl-md">
                      <span>{part.label}</span>
                      {part.location ? <span className="break-all font-mono text-caption text-muted-foreground">{part.location}</span> : null}
                    </span>
                  }
                  detail={part.reason && part.state !== "installed" ? <Note tone={line.tone === "error" ? "error" : "muted"}>{part.reason}</Note> : null}
                >
                  <Status tone={line.tone} data-hook-state={`${device.id}:${part.id}:${part.state}`}>
                    {line.text}
                  </Status>
                  {kitPartNeedsReinstall(part) ? (
                    <Button
                      variant="secondary"
                      disabled={device.kit?.busy === true}
                      onClick={() => {
                        setPressedAt(Date.now());
                        actions.reinstallKit(device.id, [part.id]);
                      }}
                      data-hook-reinstall={`${device.id}:${part.id}`}
                    >
                      {device.kit?.busy ? "Reinstalling…" : "Reinstall"}
                    </Button>
                  ) : null}
                </Row>
              );
            })}
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
  const accessLine = access ? githubAccessLine(access) : null;
  const change = (patch: Partial<IssueSettings>) => {
    setSettingsAt(Date.now());
    actions.setIssueSettings(patch);
  };
  return (
    <div data-settings-issues="true">
      <Group title="이슈 출처" data-settings-group="issue-sources">
        <Row
          label="GitHub"
          detail={access?.state === "failed" && access.reason ? <Note tone="warn" data-issue-github-reason="true">{access.reason}</Note> : null}
        >
          <span className="text-body text-muted-foreground">gh로 읽고 씀</span>
          {accessLine ? (
            <Status tone={accessLine.tone} data-issue-source-github={access?.state === "failed" ? access.category : "connected"}>
              {accessLine.text}
            </Status>
          ) : null}
        </Row>
        <Row label="Local">
          <span className="text-body text-muted-foreground">이 Mac에 저장 · 언제나 사용 가능</span>
        </Row>
      </Group>
      <Group
        title="프로젝트별 출처"
        note="한 프로젝트는 출처 하나입니다. 바꿔도 이미 있는 이슈는 옮기지 않고, 연결된 워크트리는 그대로 둡니다."
        data-settings-group="project-issue-sources"
      >
        {projects.length === 0 ? <Row label={<Note>이 Mac의 프로젝트가 없습니다.</Note>} /> : null}
        {projects.map((workspace) => {
          const { value, options } = issueSourceChoices(workspace, stored?.[workspace.path]);
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
                  aria-label={`${workspace.label} 이슈 출처`}
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
        {sourceError ? <Row label={<Note tone="error" data-issue-source-error="true">저장되지 않음: {sourceError}</Note>} /> : null}
      </Group>
      <Group title="작업 시작" data-settings-group="issue-start">
        {settings ? (
          <>
            <Row label="AI가 워크트리 이름 짓기">
              <Switch
                checked={settings.ai_worktree_name}
                onCheckedChange={(checked) => change({ ai_worktree_name: checked })}
                aria-label="AI가 워크트리 이름 짓기"
                data-issue-ai-worktree-name={String(settings.ai_worktree_name)}
              />
            </Row>
            <Row label="PR 본문에 Closes 넣도록 지시">
              <Switch
                checked={settings.closes_instruction}
                onCheckedChange={(checked) => change({ closes_instruction: checked })}
                aria-label="PR 본문에 Closes 넣도록 지시"
                data-issue-closes-instruction={String(settings.closes_instruction)}
              />
            </Row>
          </>
        ) : (
          <Row label={<Note>아직 읽지 못했다.</Note>} />
        )}
        {settingsError ? <Row label={<Note tone="error" data-issue-settings-error="true">저장되지 않음: {settingsError}</Note>} /> : null}
      </Group>
    </div>
  );
}

// --- Devices -------------------------------------------------------------------

function DevicesTab({ actions }: { actions: Actions }) {
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
    ? deviceRemovalLines(removing.id, registrations ?? [], editorTabs ?? [], recoveryDrafts, (tabId) => bufferWarnings.has(tabId) && released(tabId))
    : [];
  const unstored = removing ? unstoredDeviceDrafts(removing.id, editorTabs ?? [], bufferWarnings, released) : [];
  return (
    <>
      <Group title="Devices" note="Hide stores only a label and an SSH alias. Authentication stays in the daemon machine's SSH environment; no password or key is asked for.">
        {rows.map((device) => {
          const status = remote?.find((row) => row.target_id === device.id);
          const line = deviceLine(device, status);
          return (
            <Row
              key={device.id}
              label={
                <span className="flex min-w-0 flex-col">
                  <span className="break-words font-semibold">{device.label}</span>
                  <span className="break-all font-mono text-caption text-muted-foreground">{device.kind === "remote" ? device.ssh_alias : "local, no SSH alias"}</span>
                </span>
              }
              detail={
                <>
                  <DeviceConnection device={device} facts={deviceFacts(device, status)} />
                  {device.kind === "remote" ? <DeviceHelper device={device} /> : null}
                  <MachineKit device={device} />
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
                  {device.kit.busy ? "Reinstalling…" : "Reinstall"}
                </Button>
              ) : null}
              {focused === device.id ? (
                <Status tone="muted">selected</Status>
              ) : (
                <Button variant="ghost" onClick={() => actions.focusDevice(device.id)} data-device-select={device.id}>
                  Select
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
                    {device.test?.state === "running" ? "Testing…" : "Test"}
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
                      Retry
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
                          Retry helper
                        </Button>
                      ) : null}
                      <Button variant="ghost" onClick={() => setRevoking(device)} data-device-host-revoke={device.id}>
                        Revoke helper…
                      </Button>
                    </>
                  ) : (
                    <Button variant="secondary" onClick={() => setAllowing(device)} data-device-host-allow={device.id}>
                      Allow and install…
                    </Button>
                  )}
                  <Button variant="ghost" onClick={() => setRemoving(device)} data-device-remove={device.id}>
                    Remove…
                  </Button>
                </>
              ) : null}
            </Row>
          );
        })}
        {remoteRows.length === 0 ? <Row label={<Note>No SSH device is registered. The daemon's own machine is always available.</Note>} /> : null}
        {deviceError ? <Row label={<Note tone="error" data-device-error="true">{deviceError}</Note>} /> : null}
      </Group>
      <AddDevice actions={actions} devices={rows} helperRoot={localRoot} cliDir={localCliDir} />
      {allowing ? (
        <Dialog open onOpenChange={(next) => { if (!next) setAllowing(null); }}>
          <DialogContent data-device-host-allow-confirm={allowing.id}>
            <DialogHeader>
              <DialogTitle>Install Hide on {allowing.label}?</DialogTitle>
            </DialogHeader>
            <DialogBody className="space-y-sm">
              {allowing.host?.state === "identity_changed" ? (
                <p className="text-body text-warning">
                  {allowing.ssh_alias} now answers as a different SSH identity than the one this consent was given to. Allow only if you expect that change.
                </p>
              ) : null}
              <KitTerms helperRoot={allowing.host?.helper_root ?? localRoot} cliDir={allowing.host?.cli_dir ?? localCliDir} />
            </DialogBody>
            <DialogFooter>
              <Button variant="secondary" onClick={() => setAllowing(null)}>Not now</Button>
              <Button
                data-device-host-allow-go="true"
                onClick={() => {
                  setActedAt(Date.now());
                  actions.setDeviceHostConsent(allowing.id, true);
                  setAllowing(null);
                }}
              >
                Allow and install
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      ) : null}
      {revoking ? (
        <AlertDialog open onOpenChange={(next) => { if (!next) setRevoking(null); }}>
          <AlertDialogContent data-device-host-revoke-confirm={revoking.id}>
            <AlertDialogHeader>
              <AlertDialogTitle>Revoke Hide&apos;s helper on {revoking.label}?</AlertDialogTitle>
              <AlertDialogDescription>
                Hide stops starting new file, Git and worktree work on {revoking.ssh_alias}. A save already sent is read back before its tab says anything; your drafts and the files on the device are not deleted, and neither is Hide&apos;s installed kit.
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>Keep allowed</AlertDialogCancel>
              <AlertDialogAction
                data-device-host-revoke-go="true"
                onClick={() => {
                  setActedAt(Date.now());
                  actions.setDeviceHostConsent(revoking.id, false);
                }}
              >
                Revoke helper
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      ) : null}
      {removing ? (
        <AlertDialog open onOpenChange={(next) => { if (!next && !removalBusy) setRemoving(null); }}>
          <AlertDialogContent data-device-remove-confirm={removing.id}>
            <AlertDialogHeader>
              <AlertDialogTitle>Remove {removing.label}?</AlertDialogTitle>
              <AlertDialogDescription>
                Hide forgets this device's registration and closes its connection here. Files, the Herdr server and any agents running on {removing.ssh_alias} keep running untouched.
              </AlertDialogDescription>
            </AlertDialogHeader>
            <p className="text-body text-subtle-foreground" data-device-remove-kit={removing.id}>
              {kitRemovalLine(removing)}
            </p>
            {removalLines.map((line) => (
              <p key={line} className="text-body text-subtle-foreground" data-device-remove-effect="true">
                {line}
              </p>
            ))}
            {unstored.length > 0 ? (
              <Note tone="warn" data-device-remove-unstored="true">
                {unstored.length === 1 ? "This draft is" : "These drafts are"} not stored in this browser, so removing the device would lose{" "}
                {unstored.length === 1 ? "it" : "them"}. Export or save {unstored.length === 1 ? "it" : "each"} first: {unstored.join(", ")}
              </Note>
            ) : null}
            <AlertDialogFooter>
              {/* A removal already storing drafts goes out when they land, so it
                  cannot be kept from here. Plain Button, not AlertDialogAction:
                  it has to stay open through the async removal. */}
              <Button variant="secondary" disabled={removalBusy} onClick={() => setRemoving(null)}>Keep device</Button>
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
                {removalBusy ? "Storing drafts…" : `Remove ${removing.label}`}
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
  if (device.kind !== "remote") return null;
  const problem = device.state !== "ready" ? deviceProblemLine(device.problem, device.ssh_alias) : null;
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
  const host = device.host;
  const line = hostLine(host);
  return (
    <div className="mt-xs space-y-xxs" data-device-host={`${device.id}:${host?.state ?? "unknown"}`}>
      <Status tone={line.tone}>{line.text}</Status>
      {host?.consent === "granted" && host.helper_root ? <p className="break-all font-mono text-caption text-muted-foreground">installs to {host.helper_root}</p> : null}
      {host?.bound_identity ? <p className="break-all font-mono text-caption text-muted-foreground">bound to {host.bound_identity}</p> : null}
      {host && host.state !== "ready" && host.message ? <Note tone={line.tone === "muted" ? "muted" : "warn"}>{host.message}</Note> : null}
    </div>
  );
}

function KitTerms({ helperRoot, cliDir }: { helperRoot: string | null; cliDir: string | null }) {
  return (
    <ul className="list-disc space-y-xs pl-md text-body text-subtle-foreground" data-kit-terms="true">
      {kitConsentTerms(helperRoot, cliDir).map((term) => (
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
function MachineKit({ device }: { device: Device }) {
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
        <Status tone="pending">{kit.busy ? "Installing Hide's kit…" : "Hide's kit is checked when the device connects"}</Status>
      </div>
    );
  }
  return (
    <div className="mt-xs grid grid-cols-[auto_auto_minmax(0,1fr)] gap-x-xs gap-y-xxs" data-machine-kit={`${device.id}:${kit.busy ? "busy" : "read"}`}>
      {kit.components.map((part) => {
        const line = kitPartLine(part);
        const mark = part.state === "installed" ? "✓" : part.state === "absent" ? "–" : part.state === "failed" ? "✕" : "!";
        const markTone = line.tone === "ok" ? "text-success" : line.tone === "muted" ? "text-muted-foreground" : line.tone === "error" ? "text-destructive" : "text-warning";
        return (
          <div key={part.id} className="col-span-3 grid grid-cols-subgrid text-caption" data-kit-part={`${device.id}:${part.id}:${part.state}`}>
            <span className={markTone} aria-hidden="true">
              {mark}
            </span>
            <span className="whitespace-nowrap text-foreground">{part.label}</span>
            {part.state === "installed" ? <span className="sr-only">{line.text}</span> : null}
            <span className="min-w-0 break-words text-subtle-foreground">
              {part.state === "installed" ? <span className="break-all font-mono">{part.location}</span> : `${line.text}${part.reason ? `: ${part.reason}` : ""}`}
              {/* An installed part can still carry a reason, such as another program's `hcoord` on PATH (B14). */}
              {part.state === "installed" && part.reason ? <span className="block text-muted-foreground" data-kit-part-note="">{part.reason}</span> : null}
            </span>
          </div>
        );
      })}
    </div>
  );
}

function DeviceTest({ test }: { test: NonNullable<Device["test"]> }) {
  const tone = test.state === "running" ? "pending" : test.state === "passed" ? "ok" : "warn";
  // A finished test names when it ran: it is that attempt's result, and it
  // stays beside the row after the connection itself has changed.
  const at = test.checked_at_unix_ms === null ? "" : ` at ${new Date(test.checked_at_unix_ms).toLocaleTimeString()}`;
  const headline = test.state === "running" ? "Testing the connection…" : `Connection test ${test.state === "passed" ? "passed" : "failed"}${at}`;
  return (
    <div className="mt-xs space-y-xxs" data-device-test-state={test.state}>
      <Status tone={tone}>{headline}</Status>
      {test.stages.map((stage) => (
        <div key={stage.stage} className="flex gap-xs text-caption">
          <span className={stage.state === "passed" ? "text-success" : stage.state === "pending" ? "text-muted-foreground" : "text-warning"} aria-hidden="true">
            {stage.state === "passed" ? "✓" : stage.state === "pending" ? "…" : "✕"}
          </span>
          <span className="sr-only">{stage.state === "passed" ? "passed" : stage.state === "pending" ? "not run" : "failed"}</span>
          <span className="w-[var(--size-device-test-stage-col)] shrink-0 font-mono text-foreground">{stage.stage}</span>
          <span className="min-w-0 break-words text-subtle-foreground">{stage.detail}</span>
        </div>
      ))}
    </div>
  );
}

function AddDevice({ actions, devices, helperRoot, cliDir }: { actions: Actions; devices: Device[]; helperRoot: string | null; cliDir: string | null }) {
  const [label, setLabel] = useState("");
  const [alias, setAlias] = useState("");
  const [socket, setSocket] = useState("");
  const [submitted, setSubmitted] = useState<{ id: string; at: number } | null>(null);
  const error = useErrorSince(submitted?.at ?? null, ["device."]);
  const problem = alias ? aliasProblem(alias) : null;
  const added = submitted ? devices.some((device) => device.id === submitted.id) : false;
  useEffect(() => {
    if (!added) return;
    setLabel("");
    setAlias("");
    setSocket("");
    setSubmitted(null);
  }, [added]);
  const pending = submitted !== null && !added && error === null;
  const blocked = pending || !label.trim() || !alias.trim() || problem !== null || socketProblem(socket) !== null;
  // Adding is where the whole kit is agreed to, once (PRD device-parity
  // D-12): the terms are on the form and there is one way to add.
  const submit = () => {
    if (blocked) return;
    const id = deviceIdFor(alias, devices.map((device) => device.id));
    setSubmitted({ id, at: Date.now() });
    actions.registerDevice(id, label.trim(), alias.trim(), { hostConsent: true, herdrSocketPath: socket.trim() || null });
  };
  return (
    <Group title="Add device" note="Use an alias already in the daemon machine's ~/.ssh/config. Hide connects right away and shows the result on the row.">
      <Row label="Label">
        <Input value={label} disabled={pending} placeholder="Studio" aria-label="Device label" className="w-(--size-settings-control-w)" onChange={(event) => setLabel(event.target.value)} data-device-label="true" />
      </Row>
      <Row label="SSH alias" detail={problem ? <Note tone="warn">{problem}</Note> : error ? <Note tone="error" data-add-device-error="true">{error}</Note> : null}>
        <Input
          mono
          value={alias}
          disabled={pending}
          placeholder="studio"
          aria-label="SSH alias"
          className="w-(--size-settings-control-w)"
          onChange={(event) => setAlias(event.target.value)}
          data-device-alias="true"
        />
      </Row>
      <Row label="Herdr socket" detail={socketProblem(socket) ? <Note tone="warn">{socketProblem(socket)}</Note> : <Note>Optional. Leave empty for the device's default Herdr server.</Note>}>
        <Input
          mono
          value={socket}
          disabled={pending}
          placeholder="default server"
          aria-label="Herdr socket on the device"
          className="w-(--size-settings-control-w)"
          onChange={(event) => setSocket(event.target.value)}
          data-device-socket="true"
        />
      </Row>
      <Row label={<span className="text-subtle-foreground">What Hide installs</span>} detail={<KitTerms helperRoot={helperRoot} cliDir={cliDir} />} />
      <Row label="">
        <Button disabled={blocked} onClick={submit} data-add-device="true">
          {pending ? "Adding…" : "Add"}
        </Button>
      </Row>
    </Group>
  );
}

// --- Shortcuts -----------------------------------------------------------------

function ShortcutsTab({ actions }: { actions: Actions }) {
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
  const current = (): Record<string, string> => ({ ...(diagnostic ? {} : (stored ?? {})) });
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
        title="Pane and navigation chords"
        note={
          host === "electron"
            ? system === "mac"
              ? "The macOS app's pane chords: this desktop app and the macOS app share them. Pane chords need ⌘; navigation can use ⌃ or ⌥ as well. A chord that macOS or the app menu keeps is refused before it is saved."
              : "This desktop app's pane chords. Pane chords need Ctrl+Shift, so a plain Ctrl key stays the terminal's; navigation can use Ctrl or Alt as well. A chord the system keeps is refused before it is saved."
            : system === "mac"
              ? "These chords are this browser host's own; the macOS and desktop apps keep their own set. A chord needs ⌘, ⌥ or ⌃, and one Chrome keeps is refused before it is saved."
              : "These chords are this browser host's own; the desktop app keeps its own set. A chord needs Ctrl or Alt, and one Chrome or the system keeps is refused before it is saved."
        }
      >
        {rowsFor(EDITABLE_PANE_COMMANDS.filter((id) => !(AREA_COMMANDS as readonly CommandId[]).includes(id)))}
        <Row label={<span className="text-subtle-foreground">Toggle Conversation</span>}>
          <Status tone="muted">macOS app only; the web shell has no conversation view</Status>
        </Row>
      </Group>
      <Group
        title="Area commands"
        note="Moving between Agent areas or View areas and resizing the one in use. They have no chord until you set one, and the desktop app lists them in its Pane menu."
        data-settings-group="area-commands"
      >
        {rowsFor(AREA_COMMANDS)}
      </Group>
      <Group
        title="Numbered chords"
        note={
          host === "electron"
            ? `The number is the order on screen: tabs left to right, Agents rows top to bottom, first to ninth. Hold ${system === "mac" ? "⌘ or ⌥" : "Ctrl+Shift or Alt"} to see it. These chords are fixed; a pane chord bound onto one is refused.`
            : system === "mac"
              ? "The desktop app's ⌘1-9 and ⌥1-9; a browser keeps its own ⌘1-9, so this host has no numbered chords."
              : "The desktop app's Ctrl+Shift+1-9 and Alt+1-9; a browser keeps its own Ctrl+1-9, so this host has no numbered chords."
        }
        data-settings-group="numbered-chords"
      >
        {sheetRows("Tabs", registry, host, system)
          .concat(sheetRows("Navigate", registry, host, system))
          .filter((row) => row.id.startsWith("select_"))
          .map((row) => (
            <Row key={row.id} label={row.title}>
              <Kbd data-shortcut-effective={row.id}>{row.chord ?? "-"}</Kbd>
              {row.chord === null ? <Status tone="muted">not on this host</Status> : null}
            </Row>
          ))}
      </Group>
      {diagnostic ? <Note tone="warn" data-shortcut-diagnostic="true">{diagnostic}</Note> : null}
      {saving ? <Note tone="pending">Saving…</Note> : null}
      {saveError ? <Note tone="error" data-shortcut-save-error="true">Not saved: {saveError}</Note> : null}
      <div className="mt-sm flex justify-end">
        <Button variant="secondary" disabled={!stored || Object.keys(stored).length === 0} onClick={() => apply({})} data-shortcut-reset-all="true">
          Restore all defaults
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
}: {
  id: CommandId;
  host: HostKind;
  system: KeySystem;
  registry: ReturnType<typeof resolvedRegistry>["registry"];
  overridden: boolean;
  onApply: (chord: Chord) => void;
  onReset: () => void;
}) {
  const command = registry.find((row) => row.id === id);
  const [recording, setRecording] = useState(false);
  const [draft, setDraft] = useState<Chord | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const setRecordingFlag = useUiStore((s) => s.setRecordingShortcut);
  useEffect(() => {
    if (!recording) return;
    setRecordingFlag(true);
    return () => setRecordingFlag(false);
  }, [recording, setRecordingFlag]);
  if (!command) return null;
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
    const reason = bindingProblem(id, chord, registry, host, system);
    setRecording(false);
    setProblem(reason);
    setDraft(reason ? null : chord);
  };
  return (
    <Row
      label={command.title}
      detail={
        problem ? (
          <Note tone="error" data-shortcut-problem={id}>
            {problem} {effective ? `${displayChord(effective, system)} stays.` : ""}
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
            Apply
          </Button>
          <Button variant="ghost" onClick={() => setDraft(null)}>
            Cancel
          </Button>
        </>
      ) : (
        <Button
          variant={recording ? "default" : "secondary"}
          aria-label={recording ? `Recording a chord for ${command.title}; press it, or Escape to cancel` : `Change ${command.title}`}
          onKeyDown={recording ? record : undefined}
          onBlur={() => setRecording(false)}
          onClick={() => {
            setProblem(null);
            setRecording(true);
          }}
          data-shortcut-record={id}
        >
          {recording ? "Press a chord…" : "Change"}
        </Button>
      )}
      {overridden && !draft ? (
        <Button variant="ghost" onClick={onReset} data-shortcut-reset={id}>
          Default
        </Button>
      ) : null}
    </Row>
  );
}
