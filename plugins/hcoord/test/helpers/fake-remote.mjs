// A fake Herdr and ssh pair for hcoord tests. Each host (local or a saved
// machine label) keeps its own agents, panes, worktrees and prompt log under
// one root, `herdr --machine <label>` routes to that host, and `ssh <target>`
// runs its command with the remote host's HOME. Nothing here touches the real
// Herdr server, launchd, or network.
import fs from "node:fs";
import path from "node:path";

const HERDR = String.raw`#!/usr/bin/env node
const fs = require("node:fs"), path = require("node:path");
const root = process.env.FAKE_REMOTE_ROOT;
let argv = process.argv.slice(2), host = process.env.FAKE_REMOTE_HOST || "local";
const machines = () => JSON.parse(fs.readFileSync(path.join(root, "machines.json"), "utf8"));
if (argv[0] === "--machine") {
  const label = argv[1];
  const found = machines().find((m) => m.label === label || m.id === label);
  if (!found || !found.enabled) { process.stderr.write("error: unknown machine '" + label + "'; use ` + "`herdr machine list`" + String.raw`\n"); process.exit(2); }
  if (fs.existsSync(path.join(root, "h", label, "down"))) { process.stderr.write("ssh: connect to host " + found.target + " port 22: Operation timed out\n"); process.exit(255); }
  host = label; argv = argv.slice(2);
}
const dir = path.join(root, "h", host);
fs.mkdirSync(dir, { recursive: true });
const load = (name, fallback) => { const file = path.join(dir, name); return fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, "utf8")) : fallback; };
const save = (name, value) => fs.writeFileSync(path.join(dir, name), JSON.stringify(value));
const log = (name, value) => fs.appendFileSync(path.join(dir, name), JSON.stringify(value) + "\n");
const out = (result) => process.stdout.write(JSON.stringify({ id: "cli", result }));
const fail = (code, message, exit = 1) => { process.stderr.write(JSON.stringify({ error: { code, message } })); process.exit(exit); };
const flag = (name) => { const index = argv.indexOf(name); return index < 0 ? undefined : argv[index + 1]; };
log("calls.jsonl", argv);
// A stopped server: the socket is left behind and every call is refused, as after a reboot.
if (fs.existsSync(path.join(dir, "server-down"))) { process.stderr.write("connect failed: Connection refused (os error 61)\n"); process.exit(1); }
const agents = load("agents.json", {}), panes = load("panes.json", {});
const info = (pane, a) => ({ pane_id: pane, name: a.name, agent: a.kind, agent_session: a.session ? { value: a.session } : undefined, terminal_id: a.instance, agent_status: a.status, interactive_ready: a.ready !== false, cwd: panes[pane]?.cwd });
const [a0, a1, a2] = argv;
if (a0 === "--help") process.stdout.write("Usage: herdr --machine <label-or-id> <command>\n");
else if (a0 === "machine" && a1 === "list") process.stdout.write(JSON.stringify(machines()));
else if (a0 === "agent" && a1 === "get") { const a = agents[a2]; if (!a) fail("agent_not_found", "no agent"); out({ type: "agent_info", agent: info(a2, a) }); }
else if (a0 === "agent" && a1 === "list") out({ type: "agent_list", agents: Object.entries(agents).map(([pane, a]) => info(pane, a)) });
else if (a0 === "agent" && a1 === "read") process.stdout.write("› Ask Codex to do anything");
else if (a0 === "agent" && a1 === "prompt") {
  if (a2 === "--help") process.stdout.write("Usage: herdr agent prompt <TARGET> <TEXT>");
  else { log("prompts.jsonl", { target: a2, text: argv[3] }); out({ outcome: "submitted" }); }
}
else if (a0 === "agent" && a1 === "start") {
  const pane = flag("--pane");
  if (!panes[pane]) fail("pane_not_found", "no pane");
  if (fs.existsSync(path.join(dir, "start-fails"))) { process.stderr.write("start outcome unknown"); process.exit(8); }
  if (fs.existsSync(path.join(dir, "start-blocked"))) {
    // Like Claude's folder-trust prompt: the agent exists, shows its own question, and reports no session.
    agents[pane] = { name: a2, kind: flag("--kind"), session: null, instance: a2 + "-instance", status: "blocked", ready: false };
    save("agents.json", agents); process.stderr.write(JSON.stringify({ error: { code: "agent_not_ready", message: "agent did not become ready" } })); process.exit(1);
  }
  agents[pane] = { name: a2, kind: flag("--kind"), session: a2 + "-session", instance: a2 + "-instance", status: "idle", ready: true };
  save("agents.json", agents); out({ agent: { name: a2, pane_id: pane } });
}
else if (a0 === "pane" && a1 === "list") out({ type: "pane_list", panes: Object.entries(panes).map(([pane, p]) => ({ pane_id: pane, workspace_id: p.workspace, cwd: p.cwd, tokens: p.tokens ?? {}, ...(agents[pane] ? { agent_session: agents[pane].session ? { value: agents[pane].session } : undefined, terminal_id: agents[pane].instance } : {}) })) });
else if (a0 === "pane" && a1 === "get") { const p = panes[a2]; if (!p) fail("pane_not_found", "no pane"); out({ type: "pane_info", pane: { pane_id: a2, workspace_id: p.workspace, cwd: p.cwd, tokens: p.tokens ?? {} } }); }
else if (a0 === "pane" && a1 === "report-metadata") {
  const p = panes[a2]; if (!p) fail("pane_not_found", "no pane");
  if (fs.existsSync(path.join(dir, "metadata-denied"))) fail("permission_denied", "metadata denied");
  p.tokens ??= {};
  for (let index = 3; index < argv.length; index += 1) {
    if (argv[index] === "--token") { const [key, ...parts] = argv[index + 1].split("="); p.tokens[key] = parts.join("="); index += 1; }
    else if (argv[index] === "--clear-token") { delete p.tokens[argv[index + 1]]; index += 1; }
  }
  save("panes.json", panes); out({ outcome: "reported" });
}
else if (a0 === "tab" && a1 === "create") { const label = flag("--label"), pane = label + "-pane"; panes[pane] = { workspace: flag("--workspace"), cwd: flag("--cwd") }; save("panes.json", panes); out({ root_pane: { pane_id: pane } }); }
else if (a0 === "worktree" && a1 === "create" && a2 === "--help") process.stdout.write("Usage: herdr worktree create [OPTIONS]");
else if (a0 === "worktree" && a1 === "create") {
  const repo = flag("--cwd"), branch = flag("--branch"), label = flag("--label"), target = flag("--path");
  if (!repo || !fs.existsSync(path.join(repo, ".git"))) fail("not_git_worktree", "Herdr worktree actions require a path inside a Git work tree");
  if (target && fs.existsSync(target)) fail("worktree_create_failed", "fatal: '" + target + "' already exists");
  if (fs.existsSync(path.join(dir, "worktree-denied"))) fail("worktree_create_failed", "fatal: could not create work tree dir: Permission denied");
  const pane = label + "-pane", workspace = "ws-" + label;
  fs.mkdirSync(target, { recursive: true });
  panes[pane] = { workspace, cwd: target }; save("panes.json", panes);
  const trees = load("worktrees.json", []); trees.push({ branch, path: target, repo, workspace, pane }); save("worktrees.json", trees);
  if (fs.existsSync(path.join(dir, "worktree-lost-reply"))) { fs.rmSync(path.join(dir, "worktree-lost-reply")); process.stderr.write("Connection to mini closed by remote host.\n"); process.exit(255); }
  out({ type: "worktree_created", root_pane: { pane_id: pane, workspace_id: workspace, cwd: target }, workspace: { workspace_id: workspace, label }, worktree: { branch, path: target } });
}
else if (a0 === "worktree" && a1 === "list") out({ type: "worktree_list", worktrees: load("worktrees.json", []) });
else if (a0 === "notification" && a1 === "show") { log("notifications.jsonl", argv.slice(2)); out({ outcome: "shown" }); }
else fail("unexpected", "unexpected fake Herdr operation " + argv.join(" "), 9);
`;

