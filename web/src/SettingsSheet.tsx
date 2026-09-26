// The web Settings sheet (PRD S5 B1-B10, B18-B22): the native sheet's sections
// less Pet, each row reading a value the core or the daemon reported and each
// edit sent as the one core event that owns it. Nothing here decides a value;
// a pending edit shows as pending until the snapshot says it landed.

import { XIcon } from "lucide-react";
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
  helperConsentTerms,
  herdrLine,
  hostLine,
  offeredModels,
  ownerLine,
  providerLine,
  environmentTone,
  shown,
  socketProblem,
  usableAccent,
  usableFontSize,
  type SettingsTab,
} from "./settings";
import {
  EDITABLE_PANE_COMMANDS,
  bindingProblem,
  chordEquals,
  chordFromEvent,
  defaultBrowserChord,
  displayChord,
  resolvedRegistry,
  serializeChord,
  type Chord,
  type CommandId,
} from "./shortcuts";
import type { AgentHookRuntime, Device } from "./snapshot";
import { latestDraft } from "./editor/draft";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { hostKind } from "./host";

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
            {tab === "devices" ? <DevicesTab actions={actions} /> : null}
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

function AppearanceTab({ actions }: { actions: Actions }) {
  const accent = useShellStore((s) => usableAccent(s.rest?.ui_state?.accent_hex));
  const theme = useShellStore((s) => readTheme(s.rest?.ui_state?.theme).choice);
  const fontSize = useShellStore((s) => usableFontSize(s.rest?.ui_state?.font_size)) ?? FONT_SIZE_BASE;
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
      <Group title="Density" note="Terminal and editor text keep their own size (⌘= and ⌘- in a pane or document).">
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
  const daemonHost = useShellStore((s) => s.daemon?.host_name ?? null);
  const selectedDevice = useShellStore((s) => {
    const id = s.rest?.navigator?.focused_device_id ?? "local";
    return s.rest?.navigator?.devices?.find((device) => device.id === id && device.kind === "remote") ?? null;
  });
  const ai = useShellStore((s) => s.rest?.status?.background_ai);
  const hooks = useShellStore((s) => s.rest?.status?.agent_hooks);
  const [changedAt, setChangedAt] = useState<number | null>(null);
  const aiError = useErrorSince(changedAt, ["ai_settings."]);
  const [installing, setInstalling] = useState<AgentHookRuntime | null>(null);
  const [pressed, setPressed] = useState<{ id: string; at: number; headline: string } | null>(null);
  const hookError = useErrorSince(pressed?.at ?? null, ["agent_hooks."]);

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
    document.addEventListener("visibilitychange", report);
    return () => {
      document.removeEventListener("visibilitychange", report);
      actions.observeAgents(false);
    };
  }, [actions, live]);

  const selected = ai?.providers.find((provider) => provider.id === ai.provider) ?? null;
  const pendingRow = pressed ? hooks?.runtimes.find((row) => row.id === pressed.id) : null;
  const installSettled = pendingRow && pressed ? pendingRow.headline !== pressed.headline || hookError !== null : true;

  return (
    <>
      {selectedDevice ? (
        // A device's agents run under that machine's own hook files and CLIs
        // (PRD S5.5 B37); nothing here reads or writes them.
        <Row
          label={
            <Note tone="warn" data-agents-device-note={selectedDevice.id}>
              {selectedDevice.label} is selected, but everything on this page belongs to {daemonHost ?? "the daemon's machine"}. Hooks and Background AI for agents on {selectedDevice.label} are set up on that machine, by running Hide there and pressing Install hook in its own Settings. Hide does not copy hooks or AI settings over SSH or install anything on {selectedDevice.label} for them.
            </Note>
          }
        />
      ) : null}
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
        title="Subagent visibility"
        note="A hook reports what each agent spawns inside its own process. Hide installs it only when you press Install, into the file shown on the daemon's machine."
      >
        {(hooks?.runtimes ?? []).length === 0 ? <Row label={<Note>Hook state has not been read yet.</Note>} /> : null}
        {(hooks?.runtimes ?? []).map((runtime) => {
          const pending = pressed?.id === runtime.id && !installSettled;
          return (
            <Row
              key={runtime.id}
              label={
                <span className="flex min-w-0 flex-col">
                  <span className="font-semibold">{runtime.label}</span>
                  <span className="break-all font-mono text-caption text-muted-foreground">{runtime.path}</span>
                </span>
              }
              detail={pressed?.id === runtime.id && hookError ? <Note tone="error" data-hook-error={runtime.id}>Nothing was written: {hookError}</Note> : null}
            >
              <Status tone={runtime.installed ? "ok" : "warn"} data-hook-state={runtime.id}>
                {pending ? "Installing…" : runtime.headline}
              </Status>
              {runtime.offers_install ? (
                <Button variant="secondary" disabled={pending} onClick={() => setInstalling(runtime)} data-install-hook={runtime.id}>
                  {runtime.installed ? "Update hook" : "Install hook"}
                </Button>
              ) : null}
            </Row>
          );
        })}
        {hooks?.last_report_failure ? <Row label={<Note tone="error">{hooks.last_report_failure}</Note>} /> : null}
        {(hooks?.sessions_predating_install ?? []).map((pane) => (
          <Row key={pane.pane_id} label={<Note tone="warn">{`${pane.label} (${pane.pane_id}): ${pane.message}`}</Note>} />
        ))}
      </Group>
      {installing ? (
        <AlertDialog open onOpenChange={(next) => { if (!next) setInstalling(null); }}>
          <AlertDialogContent data-hook-confirm={installing.id}>
            <AlertDialogHeader>
              <AlertDialogTitle>Install the Hide hook for {installing.label}?</AlertDialogTitle>
              <AlertDialogDescription className="break-all font-mono text-caption text-muted-foreground">{installing.path}</AlertDialogDescription>
            </AlertDialogHeader>
            <p className="text-body text-subtle-foreground">
              Hide adds its own entries, marked hide-subagents, to this file on the daemon's machine. Every other entry and setting in it is kept as it is, and no other machine's file is written.
            </p>
            <AlertDialogFooter>
              <AlertDialogCancel>Cancel</AlertDialogCancel>
              <AlertDialogAction
                variant="default"
                data-hook-install-confirm="true"
                onClick={() => {
                  setPressed({ id: installing.id, at: Date.now(), headline: installing.headline });
                  actions.installHook(installing.id);
                }}
              >
                Install into {installing.label}
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      ) : null}
    </>
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
  const deviceError = useErrorSince(actedAt, ["device.", "remote."]);
  const rows = devices ?? [];
  const localRoot = rows.find((device) => device.kind !== "remote")?.host?.helper_root ?? null;
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
                  {device.test ? <DeviceTest test={device.test} /> : null}
                </>
              }
            >
              <Status tone={line.tone} data-device-state={`${device.id}:${device.state}`}>
                {line.text}
              </Status>
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
                      Allow helper…
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
      <AddDevice actions={actions} devices={rows} helperRoot={localRoot} />
      {allowing ? (
        <Dialog open onOpenChange={(next) => { if (!next) setAllowing(null); }}>
          <DialogContent data-device-host-allow-confirm={allowing.id}>
            <DialogHeader>
              <DialogTitle>Allow Hide&apos;s helper on {allowing.label}?</DialogTitle>
            </DialogHeader>
            <DialogBody className="space-y-sm">
              {allowing.host?.state === "identity_changed" ? (
                <p className="text-body text-warning">
                  {allowing.ssh_alias} now answers as a different SSH identity than the one this consent was given to. Allow only if you expect that change.
                </p>
              ) : null}
              <HelperTerms helperRoot={allowing.host?.helper_root ?? localRoot} />
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
                Allow helper
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
                Hide stops starting new file, Git and worktree work on {revoking.ssh_alias}. A save already sent is read back before its tab says anything; your drafts and the files on the device are not deleted, and neither is the installed helper.
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

function HelperTerms({ helperRoot }: { helperRoot: string | null }) {
  return (
    <ul className="list-disc space-y-xs pl-md text-body text-subtle-foreground" data-helper-terms="true">
      {helperConsentTerms(helperRoot).map((term) => (
        <li key={term}>{term}</li>
      ))}
    </ul>
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

function AddDevice({ actions, devices, helperRoot }: { actions: Actions; devices: Device[]; helperRoot: string | null }) {
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
  // Registering is where the helper is agreed to (PRD S5.5 B50): the terms
  // are on the form, and the operator picks one of the two outcomes.
  const submit = (hostConsent: boolean) => {
    if (blocked) return;
    const id = deviceIdFor(alias, devices.map((device) => device.id));
    setSubmitted({ id, at: Date.now() });
    actions.registerDevice(id, label.trim(), alias.trim(), { hostConsent, herdrSocketPath: socket.trim() || null });
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
      <Row label={<span className="text-subtle-foreground">Hide's helper</span>} detail={<HelperTerms helperRoot={helperRoot} />} />
      <Row label="">
        <Button variant="ghost" disabled={blocked} onClick={() => submit(false)} data-add-device-without-helper="true">
          Add without files
        </Button>
        <Button disabled={blocked} onClick={() => submit(true)} data-add-device="true">
          {pending ? "Adding…" : "Allow helper and add"}
        </Button>
      </Row>
    </Group>
  );
}

// --- Shortcuts -----------------------------------------------------------------

function ShortcutsTab({ actions }: { actions: Actions }) {
  const stored = useShellStore((s) => s.rest?.ui_state?.browser_shortcut_bindings);
  const { registry, diagnostic } = resolvedRegistry(stored);
  const [sentAt, setSentAt] = useState<number | null>(null);
  const [sent, setSent] = useState<Record<string, string> | null>(null);
  const saveError = useErrorSince(sentAt, ["ui_state."]);
  const saving = sent !== null && JSON.stringify(sent) !== JSON.stringify(stored ?? {}) && saveError === null;
  const apply = (bindings: Record<string, string>) => {
    setSent(bindings);
    setSentAt(Date.now());
    actions.setBrowserShortcuts(bindings);
  };
  const current = (): Record<string, string> => ({ ...(diagnostic ? {} : (stored ?? {})) });
  return (
    <>
      <Group
        title="Pane chords"
        note={
          hostKind() === "electron"
            ? "These chords apply when hide runs in a browser; this desktop app uses the chords its menus show."
            : "These chords are this browser host's own; the macOS app keeps its own set. A chord needs ⌘, ⌥ or ⌃, and one Chrome keeps is refused before it is saved."
        }
      >
        {EDITABLE_PANE_COMMANDS.map((id) => (
          <ShortcutRow
            key={id}
            id={id}
            registry={registry}
            overridden={!diagnostic && stored?.[id] !== undefined}
            onApply={(chord) => {
              const next = current();
              const fallback = defaultBrowserChord(id);
              if (fallback && chordEquals(fallback, chord)) delete next[id];
              else next[id] = serializeChord(chord);
              apply(next);
            }}
            onReset={() => {
              const next = current();
              delete next[id];
              apply(next);
            }}
          />
        ))}
        <Row label={<span className="text-subtle-foreground">Toggle Conversation</span>}>
          <Status tone="muted">macOS app only; the web shell has no conversation view</Status>
        </Row>
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
  registry,
  overridden,
  onApply,
  onReset,
}: {
  id: CommandId;
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
    const reason = bindingProblem(id, chord, registry);
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
            {problem} {command.browser ? `${displayChord(command.browser)} stays.` : ""}
          </Note>
        ) : null
      }
    >
      <Kbd data-shortcut-effective={id}>{command.browser ? displayChord(command.browser) : "-"}</Kbd>
      {draft ? (
        <>
          <span className="text-body text-subtle-foreground">→</span>
          <Kbd className="text-foreground" data-shortcut-draft={id}>
            {displayChord(draft)}
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
