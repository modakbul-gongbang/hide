// The pure decisions behind the Settings sheet: what a row says about a value
// the core or the daemon reported, and what the copied diagnostics carry.
// Nothing here reads the store, so every rule is tested without a browser.

import type { TFunction } from "i18next";
import type { DaemonInfo } from "./store";
import type { MessageKey } from "./i18n/catalogs";
import type { AccentName } from "./theme";
import type { AgentRow, AiProvider, CoreDiagnostic, Device, DeviceHost, EnvironmentStatus, HerdrStatus, KitAgent, KitComponent, KitComponentId, KitPiece, RemoteStatus, Workspace } from "./snapshot";

type Translate = TFunction<"translation">;

export type SettingsTab = "general" | "appearance" | "agents" | "issues" | "devices" | "mobile" | "performance" | "shortcuts";

export const SETTINGS_TABS: readonly SettingsTab[] = [
  "general", "appearance", "agents", "issues", "devices", "mobile", "performance", "shortcuts",
];

/**
 * The Sleep idle agents choices (PRD agent-sleep D-10), in the hours the core
 * accepts; Never is the default and sends null. `unit` is how the choice reads.
 */
export const SLEEP_AFTER_CHOICES: readonly { id: string; hours: number | null; unit: "hours" | "days" }[] = [
  { id: "never", hours: null, unit: "hours" },
  { id: "12", hours: 12, unit: "hours" },
  { id: "24", hours: 24, unit: "hours" },
  { id: "72", hours: 72, unit: "days" },
];

export function sleepAfterLabel(choice: (typeof SLEEP_AFTER_CHOICES)[number], t: Translate): string {
  if (choice.hours === null) return t("settings.sleep.never");
  return choice.unit === "days" ? t("settings.sleep.days", { days: choice.hours / 24 }) : t("settings.sleep.hours", { hours: choice.hours });
}

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

const GH_FAILURE_KEY: Record<string, MessageKey> = {
  "not installed": "issueSettings.ghNotInstalled",
  "not logged in": "issueSettings.ghNotLoggedIn",
  "network or rate limit": "issueSettings.readFailed",
};

