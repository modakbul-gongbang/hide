// Every environment key the desktop host reads, in one enumerable registry
// (engineering practice env.md). Nothing else under src/ reads
// the process environment; `env.test.ts` scans for it.
//
// The daemon's own keys (HIDE_STATE_DIR, HERDR_SOCKET_PATH, ...) are not
// read here: the `hide` CLI inherits this process's environment and
// `hided/src/env.rs` owns them. HOME, HERDR_BIN_PATH and HERDR_PANE_ID are
// read by both: the host reads the two Herdr keys only to know whether it
// may name the Herdr it bundles (`herdr.ts`).

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
    key: "HERDR_BIN_PATH",
    requirement: "optional",
    shape: "absolute path of the herdr binary",
    fallback: "a packaged app passes its own Contents/Resources/herdr to every `hide` child; unpackaged, nothing is added and hided resolves herdr from PATH",
    note: "Names the Herdr a daemon this host starts attaches pane terminals with; set without HERDR_PANE_ID it is an override passed through unchanged, so an isolated e2e or a development server keeps its own binary",
  },
  {
    key: "HERDR_PANE_ID",
    requirement: "optional",
    shape: "any non-empty value; only its presence is read",
    fallback: "absent: an inherited HERDR_BIN_PATH is an explicit override",
    note: "Herdr sets it in every pane beside its own HERDR_BIN_PATH, so a packaged app opened from a pane replaces that HERDR_BIN_PATH with its bundled herdr instead of inheriting a path that dies with the bundle that started the server",
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
    note: "Searched for `hide` after the override and the worktree build; every `hide` child gets system dirs when PATH is absent and standard user install dirs appended so a daemon started from Finder can find installed tools such as `gh`",
  },
];

export type DesktopEnv = {
  cliPath: string | null;
  userDataDir: string | null;
  herdrBinPath: string | null;
  herdrPaneId: string | null;
  shell: string;
  home: string;
  path: string;
  /** What every `hide` child inherits unchanged; hided's own keys travel in it and `hided/src/env.rs` owns them. */
  inherited: Record<string, string | undefined>;
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
    herdrBinPath: absolute("HERDR_BIN_PATH"),
    herdrPaneId: source.HERDR_PANE_ID || null,
    shell: absolute("SHELL") ?? "/bin/zsh",
    home: absolute("HOME") ?? os.userInfo().homedir,
    path: source.PATH ?? "",
    inherited: source,
  };
  if (problems.length > 0) {
    throw new Error(`desktop environment is not usable: ${problems.map((problem) => `${problem.key}: ${problem.kind}`).join("; ")}`);
  }
  return env;
}
