#!/usr/bin/env node
// Installs the fixed command path HQs use over SSH. The checkout remains the
// source of truth, so updating it and running this command moves the shim.

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
execFileSync(process.execPath, [path.join(root, "scripts", "build.mjs")], { cwd: root, stdio: "inherit" });
const home = process.env.HCOORD_HOME || path.join(os.homedir(), ".hcoord");
const directory = path.join(home, "bin");
const target = path.join(directory, "hcoord");
const temporary = `${target}.${process.pid}.tmp`;
const quote = (value) => `'${value.replaceAll("'", `'\\''`)}'`;
fs.mkdirSync(directory, { recursive: true, mode: 0o700 });
fs.writeFileSync(temporary, `#!/bin/sh\nexec ${quote(process.execPath)} ${quote(path.join(root, "dist", "hcoord", "cli.js"))} "$@"\n`, { mode: 0o700 });
fs.renameSync(temporary, target);
process.stdout.write(`${target}\n`);