const SSH = String.raw`#!/usr/bin/env node
const fs = require("node:fs"), path = require("node:path"), { spawnSync } = require("node:child_process");
const root = process.env.FAKE_REMOTE_ROOT;
const argv = process.argv.slice(2);
let index = 0;
while (index < argv.length && argv[index].startsWith("-")) { index += ["-o", "-p", "-i", "-l"].includes(argv[index]) ? 2 : 1; }
const target = argv[index];
const command = argv.slice(argv[index + 1] === "--" ? index + 2 : index + 1).join(" ");
const machine = JSON.parse(fs.readFileSync(path.join(root, "machines.json"), "utf8")).find((m) => m.target === target);
if (!machine) { process.stderr.write("ssh: Could not resolve hostname " + target + "\n"); process.exit(255); }
const hostDir = path.join(root, "h", machine.label);
fs.appendFileSync(path.join(hostDir, "ssh.jsonl"), JSON.stringify(command) + "\n");
if (fs.existsSync(path.join(hostDir, "down"))) { process.stderr.write("ssh: connect to host " + target + " port 22: Operation timed out\n"); process.exit(255); }
if (fs.existsSync(path.join(hostDir, "auth-denied"))) { process.stderr.write(target + ": Permission denied (publickey).\n"); process.exit(255); }
if (fs.existsSync(path.join(hostDir, "stderr-flood"))) {
  const block = Buffer.alloc(64 * 1024, "x");
  for (let written = 0; written < 17 * 1024 * 1024; written += block.length) fs.writeSync(2, block);
  process.exit(19);
}
const env = { PATH: path.dirname(process.execPath) + ":" + path.join(root, "bin") + ":/usr/bin:/bin", HOME: path.join(hostDir, "home"), FAKE_REMOTE_ROOT: root, FAKE_REMOTE_HOST: machine.label };
const result = spawnSync("/bin/sh", ["-c", command], { env, encoding: "utf8", input: "" });
process.stdout.write(result.stdout); process.stderr.write(result.stderr); process.exit(result.status ?? 255);
`;

