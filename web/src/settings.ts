// The pure decisions behind the Settings sheet: what a row says about a value
// the core or the daemon reported, and what the copied diagnostics carry.
// Nothing here reads the store, so every rule is tested without a browser.

import type { DaemonInfo } from "./store";
import type { AgentRow, AiProvider, CoreDiagnostic, Device, DeviceHost, EnvironmentStatus, HerdrStatus, KitComponent, KitComponentId, RemoteStatus, Workspace } from "./snapshot";

export type SettingsTab = "general" | "appearance" | "agents" | "issues" | "devices" | "mobile" | "performance" | "shortcuts";

export const SETTINGS_TABS: readonly { id: SettingsTab; title: string; subtitle: string }[] = [
  { id: "general", title: "General", subtitle: "This daemon, the Herdr runtime behind it, and where its state lives." },
  { id: "appearance", title: "Appearance", subtitle: "Theme, accent and interface density." },
  { id: "agents", title: "Agents", subtitle: "The agent CLIs the daemon's machine can launch, Background AI, and hooks." },
  { id: "issues", title: "Issues", subtitle: "이슈를 어디에 두고 어떻게 작업을 시작할지." },
  { id: "devices", title: "Devices", subtitle: "SSH targets. Authentication stays in the daemon machine's SSH environment." },
  { id: "mobile", title: "Mobile", subtitle: "이 맥의 hide를 폰에서 열고, 기다리는 에이전트에 답하고, 알림을 받습니다." },
  { id: "performance", title: "Performance", subtitle: "What this machine ends while you are away, and resumes when you come back." },
  { id: "shortcuts", title: "Shortcuts", subtitle: "The pane chords this host runs. Every other chord is on the Keyboard shortcuts sheet." },
];

/**
 * The Sleep idle agents choices (PRD agent-sleep D-10), in the hours the core
 * accepts; Never is the default and sends null.
 */
export const SLEEP_AFTER_CHOICES: readonly { id: string; hours: number | null; label: string }[] = [
  { id: "never", hours: null, label: "Never" },
  { id: "12", hours: 12, label: "12 hours" },
  { id: "24", hours: 24, label: "24 hours" },
  { id: "72", hours: 72, label: "3 days" },
];

/** The chosen Sleep idle agents choice; a value this build does not offer reads as Never. */
export function sleepAfterChoice(hours: unknown): string {
  return SLEEP_AFTER_CHOICES.find((choice) => choice.hours !== null && choice.hours === hours)?.id ?? "never";
}

/** How many of this machine's agents sleep now (B3); another device's rows never sleep. */
export function sleepingCount(agents: readonly AgentRow[] | undefined): number {
  return (agents ?? []).filter((agent) => agent.sleep && !agent.pane_id.startsWith("remote:")).length;
}

/** What `gh` answered across this Mac's Git projects, the Issues tab's GitHub row. */
export type GithubAccess = { state: "connected" } | { state: "failed"; category: string; reason: string | null };

/**
 * Whether GitHub reads work on this Mac: connected once any local Git
 * project's last read succeeded, otherwise the failure `gh` itself gave, and
 * null while nothing has answered. A repository with no GitHub remote says
 * nothing about `gh`, and a refusal of `gh` itself (not installed, not logged
 * in) is the same for every repository, so it speaks before a network one.
 */
export function githubAccess(workspaces: readonly Workspace[]): GithubAccess | null {
  const statuses = workspaces
    .filter((workspace) => workspace.is_git && !workspace.remote_target_id)
    .flatMap((workspace) => workspace.checkouts.map((checkout) => checkout.github))
    .filter((status) => status !== undefined);
  if (statuses.some((status) => status.last_success_at_unix_ms != null && !status.stale)) return { state: "connected" };
  const failure =
    statuses.find((status) => status.failure_category === "not installed" || status.failure_category === "not logged in") ??
    statuses.find((status) => status.failure_category != null && status.failure_category !== "no GitHub remote");
  return failure?.failure_category ? { state: "failed", category: failure.failure_category, reason: failure.unavailable_reason } : null;
}

const GH_FAILURE_TEXT: Record<string, string> = {
  "not installed": "gh 설치 안 됨",
  "not logged in": "gh 로그인 안 됨",
  "network or rate limit": "읽기 실패",
};

/** What the GitHub row's state says; a category this build does not know is shown as the core named it. */
export function githubAccessLine(access: GithubAccess): { text: string; tone: "ok" | "warn" } {
  if (access.state === "connected") return { text: "연결됨", tone: "ok" };
  return { text: GH_FAILURE_TEXT[access.category] ?? access.category, tone: "warn" };
}

export type IssueSourceChoice = "auto" | "github" | "local";

/**
 * A local project's issue source choices and the one in force. Auto names
 * the source it resolves to only where the core's answer shows it: the
 * project reads its default now, or it is a folder, which is always Local.
 * GitHub is offered only to a Git project, and a stored choice the project
 * cannot take (GitHub for a folder) reads as Auto, the source the core falls
 * back to.
 */
