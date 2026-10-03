// A launchctl stand-in so no test touches the real launchd domain. It records argv
// and answers from a state file, with the refusals launchd gives (measured 2026-09-26).
import fs from "node:fs";
import path from "node:path";

export const FAKE_LAUNCHCTL_SOURCE = `#!/usr/bin/env node
const fs = require("node:fs");
const argv = process.argv.slice(2);
if (process.env.LAUNCHCTL_FAKE_LOG) fs.appendFileSync(process.env.LAUNCHCTL_FAKE_LOG, JSON.stringify(argv) + "\\n");
const stateFile = process.env.LAUNCHCTL_FAKE_STATE;
const state = stateFile && fs.existsSync(stateFile) ? JSON.parse(fs.readFileSync(stateFile, "utf8")) : { loaded: {} };
const save = () => stateFile && fs.writeFileSync(stateFile, JSON.stringify(state));
const label = (target) => String(target).split("/").pop();
// launchd returns from bootout while the job is still exiting; the label reads
// loaded for LAUNCHCTL_FAKE_BOOTOUT_SETTLE_PRINTS more prints (measured 2026-09-26).
if (argv[0] === "print") {
  const settling = (state.settling ?? {})[label(argv[1])];
  if (settling !== undefined) { if (settling <= 1) { delete state.settling[label(argv[1])]; delete state.loaded[label(argv[1])]; } else state.settling[label(argv[1])] = settling - 1; save(); }
  if (state.loaded[label(argv[1])]) { process.stdout.write("service loaded"); process.exit(0); }
  process.stderr.write("Could not find service \\"" + label(argv[1]) + "\\" in domain for user gui\\n"); process.exit(113);
}
// launchd refuses a missing plist and a label it already has loaded with the
// same "5: Input/output error" (measured 2026-09-26 on hcoord daemon start after stop).
if (argv[0] === "bootstrap") { const plist = argv[2]; const name = require("node:path").basename(plist, ".plist"); if (!fs.existsSync(plist) || state.loaded[name]) { process.stderr.write("Bootstrap failed: 5: Input/output error\\n"); process.exit(5); } state.loaded[name] = plist; save(); process.exit(0); }
if (argv[0] === "bootout") { const prints = Number(process.env.LAUNCHCTL_FAKE_BOOTOUT_SETTLE_PRINTS ?? 0); if (prints > 0) state.settling = { ...(state.settling ?? {}), [label(argv[1])]: prints }; else delete state.loaded[label(argv[1])]; save(); process.exit(0); }
if (argv[0] === "kickstart") { if (!state.loaded[label(argv[1])]) { process.stderr.write("Could not find service\\n"); process.exit(113); } state.kicked = (state.kicked ?? 0) + 1; save(); process.exit(0); }
process.stderr.write("Usage: launchctl <subcommand>\\n"); process.exit(64);
`;

export function installFakeLaunchctl(root) {
  const bin = path.join(root, "fake-bin");
  fs.mkdirSync(bin, { recursive: true });
  fs.writeFileSync(path.join(bin, "launchctl"), FAKE_LAUNCHCTL_SOURCE, { mode: 0o755 });
  const log = path.join(root, "launchctl-argv.log");
  const stateFile = path.join(root, "launchctl-state.json");
  return {
    bin,
    log,
    stateFile,
    env: { PATH: `${bin}${path.delimiter}${process.env.PATH ?? ""}`, LAUNCHCTL_FAKE_LOG: log, LAUNCHCTL_FAKE_STATE: stateFile },
    argv() { return fs.existsSync(log) ? fs.readFileSync(log, "utf8").trim().split("\n").filter(Boolean).map((line) => JSON.parse(line)) : []; },
    state() { return fs.existsSync(stateFile) ? JSON.parse(fs.readFileSync(stateFile, "utf8")) : { loaded: {} }; },
  };
}
