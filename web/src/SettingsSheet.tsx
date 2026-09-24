// The web Settings sheet (PRD S5 B1-B10, B18-B22): the native sheet's sections
// less Pet, each row reading a value the core or the daemon reported and each
// edit sent as the one core event that owns it. Nothing here decides a value;
// a pending edit shows as pending until the snapshot says it landed.

import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import type { Actions } from "./actions";
import { Button, Dialog, Field, Group, Note, Row, Select, Status, Value } from "./components/ui/controls";
import {
  ACCENT_CHOICES,
  FONT_SIZE_BASE,
  FONT_SIZE_MAX,
  FONT_SIZE_MIN,
  SETTINGS_TABS,
  aliasProblem,
  canRetryDevice,
  deviceIdFor,
  deviceLine,
  diagnosticsText,
  herdrLine,
  offeredModels,
  providerLine,
  environmentTone,
  shown,
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
import { useShellStore } from "./store";
import { useUiStore } from "./ui";

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
  const [tab, setTab] = useState<SettingsTab>("general");
  const tabRefs = useRef<Record<string, HTMLButtonElement | null>>({});
  const subtitle = SETTINGS_TABS.find((row) => row.id === tab)?.subtitle ?? "";
  const moveTab = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowRight" && event.key !== "ArrowLeft") return;
    event.preventDefault();
    const index = SETTINGS_TABS.findIndex((row) => row.id === tab);
    const next = SETTINGS_TABS[(index + (event.key === "ArrowRight" ? 1 : SETTINGS_TABS.length - 1)) % SETTINGS_TABS.length];
    if (!next) return;
    setTab(next.id);
    tabRefs.current[next.id]?.focus();
  };
  return (
    <Dialog label="Settings" width="w-[var(--size-settings-sheet-w)] h-[var(--size-settings-sheet-h-max)]" onClose={close} data-settings="true">
      <header className="flex items-start gap-md border-b border-divider px-xl py-lg">
        <div className="min-w-0 flex-1">
          <h2 className="text-headline font-semibold text-primary">Settings</h2>
          <p className="text-body text-secondary">{subtitle}</p>
        </div>
        <Button appearance="quiet" aria-label="Close Settings" title="Close Settings (Esc)" onClick={close} data-settings-close="true">
          ✕
        </Button>
      </header>
      <div role="tablist" aria-label="Settings section" className="flex flex-wrap gap-xs border-b border-divider bg-sidebar px-xl py-sm" onKeyDown={moveTab}>
        {SETTINGS_TABS.map((row) => (
          <button
            key={row.id}
            ref={(element) => {
              tabRefs.current[row.id] = element;
            }}
            type="button"
            role="tab"
            id={`settings-tab-${row.id}`}
            aria-selected={tab === row.id}
            aria-controls="settings-panel"
            tabIndex={tab === row.id ? 0 : -1}
            data-settings-tab={row.id}
            className={`rounded-sm px-sm py-xxs text-body outline-none focus-visible:ring-1 focus-visible:ring-accent ${
              tab === row.id ? "bg-elevated font-semibold text-primary" : "text-secondary hover:text-primary"
            }`}
            onClick={() => setTab(row.id)}
          >
            {row.title}
          </button>
        ))}
      </div>
      <div id="settings-panel" role="tabpanel" aria-labelledby={`settings-tab-${tab}`} className="min-h-0 flex-1 overflow-auto px-xl py-lg">
        {tab === "general" ? <GeneralTab /> : null}
        {tab === "appearance" ? <AppearanceTab actions={actions} /> : null}
        {tab === "agents" ? <AgentsTab actions={actions} /> : null}
        {tab === "devices" ? <DevicesTab actions={actions} /> : null}
        {tab === "shortcuts" ? <ShortcutsTab actions={actions} /> : null}
      </div>
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
          <Button onClick={copyDiagnostics} data-copy-diagnostics="true">
            Copy diagnostics
          </Button>
          {copy === "copied" ? <Status tone="ok">Copied</Status> : null}
          {copy === "failed" ? <Status tone="error">The browser refused the clipboard</Status> : null}
        </Row>
      </Group>
      <Group title="Authentication">
        <Row label={<span className="text-body text-secondary">Hide delegates authentication to Herdr, SSH and the agent CLIs on the daemon's machine. It has no credential, token or passphrase field.</span>} />
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
        <li key={`${row.occurred_at}-${row.kind}`} className="break-words font-mono text-caption text-secondary">
          <span className="text-muted">{new Date(row.occurred_at).toLocaleTimeString()}</span> {row.kind}: {row.message}
        </li>
      ))}
    </ul>
  );
}

