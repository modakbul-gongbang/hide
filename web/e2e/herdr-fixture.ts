// An isolated pinned Herdr server with two fake agent panes, for the click
// flow e2e (PRD B7, B8). Nothing here touches the operator's socket: every
// command runs with a private HERDR_SOCKET_PATH, session, config and state.
//
// Sidebar rows exist only for panes Herdr classifies as an agent, and the
// pinned Herdr classifies by the process it started, so each pane runs a
// tiny compiled `claude` that copies stdin to stdout. A copied /bin/cat
// would not do: macOS kills a relocated platform binary. Like a real TUI it
// reads the terminal raw, byte by byte with no line discipline echo, and it
// appends every byte it reads to `HIDE_E2E_INPUT_LOG`: that file is what the
// PTY received, which is how a test tells a click's mouse report from what
// the shell only sent (PRD S2 B20).

import { execFileSync, spawn, spawnSync, type ChildProcess } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

export type HerdrFixture = {
  bin: string;
  socket: string;
  env: NodeJS.ProcessEnv;
  /** The fixture root; `fixture/` under it is the first workspace's cwd. */
  root: string;
  workspace: string;
  tab: string;
  panes: [string, string];
  /** Per agent pane, the file its `claude` shim appends every byte the PTY delivered to. */
  inputLogs: [string, string];
  /** The PATH the fixture's `claude` shim is on, for panes created later. */
  fixturePath: string;
  /** Runs a pinned-herdr CLI command against the private server and parses its JSON. */
  run: (args: string[]) => unknown;
  stop: () => void;
};

const SHIM_SOURCE = `#include <fcntl.h>
#include <stdlib.h>
#include <termios.h>
#include <unistd.h>
int main(void) {
  const char *log_path = getenv("HIDE_E2E_INPUT_LOG");
  int log = log_path ? open(log_path, O_WRONLY | O_CREAT | O_APPEND, 0644) : -1;
  struct termios tio;
  if (tcgetattr(0, &tio) == 0) {
    tio.c_lflag &= ~(ICANON | ECHO);
    tio.c_cc[VMIN] = 1;
    tio.c_cc[VTIME] = 0;
    tcsetattr(0, TCSANOW, &tio);
  }
  char b[4096]; ssize_t n;
  while ((n = read(0, b, sizeof b)) > 0) {
    if (log >= 0 && write(log, b, (size_t)n) < 0) return 1;
    if (write(1, b, (size_t)n) < 0) return 1;
  }
  return 0;
}
`;

export function pinnedHerdrVersion(): string {
  const manifest = path.resolve("..", "macos/Sources/HerdrMacOS/Resources/herdr-bundle.json");
  return (JSON.parse(fs.readFileSync(manifest, "utf8")) as { version: string }).version;
}

export function herdrBinary(): string {
  const candidates = [process.env.HIDE_E2E_HERDR_BIN, process.env.HERDR_BIN_PATH];
  for (const candidate of candidates) {
    if (candidate && fs.existsSync(candidate)) return candidate;
  }
  const onPath = spawnSync("/usr/bin/which", ["herdr"], { encoding: "utf8" }).stdout.trim();
  if (onPath) return onPath;
  throw new Error(
    "no herdr binary: set HIDE_E2E_HERDR_BIN or HERDR_BIN_PATH, or put the pinned herdr on PATH",
  );
}

function herdr(env: NodeJS.ProcessEnv, bin: string, args: string[]): unknown {
  const out = execFileSync(bin, args, { env, encoding: "utf8", timeout: 30_000 });
  return JSON.parse(out) as unknown;
}

function isolatedEnv(root: string, socket: string): NodeJS.ProcessEnv {
  const env = { ...process.env };
  for (const key of ["HERDR_PANE_ID", "HERDR_TAB_ID", "HERDR_WORKSPACE_ID", "HERDR_ENV"]) {
    delete env[key];
  }
  const config = path.join(root, "herdr-config.toml");
  fs.writeFileSync(config, "[update]\nversion_check = false\nmanifest_check = false\n");
  for (const dir of ["xdg-config", "xdg-state", "home", "fixture", "bin"]) {
    fs.mkdirSync(path.join(root, dir), { recursive: true });
  }
  // Herdr starts the pane shell from SHELL, so the fixture names it: the
  // CI runner's login shell is bash, and only zsh reads this private
  // HOME's .zshrc. A fixed prompt keeps the workstation's user and host
  // name out of screenshots and is what the fixture waits for.
  fs.writeFileSync(path.join(root, "home", ".zshrc"), "PS1='fixture %# '\n");
  return {
    ...env,
    SHELL: "/bin/zsh",
    HOME: path.join(root, "home"),
    HERDR_SESSION: `hide-e2e-${path.basename(root)}`,
    HERDR_SOCKET_PATH: socket,
    HERDR_CONFIG_PATH: config,
    XDG_CONFIG_HOME: path.join(root, "xdg-config"),
    XDG_STATE_HOME: path.join(root, "xdg-state"),
    HERDR_DISABLE_SOUND: "1",
  };
}

