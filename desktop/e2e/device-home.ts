// The device side of the SSH e2e. The isolated sshd logs in as this account,
// and connecting a device installs Hide's kit into that account's HOME
// (hooks in ~/.claude and ~/.codex, ~/.hide/kit, ~/.hcoord and hcoord's
// LaunchAgent), so these specs run only against an sshd that gives its
// sessions a private HOME, and they prove that before a device is registered.
//
// The sshd's config carries, besides the port, keys and known_hosts the
// HIDE_E2E_SSH_* variables name:
//
//   SetEnv HOME=<private> HCOORD_HOME=<private>/.hcoord
//
// <private> holds a `.hide-e2e-device-home` file and HIDE_E2E_SSH_HOME names
// it. HCOORD_HOME gives the kit's hcoord daemon a LaunchAgent label of its
// own (`plugins/hcoord/src/hcoord/platform.ts`); without it the daemon would
// take the account's `com.hcoord.daemon` and replace the operator's. The
// device's Node is the machine's own (`/opt/homebrew/bin` or PATH), and its
// herdr is the pinned one, linked into the private HOME's ~/.local/bin as a
// device's own install would be.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { herdrBinary } from "../../web/e2e/herdr-fixture";
import { HIDE_CLI, REPO } from "./fixture";

const MARKER = ".hide-e2e-device-home";
export const OPERATOR_HCOORD_LABEL = "com.hcoord.daemon";

/** The private HOME the isolated sshd gives its sessions, refused unless it is declared a test HOME. */
export function deviceHome(): string {
  const declared = process.env.HIDE_E2E_SSH_HOME;
  if (!declared || !path.isAbsolute(declared)) {
    throw new Error("HIDE_E2E_SSH_HOME must name the private HOME the isolated sshd sets (see desktop/e2e/device-home.ts)");
  }
  const home = fs.realpathSync(declared);
  assertTestHome(home);
  return home;
}

/** Refuses any folder that is the account's own HOME, by the environment or by the account database, or that is not marked as a test HOME. */
function assertTestHome(home: string): void {
  for (const own of [os.homedir(), os.userInfo().homedir]) {
    if (fs.existsSync(own) && home === fs.realpathSync(own)) throw new Error(`${home} is this account's own HOME`);
  }
  if (!fs.existsSync(path.join(home, MARKER))) throw new Error(`${home} has no ${MARKER}, so it is not declared a test HOME`);
}

/** An SSH config and known_hosts under the run's local HOME, one host entry per alias for the isolated server. */
export function writeSshConfig(localHome: string, aliases: string[]): void {
  const ssh = path.join(localHome, ".ssh");
  fs.mkdirSync(ssh, { recursive: true });
  fs.copyFileSync(process.env.HIDE_E2E_SSH_KNOWN_HOSTS!, path.join(ssh, "known_hosts"));
  fs.writeFileSync(path.join(ssh, "config"), aliases.flatMap((alias) => [
    `Host ${alias}`, "  HostName 127.0.0.1", `  Port ${process.env.HIDE_E2E_SSH_PORT}`,
    `  User ${os.userInfo().username}`, `  IdentityFile ${process.env.HIDE_E2E_SSH_KEY}`, "  IdentityAgent none", "",
  ]).join("\n"), { mode: 0o600 });
}

/**
 * Asks the server what HOME and HCOORD_HOME a session there gets, and refuses
 * unless they are the declared private HOME and a folder inside it. Returns
 * the HCOORD_HOME the device's hcoord will use.
 */
export function proveDeviceHome(localEnv: Record<string, string>, alias: string, home: string): string {
  const ssh = path.join(localEnv.HOME!, ".ssh");
  const answer = spawnSync("ssh", [
    "-F", path.join(ssh, "config"), "-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=yes",
    "-o", `UserKnownHostsFile=${path.join(ssh, "known_hosts")}`, "-o", "GlobalKnownHostsFile=/dev/null",
    alias, 'printf "%s\\n%s" "$HOME" "$HCOORD_HOME"',
  ], { env: localEnv, encoding: "utf8", timeout: 20_000 });
  if (answer.status !== 0) throw new Error(`the isolated sshd did not answer: ${answer.stderr}`);
  const [remoteHome = "", hcoordHome = ""] = answer.stdout.split("\n");
  if (!remoteHome || fs.realpathSync(remoteHome) !== home) {
    throw new Error(`a session on the isolated sshd gets HOME=${remoteHome}, not the private ${home}; the kit would write that account's own files`);
  }
  if (!hcoordHome.startsWith(remoteHome + path.sep)) {
    throw new Error(`a session on the isolated sshd gets HCOORD_HOME=${hcoordHome}; it must sit inside ${remoteHome} so hcoord's LaunchAgent label is its own`);
  }
  return hcoordHome;
}

/** hcoord's LaunchAgent label for an HCOORD_HOME, by hcoord's own rule. */
export function hcoordLabel(hcoordHome: string): string {
  return `com.hcoord.daemon.${createHash("sha256").update(path.resolve(hcoordHome)).digest("hex").slice(0, 12)}`;
}