// --- Appearance ----------------------------------------------------------------

function accentHexOf(token: string): string | null {
  const value = getComputedStyle(document.documentElement).getPropertyValue(token).trim();
  return usableAccent(value)?.toUpperCase() ?? null;
}

function AppearanceTab({ actions }: { actions: Actions }) {
  const accent = useShellStore((s) => usableAccent(s.rest?.ui_state?.accent_hex));
  const fontSize = useShellStore((s) => usableFontSize(s.rest?.ui_state?.font_size)) ?? FONT_SIZE_BASE;
  const [changedAt, setChangedAt] = useState<number | null>(null);
  const error = useErrorSince(changedAt, ["ui_state."]);
  const [draftSize, setDraftSize] = useState(fontSize);
  useEffect(() => setDraftSize(fontSize), [fontSize]);
  return (
    <>
      <Group title="Theme" note="Accent tints controls and selection. Agent status colors keep their meaning whatever the accent.">
        <Row label="Accent" detail={error ? <Note tone="error" data-appearance-error="true">Not saved: {error}</Note> : null}>
          <div role="radiogroup" aria-label="Accent" className="flex items-center gap-sm">
            {ACCENT_CHOICES.map((choice) => {
              const hex = accentHexOf(choice.token);
              const selected = hex !== null && accent === hex.toLowerCase();
              return (
                <button
                  key={choice.token}
                  type="button"
                  role="radio"
                  aria-checked={selected}
                  aria-label={`Accent ${choice.name}`}
                  title={choice.name}
                  disabled={!hex}
                  data-accent={choice.name.toLowerCase()}
                  className={`h-[var(--size-checkbox)] w-[var(--size-checkbox)] rounded-full outline-none focus-visible:ring-1 focus-visible:ring-primary ${choice.swatch} ${
                    selected ? "ring-2 ring-primary" : "ring-1 ring-divider"
                  }`}
                  onClick={() => {
                    if (!hex) return;
                    setChangedAt(Date.now());
                    actions.setAccent(hex);
                  }}
                />
              );
            })}
            <Value>{accent ? accent.toUpperCase() : "default"}</Value>
          </div>
        </Row>
      </Group>
      <Group title="Density" note="Terminal and editor text keep their own size (⌘= and ⌘- in a pane or document).">
        <Row label="Interface font">
          <input
            type="range"
            min={FONT_SIZE_MIN}
            max={FONT_SIZE_MAX}
            step={1}
            value={draftSize}
            aria-label="Interface font size"
            aria-valuetext={`${draftSize} points`}
            data-font-size="true"
            className="w-[var(--size-settings-control-w)] accent-[var(--color-accent)]"
            onChange={(event) => setDraftSize(Number(event.target.value))}
            onPointerUp={() => {
              if (draftSize !== fontSize) {
                setChangedAt(Date.now());
                actions.setFontSize(draftSize);
              }
            }}
            onKeyUp={() => {
              if (draftSize !== fontSize) {
                setChangedAt(Date.now());
                actions.setFontSize(draftSize);
              }
            }}
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
            aria-label="Background AI agent"
            value={ai?.provider ?? ""}
            disabled={!ai || ai.providers.length === 0}
            data-ai-provider="true"
            onChange={(event) => {
              setChangedAt(Date.now());
              actions.chooseAi(event.target.value);
            }}
          >
            {(ai?.providers ?? []).map((provider) => (
              <option key={provider.id} value={provider.id}>
                {provider.label}
              </option>
            ))}
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
            aria-label="Background AI model"
            value={selected?.model ?? ""}
            disabled={!selected || offeredModels(selected).length < 2}
            data-ai-model="true"
            onChange={(event) => {
              if (!selected) return;
              setChangedAt(Date.now());
              actions.chooseAi(selected.id, event.target.value);
            }}
          >
            {(selected ? offeredModels(selected) : []).map((model) => (
              <option key={model} value={model}>
                {model}
              </option>
            ))}
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
                  <span className="break-all font-mono text-caption text-muted">{runtime.path}</span>
                </span>
              }
              detail={pressed?.id === runtime.id && hookError ? <Note tone="error" data-hook-error={runtime.id}>Nothing was written: {hookError}</Note> : null}
            >
              <Status tone={runtime.installed ? "ok" : "warn"} data-hook-state={runtime.id}>
                {pending ? "Installing…" : runtime.headline}
              </Status>
              {runtime.offers_install ? (
                <Button disabled={pending} onClick={() => setInstalling(runtime)} data-install-hook={runtime.id}>
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
        <Dialog label={`Install the Hide hook for ${installing.label}`} role="alertdialog" initialFocus="container" onClose={() => setInstalling(null)} data-hook-confirm={installing.id}>
          <div className="p-lg">
            <h2 className="mb-xs text-title font-semibold">Install the Hide hook for {installing.label}?</h2>
            <p className="mb-sm break-all font-mono text-body text-secondary">{installing.path}</p>
            <p className="mb-md text-body text-secondary">
              Hide adds its own entries, marked hide-subagents, to this file on the daemon's machine. Every other entry and setting in it is kept as it is, and no other machine's file is written.
            </p>
            <div className="flex justify-end gap-sm">
              <Button onClick={() => setInstalling(null)}>Cancel</Button>
              <Button
                appearance="prominent"
                data-hook-install-confirm="true"
                onClick={() => {
                  setPressed({ id: installing.id, at: Date.now(), headline: installing.headline });
                  actions.installHook(installing.id);
                  setInstalling(null);
                }}
              >
                Install into {installing.label}
              </Button>
            </div>
          </div>
        </Dialog>
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
  const [actedAt, setActedAt] = useState<number | null>(null);
  const deviceError = useErrorSince(actedAt, ["device.", "remote."]);
  const rows = devices ?? [];
  const remoteRows = rows.filter((device) => device.kind === "remote");
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
                  <span className="break-all font-mono text-caption text-muted">{device.kind === "remote" ? device.ssh_alias : "local, no SSH alias"}</span>
                </span>
              }
              detail={
                <>
                  {device.kind === "remote" && device.state !== "ready" && device.message ? <Note tone="warn">{device.message}</Note> : null}
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
                <Button appearance="quiet" onClick={() => actions.focusDevice(device.id)} data-device-select={device.id}>
                  Select
                </Button>
              )}
              {device.kind === "remote" ? (
                <>
                  <Button
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
                      onClick={() => {
                        setActedAt(Date.now());
                        actions.retryDevice(device.id);
                      }}
                      data-device-retry={device.id}
                    >
                      Retry
                    </Button>
                  ) : null}
                  <Button appearance="quiet" onClick={() => setRemoving(device)} data-device-remove={device.id}>
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
      <AddDevice actions={actions} devices={rows} />
      {removing ? (
        <Dialog label={`Remove ${removing.label}`} role="alertdialog" initialFocus="container" onClose={() => setRemoving(null)} data-device-remove-confirm={removing.id}>
          <div className="p-lg">
            <h2 className="mb-xs text-title font-semibold">Remove {removing.label}?</h2>
            <p className="mb-md text-body text-secondary">
              Hide forgets this device's registration and closes its connection here. Files, the Herdr server and any agents running on {removing.ssh_alias} keep running untouched.
            </p>
            <div className="flex justify-end gap-sm">
              <Button onClick={() => setRemoving(null)}>Keep device</Button>
              <Button
                appearance="danger"
                data-device-remove-go="true"
                onClick={() => {
                  setActedAt(Date.now());
                  actions.removeDevice(removing.id);
                  setRemoving(null);
                }}
              >
                Remove {removing.label}
              </Button>
            </div>
          </div>
        </Dialog>
      ) : null}
    </>
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
          <span className={stage.state === "passed" ? "text-success" : stage.state === "pending" ? "text-muted" : "text-warning"} aria-hidden="true">
            {stage.state === "passed" ? "✓" : stage.state === "pending" ? "…" : "✕"}
          </span>
          <span className="w-[var(--size-device-test-stage-col)] shrink-0 font-mono text-primary">{stage.stage}</span>
          <span className="min-w-0 break-words text-secondary">{stage.detail}</span>
        </div>
      ))}
    </div>
  );
}

