// The pure rules behind the Add device dialog (PRD settings-cleanup B50 to
// B52): which of the account's ssh config Hosts can be chosen, which are
// shown dimmed and why, and the name a chosen Host fills in. hided owns the
// list (`status.ssh_hosts`); this only decides how a row reads, so each rule
// is tested without a browser.

import type { MessageKey } from "./i18n/catalogs";
import type { SshHost } from "./snapshot";

/** One Host as the dialog lists it: chosen with the keyboard or the pointer, or dimmed with its reason. */
export type HostChoice =
  | { kind: "available"; alias: string; address: string }
  | { kind: "added"; alias: string; address: string | null; name: string }
  | { kind: "unresolved"; alias: string; reason: MessageKey };

const PROBLEM_REASON: Record<NonNullable<SshHost["problem"]>, MessageKey> = {
  ssh_missing: "devices.hostProblem.sshMissing",
  ssh_failed: "devices.hostProblem.sshFailed",
  timed_out: "devices.hostProblem.timedOut",
};

/**
 * A Host already used by a registered device, or reaching the same machine as
 * one, wins over everything else: it is dimmed as "Added as <name>" (B51).
 * Otherwise a Host whose address could not be read is dimmed with a short
 * reason (B52); the rest can be chosen.
 */
export function hostChoice(host: SshHost): HostChoice {
  if (host.added_as !== null) return { kind: "added", alias: host.alias, address: host.address, name: host.added_as };
  if (host.address === null) return { kind: "unresolved", alias: host.alias, reason: PROBLEM_REASON[host.problem ?? "ssh_failed"] };
  return { kind: "available", alias: host.alias, address: host.address };
}

export function hostChoices(hosts: readonly SshHost[]): HostChoice[] {
  return hosts.map(hostChoice);
}

/** The aliases a person can pick, in the order listed. */
export function pickableAliases(choices: readonly HostChoice[]): string[] {
  return choices.filter((choice) => choice.kind === "available").map((choice) => choice.alias);
}

/** The name a chosen Host starts with: its alias, which the person may rewrite. */
export function defaultDeviceName(alias: string): string {
  return alias;
}