export function createFakeRemote(cliPath) {
  // A short root keeps every host's api.sock under the 104-byte sun_path limit,
  // even when TMPDIR is a long per-user or in-repository directory.
  const root = fs.mkdtempSync("/tmp/hcr-");
  const bin = path.join(root, "bin");
  fs.mkdirSync(bin);
  fs.writeFileSync(path.join(bin, "herdr"), HERDR, { mode: 0o755 });
  fs.writeFileSync(path.join(bin, "ssh"), SSH, { mode: 0o755 });
  fs.writeFileSync(path.join(root, "machines.json"), "[]");
  const hostDir = (host) => { const dir = path.join(root, "h", host); fs.mkdirSync(dir, { recursive: true }); return dir; };
  const read = (host, name, fallback) => { const file = path.join(hostDir(host), name); return fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, "utf8")) : fallback; };
  const write = (host, name, value) => fs.writeFileSync(path.join(hostDir(host), name), JSON.stringify(value));
  const lines = (host, name) => { const file = path.join(hostDir(host), name); return fs.existsSync(file) ? fs.readFileSync(file, "utf8").trim().split("\n").filter(Boolean).map((line) => JSON.parse(line)) : []; };
  const home = (host) => { const dir = path.join(hostDir(host), "home"); fs.mkdirSync(dir, { recursive: true }); return dir; };
  const baseEnv = { ...process.env, FAKE_REMOTE_ROOT: root, PATH: `${bin}${path.delimiter}${process.env.PATH}` };
  // Stops for the processes running out of this root, run before the root is removed:
  // removing it first raced a daemon still writing there (ENOTEMPTY) and left it running.
  const owned = [];
  delete baseEnv.HERDR_SOCKET_PATH; delete baseEnv.HERDR_BIN_PATH; delete baseEnv.HCOORD_HOME; delete baseEnv.HCOORD_REMOTE_HOME; delete baseEnv.HERDR_PANE_ID;
  return {
    root, bin,
    env: (host = "local", extra = {}) => ({ ...baseEnv, HOME: home(host), HCOORD_HOME: path.join(home(host), ".hcoord"), ...extra }),
    home,
    addMachine(label, target = label) {
      const list = JSON.parse(fs.readFileSync(path.join(root, "machines.json"), "utf8"));
      list.push({ id: `id-${label}`, label, target, session: "hcoord-test", enabled: true, selected: false });
      fs.writeFileSync(path.join(root, "machines.json"), JSON.stringify(list));
      hostDir(label);
    },
    /** Installs the built hcoord as the remote shim the installer writes. */
    installHcoord(host) {
      const dir = path.join(home(host), ".hcoord", "bin");
      fs.mkdirSync(dir, { recursive: true });
      fs.writeFileSync(path.join(dir, "hcoord"), `#!/bin/sh\nexec "${process.execPath}" "${cliPath}" "$@"\n`, { mode: 0o755 });
    },
    addAgent(host, pane, agent) {
      const agents = read(host, "agents.json", {}); agents[pane] = { kind: "claude", status: "idle", ready: true, ...agent }; write(host, "agents.json", agents);
      const panes = read(host, "panes.json", {}); panes[pane] ??= { workspace: `ws-${agent.name}`, cwd: agent.cwd ?? home(host) }; write(host, "panes.json", panes);
    },
    removeAgent(host, pane) { const agents = read(host, "agents.json", {}); delete agents[pane]; write(host, "agents.json", agents); const panes = read(host, "panes.json", {}); delete panes[pane]; write(host, "panes.json", panes); },
    setAgent(host, pane, patch) { const agents = read(host, "agents.json", {}); agents[pane] = { ...agents[pane], ...patch }; write(host, "agents.json", agents); },
    prompts: (host) => lines(host, "prompts.jsonl"),
    calls: (host) => lines(host, "calls.jsonl"),
    sshCommands: (host) => lines(host, "ssh.jsonl"),
    notifications: (host = "local") => lines(host, "notifications.jsonl"),
    worktrees: (host) => read(host, "worktrees.json", []),
    pane: (host, pane) => read(host, "panes.json", {})[pane],
    /** Pane tokens as a writer left them, for a pane whose tokens predate the daemon's current contract. */
    setPaneTokens(host, pane, tokens) { const panes = read(host, "panes.json", {}); panes[pane].tokens = tokens; write(host, "panes.json", panes); },
    /** What a Herdr server restart does to a pane: its tokens are gone and its terminal is new. */
    restartServer(host) {
      const panes = read(host, "panes.json", {}); for (const pane of Object.values(panes)) delete pane.tokens; write(host, "panes.json", panes);
      const agents = read(host, "agents.json", {}); for (const agent of Object.values(agents)) agent.instance = `${agent.instance}-restarted`; write(host, "agents.json", agents);
    },
    flag(host, name, on = true) { const file = path.join(hostDir(host), name); if (on) fs.writeFileSync(file, "1"); else fs.rmSync(file, { force: true }); },
    /** Registers a stop that `cleanup` runs before it removes the root. */
    own(stop) { owned.push(stop); },
    async cleanup() {
      const stopped = await Promise.allSettled(owned.map((stop) => stop()));
      fs.rmSync(root, { recursive: true, force: true });
      const failed = stopped.find((result) => result.status === "rejected");
      if (failed) throw failed.reason;
    },
  };
}