function AddDevice({ actions, devices }: { actions: Actions; devices: Device[] }) {
  const [label, setLabel] = useState("");
  const [alias, setAlias] = useState("");
  const [submitted, setSubmitted] = useState<{ id: string; at: number } | null>(null);
  const error = useErrorSince(submitted?.at ?? null, ["device."]);
  const problem = alias ? aliasProblem(alias) : null;
  const added = submitted ? devices.some((device) => device.id === submitted.id) : false;
  useEffect(() => {
    if (!added) return;
    setLabel("");
    setAlias("");
    setSubmitted(null);
  }, [added]);
  const pending = submitted !== null && !added && error === null;
  const submit = () => {
    if (!label.trim() || aliasProblem(alias)) return;
    const id = deviceIdFor(alias, devices.map((device) => device.id));
    setSubmitted({ id, at: Date.now() });
    actions.registerDevice(id, label.trim(), alias.trim());
  };
  return (
    <Group title="Add device" note="Use an alias already in the daemon machine's ~/.ssh/config. Hide connects right away and shows the result on the row.">
      <Row label="Label">
        <Field value={label} mono={false} disabled={pending} placeholder="Studio" aria-label="Device label" className="w-[var(--size-settings-control-w)]" onChange={(event) => setLabel(event.target.value)} data-device-label="true" />
      </Row>
      <Row label="SSH alias" detail={problem ? <Note tone="warn">{problem}</Note> : error ? <Note tone="error" data-add-device-error="true">{error}</Note> : null}>
        <Field
          value={alias}
          disabled={pending}
          placeholder="studio"
          aria-label="SSH alias"
          className="w-[var(--size-settings-control-w)]"
          onChange={(event) => setAlias(event.target.value)}
          onKeyDown={(event) => {
            if (event.nativeEvent.isComposing) return;
            if (event.key === "Enter") submit();
          }}
          data-device-alias="true"
        />
      </Row>
      <Row label="">
        <Button appearance="prominent" disabled={pending || !label.trim() || !alias.trim() || problem !== null} onClick={submit} data-add-device="true">
          {pending ? "Adding…" : "Add device"}
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
        note="These chords are this browser host's own; the macOS app keeps its own set. A chord needs ⌘, ⌥ or ⌃, and one Chrome keeps is refused before it is saved."
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
        <Row label={<span className="text-secondary">Toggle Conversation</span>}>
          <Status tone="muted">macOS app only; the web shell has no conversation view</Status>
        </Row>
      </Group>
      {diagnostic ? <Note tone="warn" data-shortcut-diagnostic="true">{diagnostic}</Note> : null}
      {saving ? <Note tone="pending">Saving…</Note> : null}
      {saveError ? <Note tone="error" data-shortcut-save-error="true">Not saved: {saveError}</Note> : null}
      <div className="mt-sm flex justify-end">
        <Button disabled={!stored || Object.keys(stored).length === 0} onClick={() => apply({})} data-shortcut-reset-all="true">
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
      <kbd className="rounded-xs bg-elevated px-xs font-mono text-caption leading-[var(--size-keycap-height)] text-secondary" data-shortcut-effective={id}>
        {command.browser ? displayChord(command.browser) : "-"}
      </kbd>
      {draft ? (
        <>
          <span className="text-body text-secondary">→</span>
          <kbd className="rounded-xs bg-elevated px-xs font-mono text-caption leading-[var(--size-keycap-height)] text-primary" data-shortcut-draft={id}>
            {displayChord(draft)}
          </kbd>
          <Button
            appearance="prominent"
            onClick={() => {
              onApply(draft);
              setDraft(null);
            }}
            data-shortcut-apply={id}
          >
            Apply
          </Button>
          <Button appearance="quiet" onClick={() => setDraft(null)}>
            Cancel
          </Button>
        </>
      ) : (
        <Button
          appearance={recording ? "prominent" : "standard"}
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
        <Button appearance="quiet" onClick={onReset} data-shortcut-reset={id}>
          Default
        </Button>
      ) : null}
    </Row>
  );
}
