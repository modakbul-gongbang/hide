// The pure decisions behind the Settings sheet: what a row says about a value
// the core or the daemon reported, and what the copied diagnostics carry.
// Nothing here reads the store, so every rule is tested without a browser.

import type { DaemonInfo } from "./store";
import type { AiProvider, CoreDiagnostic, Device, DeviceHost, EnvironmentStatus, HerdrStatus, RemoteStatus } from "./snapshot";

export type SettingsTab = "general" | "appearance" | "agents" | "devices" | "shortcuts";

export const SETTINGS_TABS: readonly { id: SettingsTab; title: string; subtitle: string }[] = [
  { id: "general", title: "General", subtitle: "This daemon, the Herdr runtime behind it, and where its state lives." },
  { id: "appearance", title: "Appearance", subtitle: "Accent and interface density. Dark is the only theme in this release." },
  { id: "agents", title: "Agents", subtitle: "The agent CLIs the daemon's machine can launch, Background AI, and hooks." },
  { id: "devices", title: "Devices", subtitle: "SSH targets. Authentication stays in the daemon machine's SSH environment." },
  { id: "shortcuts", title: "Shortcuts", subtitle: "Pane chords for this browser host. Every other chord is on the ⌘/ sheet." },
];

/** The interface font sizes Appearance offers, the native Settings range. */
export const FONT_SIZE_MIN = 11;
export const FONT_SIZE_MAX = 17;
/** The size every interface text token is drawn at when the setting is untouched. */
export const FONT_SIZE_BASE = 13;

/**
 * The accent choices, the native sheet's four, named by the token each one's
 * value and swatch come from; the class is spelled out for Tailwind's scanner.
 */
export const ACCENT_CHOICES: readonly { name: string; token: string; swatch: string }[] = [
  { name: "Lime", token: "--accent-choice-lime", swatch: "bg-accent-choice-lime" },
  { name: "Sky", token: "--accent-choice-sky", swatch: "bg-accent-choice-sky" },
  { name: "Violet", token: "--accent-choice-violet", swatch: "bg-accent-choice-violet" },
  { name: "Amber", token: "--accent-choice-amber", swatch: "bg-accent-choice-amber" },
];

const HEX_COLOR = /^#[0-9a-f]{6}$/i;

/** The stored accent when it is one the page can draw; anything else leaves the token default. */
export function usableAccent(value: unknown): string | null {
  return typeof value === "string" && HEX_COLOR.test(value) ? value.toLowerCase() : null;
}

/** The stored interface size when it is inside the offered range; anything else leaves the base. */
export function usableFontSize(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) && value >= FONT_SIZE_MIN && value <= FONT_SIZE_MAX ? value : null;
}

/** A value the daemon or core did not report reads as unavailable, never as a guess. */
export function shown(value: string | number | null | undefined): string {
  return value === null || value === undefined || value === "" ? "unavailable" : String(value);
}

/** An environment check's tone: `invalid` always needs attention, `absent` only when required. */
export function environmentTone(state: string, required: boolean): "ok" | "warn" | "muted" {
  if (state === "invalid") return "warn";
  if (state === "absent") return required ? "warn" : "muted";
  return "ok";
}

/** One line for the Herdr connection, from the core's own state words. */
export function herdrLine(herdr: HerdrStatus | undefined): { text: string; tone: "ok" | "warn" | "error" } {
  const state = herdr?.state;
  if (!state) return { text: "unavailable", tone: "warn" };
  if (state === "connected") return { text: "Connected", tone: "ok" };
  if (herdr?.expected_protocol != null && herdr.received_protocol != null && herdr.expected_protocol !== herdr.received_protocol) {
    return { text: `Protocol ${herdr.received_protocol} (expects ${herdr.expected_protocol})`, tone: "error" };
  }
  return { text: state.replace(/_/g, " "), tone: "warn" };
}

/**
 * What a device row says. The core names `ready`, `unavailable` and
 * `disabled`; the remote status beside it adds whether an attempt is still
 * pending, so "never tried", "trying" and "failed" read differently.
 */