/** What the GitHub row's state says; a category this build does not know is shown as the core named it. */
export function githubAccessLine(access: GithubAccess, t: Translate): { text: string; tone: "ok" | "warn" } {
  if (access.state === "connected") return { text: t("common.connected"), tone: "ok" };
  const key = GH_FAILURE_KEY[access.category];
  return { text: key ? t(key) : access.category, tone: "warn" };
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
  t: Translate,
): { value: IssueSourceChoice; options: { id: IssueSourceChoice; label: string }[] } {
  const source = workspace.tasks?.source ?? null;
  const resolved = source && (!source.chosen || !workspace.is_git) ? source.label : null;
  const repository = source?.kind === "github" ? source.name : (workspace.home_issues?.repository ?? null);
  const options: { id: IssueSourceChoice; label: string }[] = [
    { id: "auto", label: resolved ? t("issueSettings.autoResolved", { source: resolved }) : t("issueSettings.auto") },
    ...(workspace.is_git ? [{ id: "github" as const, label: repository ? t("issueSettings.githubRepository", { repository }) : "GitHub" }] : []),
    { id: "local", label: t("issueSettings.local") },
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
export const ACCENT_CHOICES: readonly { id: AccentName; token: string; swatch: string }[] = [
  { id: "lime", token: "--accent-choice-lime", swatch: "bg-accent-choice-lime" },
  { id: "sky", token: "--accent-choice-sky", swatch: "bg-accent-choice-sky" },
  { id: "violet", token: "--accent-choice-violet", swatch: "bg-accent-choice-violet" },
  { id: "amber", token: "--accent-choice-amber", swatch: "bg-accent-choice-amber" },
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

const isReported = (value: string | number | null | undefined): value is string | number => value !== null && value !== undefined && value !== "";

/** A value the daemon or core did not report reads as unavailable, never as a guess; the copied diagnostics keep that word as data. */
export function shown(value: string | number | null | undefined): string {
  return isReported(value) ? String(value) : "unavailable";
}

/** `shown` for the sheet, where the missing word follows the interface language. */
export function shownIn(value: string | number | null | undefined, t: Translate): string {
  return isReported(value) ? String(value) : t("settings.unavailable");
}

/** An environment check's tone: `invalid` always needs attention, `absent` only when required. */
export function environmentTone(state: string, required: boolean): "ok" | "warn" | "muted" {
  if (state === "invalid") return "warn";
  if (state === "absent") return required ? "warn" : "muted";
  return "ok";
}

/** One line for the Herdr connection, from the core's own state words. */
export function herdrLine(herdr: HerdrStatus | undefined, t: Translate): { text: string; tone: "ok" | "warn" | "error" } {
  const state = herdr?.state;
  if (!state) return { text: t("settings.unavailable"), tone: "warn" };
  if (state === "connected") return { text: t("common.connected"), tone: "ok" };
  if (herdr?.expected_protocol != null && herdr.received_protocol != null && herdr.expected_protocol !== herdr.received_protocol) {
    return { text: t("settings.protocolMismatch", { received: String(herdr.received_protocol), expected: String(herdr.expected_protocol) }), tone: "error" };
  }
  return { text: state.replace(/_/g, " "), tone: "warn" };
}

/**
 * What a device row says. The core names `ready`, `unavailable` and
 * `disabled`; the remote status beside it adds whether an attempt is still
 * pending, so "never tried", "trying" and "failed" read differently.
 */
function deviceState(device: Device, remote: RemoteStatus | undefined): { state: "local" | "ready" | "disabled" | "connecting" | "unavailable"; tone: "ok" | "warn" | "pending" | "local" } {
  if (device.kind !== "remote") return { state: "local", tone: "local" };
  if (device.state === "ready") return { state: "ready", tone: "ok" };
  if (device.state === "disabled") return { state: "disabled", tone: "warn" };
  if (remote?.state === "not_connected" || remote?.state === "connecting") return { state: "connecting", tone: "pending" };
  return { state: "unavailable", tone: "warn" };
}

export function deviceLine(device: Device, remote: RemoteStatus | undefined, t: Translate): { text: string; tone: "ok" | "warn" | "pending" | "local" } {
  const { state, tone } = deviceState(device, remote);
  return { text: t(`devices.line.${state}`), tone };
}

/**
 * The facts a device reported about itself (B36): its Herdr version and the
 * platform its helper runs on. Nothing here is read on this machine, so a
 * fact the device has not reported is left out rather than filled in.
 */
export function deviceFacts(device: Device, remote: RemoteStatus | undefined, t: Translate): string[] {
  if (device.kind !== "remote") return [];
  const facts: string[] = [];
  if (remote?.herdr_version) facts.push(`Herdr ${remote.herdr_version}`);
  if (device.host?.state === "ready" && device.host.platform) facts.push(t("devices.helperPlatform", { platform: device.host.platform }));
  return facts;
}

/**
 * A refused trust or sign-in step, in the operator's words, with the one
 * thing to do about it (B38). Hide never changes known_hosts or asks for a
 * password; each action happens in the daemon machine's own SSH setup.
 */
export function deviceProblemLine(problem: string | null | undefined, alias: string | null, t: Translate): { headline: string; action: string } | null {
  const target = alias ?? t("devices.targetFallback");
  switch (problem) {
    case "host_key_changed":
      return { headline: t("devices.problem.hostKeyChanged"), action: t("devices.problem.hostKeyChangedAction", { alias: target }) };
    case "host_key_unknown":
      return { headline: t("devices.problem.hostKeyUnknown"), action: t("devices.problem.hostKeyUnknownAction", { alias: target }) };
    case "authentication":
      return { headline: t("devices.problem.authentication"), action: t("devices.problem.authenticationAction", { alias: target }) };
    default:
      return null;
  }
}

/** The helper line a device row carries: whether file and Git work may run there, and why not. */
export function hostLine(host: DeviceHost | undefined, t: Translate): { text: string; tone: "ok" | "warn" | "pending" | "muted" | "local" } {
  if (!host || host.consent === "this_machine") return { text: t("devices.helper.local"), tone: "local" };
  switch (host.state) {
    case "ready":
      return { text: host.platform ? t("devices.helper.readyPlatform", { platform: host.platform }) : t("devices.helper.ready"), tone: "ok" };
    case "connecting":
      return { text: t("devices.helper.connecting"), tone: "pending" };
    case "not_allowed":
      return { text: host.consent === "outdated" ? t("devices.helper.consentOutdated") : t("devices.helper.notAllowed"), tone: "muted" };
    case "identity_changed":
      return { text: t("devices.helper.identityChanged"), tone: "warn" };
    case "unsupported":
      return { text: t("devices.helper.unsupported"), tone: "warn" };
    default:
      return { text: t("devices.helper.unavailable"), tone: "warn" };
  }
}

/**
 * What the operator agrees to when a device is added or allowed (PRD
 * device-parity D-12, B12): every part of Hide's kit and where it goes, what
 * keeps running, and what is never touched. The same words back the add form
 * and the row's Allow; they are said once, and nothing asks per part.
 */
export function kitConsentTerms(helperRoot: string | null, cliDir: string | null, t: Translate): string[] {
  return [
    t("devices.terms.copy", { root: helperRoot ?? t("devices.helperFolder") }),
    t("devices.terms.configure", { cliDir: cliDir ?? t("devices.commandFolder") }),
    t("devices.terms.codex"),
    t("devices.terms.runtime"),
    t("devices.terms.confirm"),
    t("devices.terms.remove"),
  ];
}

/**
 * What removing a device does to Hide's kit on it, in one line (PRD
 * device-parity B22): with its helper connected the parts come off and records
 * stay; without one nothing on the device changes.
 */
export function kitRemovalLine(device: Device, t: Translate): string {
  const where = device.ssh_alias ?? device.label;
  const sharing = device.kit?.shares_account_with;
  if (sharing) {
    return t("devices.removal.shared", { other: sharing, alias: where });
  }
  if (device.host?.state === "ready") {
    const folder = device.host?.helper_root ? ` (${device.host.helper_root})` : "";
    return t("devices.removal.connected", { alias: where, folder });
  }
  return t("devices.removal.disconnected", { alias: where });
}

/** A kit part's state as the machine rows word it (PRD device-parity B7). */
export function kitPartLine(part: Pick<KitComponent, "state">, t: Translate): { text: string; tone: "ok" | "warn" | "error" | "muted" } {
  switch (part.state) {
    case "installed":
      return { text: t("settings.kit.installed"), tone: "ok" };
    case "outdated":
      return { text: t("settings.kit.outdated"), tone: "warn" };
    case "not_installed":
      return { text: t("settings.kit.notInstalled"), tone: "warn" };
    case "removed":
      return { text: t("settings.kit.removed"), tone: "warn" };
    case "failed":
      return { text: t("settings.kit.failed"), tone: "error" };
    case "absent":
      return { text: t("settings.kit.absent"), tone: "muted" };
    case "off":
      return { text: t("common.off"), tone: "muted" };
  }
}

/**
 * The parts the operator switches on and off from their row rather than
 * taking away by hand (PRD overview-request-view D-24). A part with nothing
 * to switch on that machine, or one that could not be read, shows no switch.
 */
export const KIT_SWITCHED_PARTS: readonly KitComponentId[] = ["codex_per_pane"];

export function kitPartSwitch(part: KitComponent): { on: boolean } | null {
  if (!KIT_SWITCHED_PARTS.includes(part.id)) return null;
  if (part.state === "installed") return { on: true };
  if (part.state === "off" || part.state === "not_installed") return { on: false };
  return null;
}

/** Whether Reinstall would change this part: the same four states the core repairs (B8). */
export function kitPartNeedsReinstall(part: Pick<KitComponent, "state">): boolean {
  return part.state === "outdated" || part.state === "not_installed" || part.state === "removed" || part.state === "failed";
}

/** The pieces of an agent Hide manages: its skill, and its hook when it has one. */
function agentPieces(agent: KitAgent): KitPiece[] {
  return agent.hook ? [agent.skill, agent.hook] : [agent.skill];
}

/**
 * The piece the agent's row speaks for: the first that failed, else the first
 * that Reinstall would repair, else the skill. An agent that is off has no
 * piece to speak for.
 */
function agentWorstPiece(agent: KitAgent): KitPiece {
  const pieces = agentPieces(agent);
  return (
    pieces.find((piece) => piece.state === "failed") ??
    pieces.find((piece) => kitPartNeedsReinstall(piece)) ??
    pieces[0]!
  );
}

/** An agent's state as its row words it (agent adapters): off, or the state of its worst piece. */
export function kitAgentLine(agent: KitAgent, t: Translate): { text: string; tone: "ok" | "warn" | "error" | "muted"; reason: string | null } {
  if (!agent.enabled) {
    // A switch-off whose removal did not finish is not Off.
    const left = agentPieces(agent).find((piece) => piece.state === "failed");
    return left ? { text: t("settings.kit.failed"), tone: "error", reason: left.reason } : { text: t("common.off"), tone: "muted", reason: null };
  }
  const piece = agentWorstPiece(agent);
  const line = kitPartLine(piece, t);
  return { ...line, reason: piece.state === "installed" ? null : piece.reason };
}

/**
 * The agent's switch: an agent set up on the machine can be switched; one
 * that is not has nothing to switch, and one that is on keeps a switch so it
 * can be turned off.
 */
export function kitAgentSwitch(agent: KitAgent): { on: boolean } | null {
  if (agent.availability === "available" || agent.enabled) return { on: agent.enabled };
  return null;
}

/** Whether Reinstall would change something for this agent: only an agent that is on has pieces to repair. */
export function kitAgentNeedsReinstall(agent: KitAgent): boolean {
  return agent.enabled && agentPieces(agent).some((piece) => kitPartNeedsReinstall(piece));
}

/** What the agent's switch puts on the machine, named for the row's second line. */
export function kitAgentGets(agent: KitAgent, t: Translate): string {
  return t(agent.hook ? "settings.agentGetsSkillHook" : "settings.agentGetsSkill");
}

/**
 * Every machine's agents for the Agents tab (B27): This Mac first, then each
 * device in the Devices tab's order. `setUp` are the agents on the machine
 * (or on, which the operator can still turn off); `others` are the labels of
 * the rest, which have nothing to switch. A machine whose kit does not run
 * carries its reason instead of rows; one not checked yet carries neither.
 */
export function kitAgentMachines(devices: readonly Device[]): { device: Device; setUp: KitAgent[]; others: string[]; unavailable: string | null }[] {
  return devices
    .filter((device) => device.kind === "remote" || device.id === "local")
    .map((device) => {
      const agents = device.kit?.agents ?? [];
      return {
        device,
        setUp: agents.filter((agent) => kitAgentSwitch(agent) !== null),
        others: agents.filter((agent) => kitAgentSwitch(agent) === null).map((agent) => agent.label),
        unavailable: device.kit?.unavailable ?? null,
      };
    });
}

/** A device Herdr socket must be an absolute single-line path on that device, or left empty. */
export function socketProblem(path: string, t: Translate): string | null {
  const trimmed = path.trim();
  if (!trimmed) return null;
  // eslint-disable-next-line no-control-regex
  if (!trimmed.startsWith("/") || trimmed === "/" || /[\u0000-\u001f]/.test(trimmed)) return t("devices.socketInvalid");
  return null;
}

/** Whether Retry is offered: a registered SSH device that is not connected and not mid-attempt. */
export function canRetryDevice(device: Device, remote: RemoteStatus | undefined): boolean {
  return device.kind === "remote" && device.state !== "ready" && deviceState(device, remote).tone !== "pending";
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
  onlyExported: (tabId: string) => boolean,
  t: Translate,
): string[] {
  const scope = `remote:${deviceId}:`;
  const projects = registrations.filter((row) => row.device_id === deviceId).length;
  const own = tabs.filter((tab) => tab.checkout_id.startsWith(scope));
  const exportedOnly = own.filter((tab) => tab.dirty && onlyExported(tab.id)).length;
  const unsaved = own.filter((tab) => tab.dirty).length - exportedOnly + drafts.filter((draft) => draft.device === deviceId).length;
  const lines: string[] = [];
  if (projects > 0 || own.length > 0) {
    lines.push(t("devices.removal.effects", { projects: t("devices.removal.projects", { count: projects }), tabs: t("devices.removal.tabs", { count: own.length }) }));
  }
  if (unsaved > 0) lines.push(t("devices.removal.drafts", { count: unsaved }));
  if (exportedOnly > 0) lines.push(t("devices.removal.exported", { count: exportedOnly }));
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

/** What an SSH alias must look like before it is sent: one token, no spaces or shell syntax. */
export function aliasProblem(alias: string, t: Translate): string | null {
  const trimmed = alias.trim();
  if (!trimmed) return t("devices.aliasRequired");
  if (!/^[A-Za-z0-9._@-]+$/.test(trimmed)) return t("devices.aliasInvalid");
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
