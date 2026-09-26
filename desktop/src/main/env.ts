// Every environment key the desktop host reads, in one enumerable registry
// (engineering practice env.md). Nothing else under src/ reads
// the process environment; `env.test.ts` scans for it.
//
// The daemon's own keys (HIDE_STATE_DIR, HERDR_SOCKET_PATH, ...) are not
// read here: the `hide` CLI inherits this process's environment and
// `hided/src/env.rs` owns them. HOME is read by both.

import os from "node:os";
import path from "node:path";

export type EnvKey = {
  key: string;
  requirement: "optional";
  shape: string;
  fallback: string;
  note: string;
};

export const ENV_REGISTRY: readonly EnvKey[] = [
  {
    key: "HIDE_CLI_PATH",
    requirement: "optional",
    shape: "absolute path of an executable `hide` CLI",
    fallback: "this worktree's target/{debug,release}/hide when run unpackaged, then PATH, the CLI that last attached, the login shell's PATH (packaged only), and ~/.local/bin, /opt/homebrew/bin, /usr/local/bin",
    note: "Names the CLI the host asks for the daemon; when it is set but not executable the window says the CLI was not found instead of searching on",
  },
  {
    key: "HIDE_DESKTOP_USER_DATA_DIR",
    requirement: "optional",
    shape: "absolute directory path",
    fallback: "~/Library/Application Support/hide-desktop",
    note: "Isolates window state, the single-instance lock, web storage and the host log for a QA or e2e instance; without it the operator's desktop profile is used",
  },
  {
    key: "SHELL",
    requirement: "optional",
    shape: "absolute path of the login shell",
    fallback: "/bin/zsh",
    note: "A packaged app launched from Finder asks this shell for the operator's PATH when neither PATH nor the CLI that last attached has `hide`",
  },
  {
    key: "HOME",
    requirement: "optional",
    shape: "absolute directory path",
    fallback: "the account's home directory",
    note: "Names the `~/.local/bin` the CLI search tries last; Electron's own home path ignores it, which would let an isolated QA instance find the operator's CLI",
  },
  {
    key: "PATH",
    requirement: "optional",
    shape: "colon-separated directories",
    fallback: "empty: the search goes on to the CLI that last attached",
    note: "Searched for `hide` after the override and the worktree build; an app opened from Finder gets launchd's bare PATH here",
  },
];

export type DesktopEnv = {
  cliPath: string | null;
  userDataDir: string | null;
  shell: string;
  home: string;
  path: string;
};

export type EnvProblem = { key: string; kind: string };

/** Validates every key at once; a problem names the key and the kind, never the value. */
export function loadEnv(source: Record<string, string | undefined>): DesktopEnv {
  const problems: EnvProblem[] = [];
  const absolute = (key: string): string | null => {
    const value = source[key];
    if (value === undefined || value === "") return null;
    if (!path.isAbsolute(value)) {
      problems.push({ key, kind: "not an absolute path" });
      return null;
    }
    return value;
  };
  const env: DesktopEnv = {
    cliPath: absolute("HIDE_CLI_PATH"),
    userDataDir: absolute("HIDE_DESKTOP_USER_DATA_DIR"),
    shell: absolute("SHELL") ?? "/bin/zsh",
    home: absolute("HOME") ?? os.userInfo().homedir,
    path: source.PATH ?? "",
  };
  if (problems.length > 0) {
    throw new Error(`desktop environment is not usable: ${problems.map((problem) => `${problem.key}: ${problem.kind}`).join("; ")}`);
  }
  return env;
}