export function deviceLine(device: Device, remote: RemoteStatus | undefined): { text: string; tone: "ok" | "warn" | "pending" | "local" } {
  if (device.kind !== "remote") return { text: "this daemon's machine", tone: "local" };
  if (device.state === "ready") return { text: "connected", tone: "ok" };
  if (device.state === "disabled") return { text: "disabled", tone: "warn" };
  if (remote?.state === "not_connected" || remote?.state === "connecting") return { text: "connecting…", tone: "pending" };
  return { text: "not connected", tone: "warn" };
}

/**
 * Where every value on these pages is kept (PRD S5.5 B35): the daemon's own
 * state on its machine, whichever device is selected in the sidebar.
 */
export function ownerLine(daemon: DaemonInfo | null, selected: Device | null): string {
  const host = daemon?.host_name ? daemon.host_name : "the daemon's machine";
  const kept = `Appearance, shortcuts, Background AI, hooks and the device list are kept by hided on ${host}.`;
  if (!selected || selected.kind !== "remote") return kept;
  return `${kept} ${selected.label} is selected; that changes where files, Git and panes run, not where these settings are kept.`;
}

/**
 * The facts a device reported about itself (B36): its Herdr version and the
 * platform its helper runs on. Nothing here is read on this machine, so a
 * fact the device has not reported is left out rather than filled in.
 */
export function deviceFacts(device: Device, remote: RemoteStatus | undefined): string[] {
  if (device.kind !== "remote") return [];
  const facts: string[] = [];
  if (remote?.herdr_version) facts.push(`Herdr ${remote.herdr_version}`);
  if (device.host?.state === "ready" && device.host.platform) facts.push(`helper on ${device.host.platform}`);
  return facts;
}

/**
 * A refused trust or sign-in step, in the operator's words, with the one
 * thing to do about it (B38). Hide never changes known_hosts or asks for a
 * password; each action happens in the daemon machine's own SSH setup.
 */
export function deviceProblemLine(problem: string | null | undefined, alias: string | null): { headline: string; action: string } | null {
  const target = alias ?? "the device";
  switch (problem) {
    case "host_key_changed":
      return {
        headline: "Host key changed",
        action: `${target} answered with a different host key than known_hosts records. Verify the device before you update known_hosts; Hide will not connect until then.`,
      };
    case "host_key_unknown":
      return {
        headline: "Host key not in known_hosts",
        action: `Run ssh ${target} once on the daemon's machine to review and record its host key, then Retry.`,
      };
    case "authentication":
      return {
        headline: "Sign-in refused",
        action: `The host key was verified, but no key the daemon machine's ssh config names for ${target} was accepted. Check its IdentityFile or IdentityAgent, then Retry.`,
      };
    default:
      return null;
  }
}

/** The helper line a device row carries: whether file and Git work may run there, and why not. */
export function hostLine(host: DeviceHost | undefined): { text: string; tone: "ok" | "warn" | "pending" | "muted" | "local" } {
  if (!host || host.consent === "this_machine") return { text: "files and Git run on this daemon's machine", tone: "local" };
  switch (host.state) {
    case "ready":
      return { text: `helper ready${host.platform ? ` (${host.platform})` : ""}`, tone: "ok" };
    case "connecting":
      return { text: "starting the helper…", tone: "pending" };
    case "not_allowed":
      return { text: host.consent === "outdated" ? "helper needs a new consent" : "helper not allowed", tone: "muted" };
    case "identity_changed":
      return { text: "device identity changed", tone: "warn" };
    case "unsupported":
      return { text: "helper unsupported here", tone: "warn" };
    default:
      return { text: "helper unavailable", tone: "warn" };
  }
}

/**
 * What the operator agrees to when Hide's helper is allowed on a device
 * (PRD S5.5 B50): where it is installed, when it runs, what an update may do,
 * and what it never does. The same words back the add form and the row's Allow.
 */
export function helperConsentTerms(helperRoot: string | null): string[] {
  return [
    `Hide copies one helper program into ${helperRoot ?? "the helper folder in the device account's home"} on the device, and replaces it there when this version of Hide needs a newer one.`,
    "It runs only while Hide holds the SSH connection and serves file, Git and worktree work for projects registered on that device. Nothing stays resident and nothing starts at login.",
    "It changes no hook, AI or shell settings there, and every move to the Trash or worktree removal still asks you for its target each time.",
    "A wider permission or a different SSH identity asks again; revoking stops new work and deletes no draft or remote file.",
  ];
}

