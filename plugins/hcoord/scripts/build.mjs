import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";

const root = path.resolve(import.meta.dirname, "..");
const dist = path.join(root, "dist");
const staging = path.join(root, "dist.staging");
const previous = path.join(root, "dist.previous");
const compiler = path.join(root, "node_modules", "typescript", "bin", "tsc");

if (!fs.existsSync(compiler)) {
  console.error("hcoord build requires installed dependencies; run npm install or pnpm install in the checkout");
  process.exit(1);
}
fs.rmSync(staging, { recursive: true, force: true });
const compiled = spawnSync(process.execPath, [compiler, "-p", "tsconfig.json", "--outDir", staging], { cwd: root, stdio: "inherit" });
if (compiled.status !== 0) {
  fs.rmSync(staging, { recursive: true, force: true });
  console.error(`hcoord build: tsc exited ${compiled.status ?? "without a status"}; dist was not replaced`);
  process.exit(compiled.status ?? 1);
}
const probe = spawnSync(process.execPath, ["-e", "require(process.argv[1])", path.join(staging, "hcoord", "cli.js")], { cwd: root, encoding: "utf8" });
if (probe.status !== 0) {
  fs.rmSync(staging, { recursive: true, force: true });
  console.error(`hcoord build: compiled CLI does not load (${(probe.stderr || probe.stdout).trim()})`);
  process.exit(1);
}
fs.rmSync(previous, { recursive: true, force: true });
if (fs.existsSync(dist)) fs.renameSync(dist, previous);
fs.renameSync(staging, dist);
fs.rmSync(previous, { recursive: true, force: true });