function paneText(env: NodeJS.ProcessEnv, bin: string, pane: string): string {
  const result = spawnSync(bin, ["pane", "read", pane, "--source", "visible", "--format", "text"], {
    env,
    encoding: "utf8",
    timeout: 10_000,
  });
  return result.status === 0 ? result.stdout : "";
}

async function waitFor(predicate: () => boolean, what: string, ms = 10_000): Promise<void> {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    if (predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`timed out waiting for ${what}`);
}

export async function startHerdr(): Promise<HerdrFixture> {
  const bin = herdrBinary();
  const version = execFileSync(bin, ["--version"], { encoding: "utf8" }).trim().split(/\s+/)[1];
  const pinned = pinnedHerdrVersion();
  if (version !== pinned) {
    throw new Error(`herdr ${version} at ${bin} is not the pinned ${pinned}`);
  }
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hide-e2e-herdr-"));
  // Unix socket paths are short; keep the node directly under /tmp.
  const socket = `/tmp/hide-e2e-${crypto.randomBytes(4).toString("hex")}.sock`;
  const env = isolatedEnv(root, socket);
  try {
    fs.writeFileSync(path.join(root, "shim.c"), SHIM_SOURCE);
    execFileSync("cc", ["-O1", "-o", path.join(root, "bin", "claude"), path.join(root, "shim.c")]);
  } catch (error) {
    fs.rmSync(root, { recursive: true, force: true });
    throw error;
  }
  const fixturePath = `${path.join(root, "bin")}:/usr/bin:/bin`;

  const log = fs.openSync(path.join(root, "herdr-server.log"), "w");
  const server: ChildProcess = spawn(bin, ["server"], { env, stdio: ["ignore", log, log] });
  const stop = () => {
    spawnSync(bin, ["server", "stop"], { env, timeout: 10_000 });
    if (server.exitCode === null) server.kill("SIGKILL");
    for (const file of [socket, socket.replace(/\.sock$/, "-client.sock")]) {
      fs.rmSync(file, { force: true });
    }
    // Run evidence for a reviewer, next to the screenshots: the server's log
    // and what each agent pane's PTY received.
    const keep = process.env.HIDE_E2E_SCREENSHOT_DIR;
    if (keep) {
      for (const name of ["herdr-server.log", "input-one.log", "input-two.log"]) {
        const file = path.join(root, name);
        if (fs.existsSync(file)) fs.copyFileSync(file, path.join(keep, `${path.basename(root)}-${name}`));
      }
    }
    fs.rmSync(root, { recursive: true, force: true });
  };
  try {
    await waitFor(() => fs.existsSync(socket), `herdr socket ${socket}`);
    const snapshot = herdr(env, bin, ["api", "snapshot"]) as {
      result?: { snapshot?: { workspaces?: unknown[] } };
    };
    const workspaces = snapshot.result?.snapshot?.workspaces ?? [];
    if (workspaces.length !== 0) throw new Error("private herdr server already has workspaces");

    const inputLogs: [string, string] = [path.join(root, "input-one.log"), path.join(root, "input-two.log")];
    const created = herdr(env, bin, [
      "workspace",
      "create",
      "--cwd",
      path.join(root, "fixture"),
      "--label",
      "e2e",
      "--env",
      `PATH=${fixturePath}`,
      "--env",
      `HIDE_E2E_INPUT_LOG=${inputLogs[0]}`,
      "--focus",
    ]) as { result: { workspace: { workspace_id: string }; tab: { tab_id: string }; root_pane: { pane_id: string } } };
    const first = created.result.root_pane.pane_id;
    const split = herdr(env, bin, [
      "pane",
      "split",
      first,
      "--direction",
      "right",
      "--env",
      `PATH=${fixturePath}`,
      "--env",
      `HIDE_E2E_INPUT_LOG=${inputLogs[1]}`,
      "--no-focus",
    ]) as { result: { pane: { pane_id: string } } };
    const second = split.result.pane.pane_id;
    // The shell must have printed its prompt before agent start accepts the
    // pane; the fixture .zshrc makes that prompt a fixed string.
    for (const pane of [first, second]) {
      await waitFor(() => paneText(env, bin, pane).includes("fixture %"), `a prompt in pane ${pane}`);
    }
    herdr(env, bin, ["agent", "start", "one", "--kind", "claude", "--pane", first]);
    herdr(env, bin, ["agent", "start", "two", "--kind", "claude", "--pane", second]);
    // Distinct row labels; report-metadata prints nothing on success.
    for (const [pane, task] of [
      [first, "Agent one"],
      [second, "Agent two"],
    ]) {
      execFileSync(bin, ["pane", "report-metadata", pane, "--source", "e2e", "--token", `task=${task}`], {
        env,
        timeout: 30_000,
      });
    }
    return {
      bin,
      socket,
      env,
      root,
      workspace: created.result.workspace.workspace_id,
      tab: created.result.tab.tab_id,
      panes: [first, second],
      inputLogs,
      fixturePath,
      run: (args) => herdr(env, bin, args),
      stop,
    };
  } catch (error) {
    stop();
    throw error;
  }
}