/** A device Herdr socket must be an absolute single-line path on that device, or left empty. */
export function socketProblem(path: string): string | null {
  const trimmed = path.trim();
  if (!trimmed) return null;
  // eslint-disable-next-line no-control-regex
  if (!trimmed.startsWith("/") || trimmed === "/" || /[\u0000-\u001f]/.test(trimmed)) return "Enter an absolute socket path on the device, such as /Users/example/.config/herdr/herdr.sock.";
  return null;
}

/** Whether Retry is offered: a registered SSH device that is not connected and not mid-attempt. */
export function canRetryDevice(device: Device, remote: RemoteStatus | undefined): boolean {
  return device.kind === "remote" && device.state !== "ready" && deviceLine(device, remote).tone !== "pending";
}

/**
 * The id a new device registers under: the alias folded to the core's id
 * shape, with a numeric suffix when an existing device already holds it.
 */
export function deviceIdFor(alias: string, existing: readonly string[]): string {
  const base =
    alias
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9_-]+/g, "-")
      .replace(/^-+|-+$/g, "") || "device";
  const taken = new Set(existing);
  if (!taken.has(base) && base !== "local") return base;
  for (let index = 2; ; index += 1) {
    const candidate = `${base}-${index}`;
    if (!taken.has(candidate)) return candidate;
  }
}

/** What an SSH alias must look like before it is sent: one token, no spaces or shell syntax. */
/**
 * What removing a device takes from Hide: its registered projects and its
 * open file tabs; and the drafts it leaves, which stay in this browser to
 * export or discard (PRD S5.5 B26). Nothing on the device is counted, because
 * nothing there is touched.
 */
export function deviceRemovalLines(
  deviceId: string,
  registrations: readonly { device_id: string }[],
  tabs: readonly { id: string; checkout_id: string; dirty: boolean }[],
  drafts: readonly { device: string | null }[],
  /** Tabs whose draft is not stored here and leaves only as the file the operator exported (B44). */
  onlyExported: (tabId: string) => boolean = () => false,
): string[] {
  const scope = `remote:${deviceId}:`;
  const projects = registrations.filter((row) => row.device_id === deviceId).length;
  const own = tabs.filter((tab) => tab.checkout_id.startsWith(scope));
  const exportedOnly = own.filter((tab) => tab.dirty && onlyExported(tab.id)).length;
  const unsaved = own.filter((tab) => tab.dirty).length - exportedOnly + drafts.filter((draft) => draft.device === deviceId).length;
  const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
  const lines: string[] = [];
  if (projects > 0 || own.length > 0) {
    lines.push(`Hide forgets ${count(projects, "registered project", "registered projects")} and closes ${count(own.length, "file tab", "file tabs")} of it here.`);
  }
  if (unsaved > 0) {
    lines.push(`${count(unsaved, "unsaved draft stays", "unsaved drafts stay")} in this browser under unsaved drafts, to export or discard.`);
  }
  if (exportedOnly > 0) {
    lines.push(`${count(exportedOnly, "draft", "drafts")} could not be stored in this browser and ${exportedOnly === 1 ? "leaves" : "leave"} only as the file you exported.`);
  }
  return lines;
}

/**
 * The device's open tabs whose draft lives only in the tab because storing it
 * failed (B44). Removing the device closes its tabs, which would lose those
 * drafts, so the removal waits until each is exported or saved (B26): a tab
 * whose current draft is the text last exported is no longer held.
 */
export function unstoredDeviceDrafts(
  deviceId: string,
  tabs: readonly { id: string; checkout_id: string; path: string }[],
  unstored: ReadonlySet<string>,
  exported: (tabId: string) => boolean = () => false,
): string[] {
  const scope = `remote:${deviceId}:`;
  return tabs.filter((tab) => tab.checkout_id.startsWith(scope) && unstored.has(tab.id) && !exported(tab.id)).map((tab) => tab.path);
}