export function issueSourceChoices(
  workspace: Workspace,
  stored: string | undefined,
): { value: IssueSourceChoice; options: { id: IssueSourceChoice; label: string }[] } {
  const source = workspace.tasks?.source ?? null;
  const resolved = source && (!source.chosen || !workspace.is_git) ? source.label : null;
  const repository = source?.kind === "github" ? source.name : (workspace.home_issues?.repository ?? null);
  const options: { id: IssueSourceChoice; label: string }[] = [
    { id: "auto", label: resolved ? `자동 (${resolved})` : "자동" },
    ...(workspace.is_git ? [{ id: "github" as const, label: repository ? `GitHub · ${repository}` : "GitHub" }] : []),
    { id: "local", label: "Local" },
  ];
  return { value: options.find((option) => option.id === stored)?.id ?? "auto", options };
}

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
 * What the operator agrees to when a device is added or allowed (PRD
 * device-parity D-12, B12): every part of Hide's kit and where it goes, what
 * keeps running, and what is never touched. The same words back the add form
 * and the row's Allow; they are said once, and nothing asks per part.
 */
export function kitConsentTerms(helperRoot: string | null, cliDir: string | null): string[] {
  const root = helperRoot ?? "the helper folder in the device account's home";
  return [
    `Hide copies its helper, the hide command, its hook helper and hcoord into ${root}, and replaces them there when this version of Hide needs newer ones.`,
    `It links hide and hcoord in ${cliDir ?? "the account's command folder"}, adds its own entries to ~/.claude/settings.json and ~/.codex/hooks.json, and installs hcoord at ~/.hide/hcoord/bin/hcoord with the device's Node, moving an existing ~/.hcoord there. It keeps its records in ~/.hide and takes the folders of an older Hide layout away. Another tool's entries and files are left as they are.`,
    "The helper runs only while Hide holds the SSH connection, serves registered projects, and manages the device's Home (~/hide and its project links). Agent labels for the device's panes are made on this Mac from conversations the helper reads, and hcoord keeps its own daemon.",
    "Every move to the Trash or worktree removal still asks you for its target each time. A part you take away stays away until you press Reinstall.",
    "Removing the device takes Hide's parts and its helper folder off it again; hcoord and the records in ~/.hide stay. A different SSH identity asks again.",
  ];
}

/**
 * What removing a device does to Hide's kit on it, in one line (PRD
 * device-parity B22): with its helper connected the parts come off and hcoord
 * stays; without one nothing on the device changes.
 */
export function kitRemovalLine(device: Device): string {
  const where = device.ssh_alias ?? device.label;
  const sharing = device.kit?.shares_account_with;
  if (sharing) {
    return `${sharing} reaches the same account on ${where}, so Hide's kit stays there for it.`;
  }
  if (device.host?.state === "ready") {
    const folder = device.host?.helper_root ? ` (${device.host.helper_root})` : "";
    return `On ${where}, Hide removes its hook entries, its hide link and its helper folder${folder}; hcoord and the records in ~/.hide stay.`;
  }
  return `Hide's helper is not connected to ${where}, so its kit stays there; it does not get in the way of agent sessions, and adding the device again replaces it.`;
}

/** A kit part's state as the machine rows word it (PRD device-parity B7). */
export function kitPartLine(part: KitComponent): { text: string; tone: "ok" | "warn" | "error" | "muted" } {
  switch (part.state) {
    case "installed":
      return { text: "Installed", tone: "ok" };
    case "outdated":
      return { text: "Outdated", tone: "warn" };
    case "not_installed":
      return { text: "Not installed", tone: "warn" };
    case "removed":
      return { text: "Removed", tone: "warn" };
    case "failed":
      return { text: "Failed", tone: "error" };
    case "absent":
      return { text: "Not on this machine", tone: "muted" };
  }
}

/** Whether Reinstall would change this part: the same four states the core repairs (B8). */
export function kitPartNeedsReinstall(part: KitComponent): boolean {
  return part.state === "outdated" || part.state === "not_installed" || part.state === "removed" || part.state === "failed";
}

export const KIT_HOOK_PARTS: readonly KitComponentId[] = ["claude_code_hook", "codex_hook"];

/**
 * Every machine's hook parts for the Agents tab (B27): This Mac first, then
 * each device in the Devices tab's order. A machine whose kit does not run
 * carries its reason instead of rows; one not checked yet carries neither.
 */
export function kitHookMachines(devices: readonly Device[]): { device: Device; parts: KitComponent[]; unavailable: string | null }[] {
  return devices
    .filter((device) => device.kind === "remote" || device.id === "local")
    .map((device) => ({
      device,
      parts: (device.kit?.components ?? []).filter((part) => KIT_HOOK_PARTS.includes(part.id)),
      unavailable: device.kit?.unavailable ?? null,
    }));
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