/** The pid launchd reports for a label in this account's GUI domain, `loaded` without one, null when not loaded; it only reads. */
export function launchdPid(label: string): string | null {
  const printed = spawnSync("launchctl", ["print", `gui/${os.userInfo().uid}/${label}`], { encoding: "utf8", timeout: 10_000 });
  if (printed.status !== 0) return null;
  return printed.stdout.match(/^\s*pid = (\d+)/m)?.[1] ?? "loaded";
}

/** Unloads the test's own hcoord daemon; the operator's label is never passed here. */
export function bootoutTestLabel(label: string): void {
  if (label === OPERATOR_HCOORD_LABEL || !label.startsWith(`${OPERATOR_HCOORD_LABEL}.`)) throw new Error(`${label} is not a test hcoord label`);
  spawnSync("launchctl", ["bootout", `gui/${os.userInfo().uid}/${label}`], { encoding: "utf8", timeout: 15_000 });
}

export type AgentSettings = { hooks: Record<string, { hooks: { type: string; command: string; timeout?: number }[] }[]>; [key: string]: unknown };

export const claudeSettings = (home: string) => path.join(home, ".claude", "settings.json");
export const codexHooks = (home: string) => path.join(home, ".codex", "hooks.json");
export const readSettings = (file: string) => JSON.parse(fs.readFileSync(file, "utf8")) as AgentSettings;

/**
 * Puts the private HOME back to a device that has never met Hide: Herdr
 * installed in ~/.local/bin, the test's hcoord daemon unloaded, the kit's
 * folders gone, and the agent files seeded by `seedAgentFiles`.
 */
export function resetDeviceHome(home: string, label: string): { claude: AgentSettings; codex: AgentSettings } {
  assertTestHome(home);
  bootoutTestLabel(label);
  for (const name of [".claude", ".codex", ".hide", ".hcoord", ".local"]) fs.rmSync(path.join(home, name), { recursive: true, force: true });
  fs.rmSync(path.join(home, "Library", "LaunchAgents", `${label}.plist`), { force: true });
  fs.mkdirSync(path.join(home, ".local", "bin"), { recursive: true });
  fs.symlinkSync(herdrBinary(), path.join(home, ".local", "bin", "herdr"));
  return seedAgentFiles(home);
}

/** Each agent runtime's file holding only another tool's hook and setting, which every kit step must leave as it is. */
export function seedAgentFiles(home: string): { claude: AgentSettings; codex: AgentSettings } {
  const claude: AgentSettings = { theme: "dark", hooks: { SessionStart: [{ hooks: [{ type: "command", command: "/usr/bin/true other-tool-claude", timeout: 5 }] }] } };
  const codex: AgentSettings = { hooks: { SessionStart: [{ hooks: [{ type: "command", command: "/usr/bin/true other-tool-codex" }] }] } };
  for (const [file, settings] of [[claudeSettings(home), claude], [codexHooks(home), codex]] as const) {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, `${JSON.stringify(settings, null, 2)}\n`);
  }
  seedCodex(home);
  return { claude, codex };
}

/**
 * A `codex` in the account's `~/.local/bin` that has the shared daemon and
 * answers `codex features` the way Codex 0.160 does, so the kit's
 * `codex_per_pane` part is applied on a runner without Codex. A machine whose
 * PATH has a real Codex runs that one instead, against this HOME's `.codex`.
 */
function seedCodex(home: string): void {
  const bin = path.join(home, ".local", "bin");
  fs.mkdirSync(bin, { recursive: true });
  const script = [
    "#!/bin/sh",
    'state="${CODEX_HOME:-$HOME/.codex}/fake-daemon"',
    'case "$1 $2" in',
    "  '--version ') echo 'codex-cli 0.160.0' ;;",
    "  'features list') echo \"daemon_auto_start    stable  $(cat \"$state\" 2>/dev/null || echo true)\" ;;",
    "  'features disable') echo false > \"$state\" ;;",
    "  'features enable') echo true > \"$state\" ;;",
    "  *) exit 1 ;;",
    "esac",
    "",
  ].join("\n");
  fs.writeFileSync(path.join(bin, "codex"), script, { mode: 0o755 });
}

/**
 * A daemon folder laid out as the app's Contents/Resources: this build's
 * binaries and every kit part a device gets. The daemon reads the device
 * payload from beside its own executable, so a daemon started from here
 * installs hcoord too.
 */
export function stageBuild(root: string): string {
  const debug = path.dirname(HIDE_CLI);
  const build = path.join(root, "build");
  const hcoord = path.join(REPO, "plugins", "hcoord", "dist");
  const cli = path.join(hcoord, "hcoord", "cli.js");
  if (!fs.existsSync(cli)) throw new Error(`${cli} is missing: run \`pnpm --dir plugins/hcoord build\``);
  fs.mkdirSync(build, { recursive: true });
  const place = (source: string, target: string) => {
    try { fs.linkSync(source, target); } catch { fs.copyFileSync(source, target); fs.chmodSync(target, 0o755); }
  };
  for (const name of ["hided", "hide", "hide-agent-hooks", "hide-host-helper"]) {
    const binary = path.join(debug, name);
    if (!fs.existsSync(binary)) throw new Error(`${binary} is missing: run \`cargo build -p hided -p hide-agent-hooks -p hide-host\``);
    place(binary, path.join(build, name));
  }
  fs.cpSync(hcoord, path.join(build, "hcoord", "dist"), { recursive: true });
  return build;
}