/**
 * Whether the tab's current draft is exactly the text the operator last
 * exported. A tab with no draft edited in this page exported the document it
 * holds, which only an edit (a draft) can change.
 */
export function draftExported(exported: ReadonlyMap<string, string>, current: (tabId: string) => string | null) {
  return (tabId: string) => {
    const text = exported.get(tabId);
    return text !== undefined && (current(tabId) ?? text) === text;
  };
}

export function aliasProblem(alias: string): string | null {
  const trimmed = alias.trim();
  if (!trimmed) return "Enter the SSH alias from the daemon machine's ~/.ssh/config.";
  if (!/^[A-Za-z0-9._@-]+$/.test(trimmed)) return "An SSH alias is one word: letters, digits, dot, dash, underscore or @.";
  return null;
}

/** The provider row's state as the operator reads it; the core's headline wins when it has one. */
export function providerLine(provider: AiProvider): { text: string; tone: "ok" | "warn" | "pending" } {
  const tone = provider.state === "ready" ? "ok" : provider.state === "unread" ? "pending" : "warn";
  const text = provider.headline || provider.state.replace(/_/g, " ");
  return { text: provider.message ? `${text}: ${provider.message}` : text, tone };
}

/** The models offered for a provider, always including the configured one so it is never swapped silently. */
export function offeredModels(provider: AiProvider): string[] {
  const models = [...provider.models];
  if (provider.model && !models.includes(provider.model)) models.unshift(provider.model);
  return models;
}

// A secret-shaped run: the page token and anything like it (32+ hex), or a
// `token=`/`key=`/`secret=`/`password=` assignment. Diagnostics are copied to be
// pasted elsewhere, so they never carry one even when a message quoted it.
const SECRET_RUN = /\b[0-9a-f]{32,}\b/gi;
const SECRET_ASSIGNMENT = /\b(token|key|secret|password|passphrase)=([^\s&]+)/gi;

export function redact(text: string): string {
  return text.replace(SECRET_ASSIGNMENT, "$1=[redacted]").replace(SECRET_RUN, "[redacted]");
}

export type DiagnosticsInput = {
  daemon: DaemonInfo | null;
  connection: string;
  herdr: HerdrStatus | undefined;
  environment: EnvironmentStatus[];
  diagnostics: CoreDiagnostic[];
  lastError: { kind: string; message: string } | null | undefined;
  userAgent: string;
};

/** How many of the core's newest diagnostics the copy carries. */
export const COPIED_DIAGNOSTICS = 40;

/**
 * The text Copy diagnostics puts on the clipboard: daemon identity, Herdr
 * state, environment checks and the core's own recent diagnostics. It never
 * carries the page token, terminal output or anything the operator typed into
 * a pane: none of those are inputs here, and every line is redacted anyway.
 */
export function diagnosticsText(input: DiagnosticsInput): string {
  const lines: string[] = ["Hide web shell diagnostics"];
  const daemon = input.daemon;
  lines.push(`daemon: ${daemon ? `hided ${daemon.version} pid ${daemon.pid} schema ${daemon.schema_version}` : "unavailable"}`);
  lines.push(`state: ${shown(daemon?.core_state_path)}`);
  lines.push(`herdr binary: ${shown(daemon?.herdr_bin_path)}`);
  lines.push(`herdr socket: ${shown(input.herdr?.socket_path ?? daemon?.herdr_socket_path)}`);
  lines.push(`herdr: ${shown(input.herdr?.state)} version ${shown(input.herdr?.received_version)} protocol ${shown(input.herdr?.received_protocol)}/${shown(input.herdr?.expected_protocol)}`);
  lines.push(`connection: ${input.connection}`);
  lines.push(`browser: ${input.userAgent}`);
  for (const row of input.environment) lines.push(`env ${row.key}: ${row.state}${row.message ? ` - ${row.message}` : ""}`);
  if (input.lastError) lines.push(`last error: ${input.lastError.kind}: ${input.lastError.message}`);
  for (const row of input.diagnostics.slice(-COPIED_DIAGNOSTICS)) {
    lines.push(`${new Date(row.occurred_at).toISOString()} ${row.kind}: ${row.message}`);
  }
  return redact(lines.join("\n"));
}
