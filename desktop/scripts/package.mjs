// Packages the release hide.app: the Electron host with the release daemon,
// the hide CLI, the hook and device helpers, the pinned Herdr binary, the
// labels plugin and hcoord in its Contents/Resources, ad-hoc signed, zipped beside a SHA-256 checksum
// (the Electron release app PRD, D-03 and D-08).
//
// The CLI finds `hided` beside its own file and the daemon offers the device
// helper from its own directory, so all of them ship flat in Resources. A
// binary that is missing or cannot run stops the build here, by name, before
// any app exists (B12); nothing is substituted.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { packager } from "@electron/packager";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repo = path.resolve(root, "..");
const out = path.join(root, "out");
const resources = path.join(root, "resources");

/** Runs a tool in the repository; its stdout is the answer, unless `stdio` shows it instead. */
const run = (file, args, options = {}) => execFileSync(file, args, { cwd: repo, stdio: ["ignore", "pipe", "inherit"], encoding: "utf8", ...options })?.trim() ?? "";

// The app version comes from the release tag (release.yml passes
// HIDE_VERSION) or, for a local build, from the nearest tag; a placeholder
// would ship an app that reports a version nothing was released under.
function resolveVersion() {
  let version = process.env.HIDE_VERSION;
  if (!version) {
    try {
      version = run("git", ["describe", "--tags", "--match", "v[0-9]*", "--dirty"]);
    } catch {
      version = "";
    }
  }
  version = version.replace(/^v/, "");
  if (!/^[0-9]/.test(version)) throw new Error(`HIDE_VERSION is unset and no v<version> tag describes this tree (got '${version || "nothing"}')`);
  return version;
}

const version = resolveVersion();
const arch = process.arch;
const helperArch = { arm64: "aarch64", x64: "x86_64" }[arch];
if (!helperArch) throw new Error(`hide-host-helper has no package name for the ${arch} architecture`);

// A failed run leaves no app or archive behind, stale or otherwise (B12).
fs.rmSync(path.join(out, `hide-darwin-${arch}`), { recursive: true, force: true });
fs.mkdirSync(out, { recursive: true });
for (const entry of fs.readdirSync(out)) {
  if (/^hide-v.*-macos-.*\.zip(\.sha256)?$/.test(entry)) fs.rmSync(path.join(out, entry));
}

// Release hided embeds web/dist, so the web shell is built first; the cargo
// wrapper reuses the machine's toolchain and keeps output in this worktree.
run("pnpm", ["--dir", "web", "build"], { stdio: "inherit" });
run("pnpm", ["--dir", "plugins/hcoord", "build"], { stdio: "inherit" });
run("bash", ["scripts/verify-cargo.sh", "release"], { stdio: "inherit" });
const herdr = run("zsh", ["scripts/fetch-herdr-runtime.sh"]);

const release = path.join(repo, "target", "release");
const shipped = [
  ["hided", path.join(release, "hided")],
  ["hide", path.join(release, "hide")],
  ["hide-agent-hooks", path.join(release, "hide-agent-hooks")],
  [`hide-host-helper-macos-${helperArch}`, path.join(release, "hide-host-helper")],
  ["herdr", herdr],
];
const missing = shipped.filter(([, source]) => {
  try {
    fs.accessSync(source, fs.constants.X_OK);
    return !fs.statSync(source).isFile();
  } catch {
    return true;
  }
});
if (missing.length > 0) {
  throw new Error(`cannot package hide.app: not an executable file: ${missing.map(([name, source]) => `${name} (${source})`).join(", ")}`);
}

// Staged under out/ so a name inside the bundle can differ from the build's.
const staged = path.join(out, "resources");
fs.rmSync(staged, { recursive: true, force: true });
fs.mkdirSync(staged, { recursive: true });
const extraResource = shipped.map(([name, source]) => {
  const target = path.join(staged, name);
  fs.copyFileSync(source, target);
  fs.chmodSync(target, 0o755);
  return target;
});
const hcoordBuild = path.join(repo, "plugins", "hcoord", "dist");
if (!fs.existsSync(path.join(hcoordBuild, "hcoord", "cli.js"))) {
  throw new Error(`cannot package hide.app: built hcoord CLI is missing (${path.join(hcoordBuild, "hcoord", "cli.js")})`);
}
const stagedHcoord = path.join(staged, "hcoord");
fs.mkdirSync(stagedHcoord, { recursive: true });
fs.cpSync(hcoordBuild, path.join(stagedHcoord, "dist"), { recursive: true });
extraResource.push(stagedHcoord);
const notices = path.join(resources, "THIRD_PARTY_NOTICES");
if (!fs.existsSync(notices)) throw new Error(`third-party notices are missing: ${notices}`);
extraResource.push(notices);

const [outputDir] = await packager({
  dir: root,
  out,
  name: "hide",
  appBundleId: "me.grab.hide.desktop",
  appVersion: version,
  // CFBundleVersion must be a dotted integer; the numeric prefix is that.
  buildVersion: /^[0-9.]+/.exec(version)[0],
  icon: path.join(resources, "hide.icns"),
  platform: "darwin",
  arch,
  overwrite: true,
  asar: true,
  prune: true,
  extraResource,
  // Only the bundles and the status page ship; sources, tests and tooling do not.
  ignore: (file) => file !== "" && file !== "/package.json" && !file.startsWith("/dist"),
});

// The packager answers with the per-platform directory; the bundle is inside it.
const appPath = path.join(outputDir, "hide.app");
const bundledResources = path.join(appPath, "Contents", "Resources");
for (const [name] of shipped) {
  const file = path.join(bundledResources, name);
  fs.accessSync(file, fs.constants.X_OK);
  if (!fs.statSync(file).isFile()) throw new Error(`${name} did not land as a file in ${bundledResources}`);
}
const bundledHcoord = path.join(bundledResources, "hcoord", "dist", "hcoord", "cli.js");
if (!fs.statSync(bundledHcoord).isFile()) throw new Error(`built hcoord CLI did not land in ${bundledResources}`);

// The packaged daemon runs in Electron's Node runtime. Execute the exact
// bundle before signing so a disabled RunAsNode fuse fails the package by
// name instead of shipping an app whose login daemon cannot start.
const appExecutable = path.join(appPath, "Contents", "MacOS", "hide");
let runAsNode;
try {
  runAsNode = run(appExecutable, [bundledHcoord, "version", "--json"], {
    env: { ...process.env, ELECTRON_RUN_AS_NODE: "1" },
    timeout: 10_000,
  });
} catch (error) {
  fs.rmSync(appPath, { recursive: true, force: true });
  throw new Error(`cannot package hide.app: Electron RunAsNode fuse did not execute bundled hcoord: ${String(error)}`);
}
let hcoordProbe;
try { hcoordProbe = JSON.parse(runAsNode); }
catch {
  fs.rmSync(appPath, { recursive: true, force: true });
  throw new Error("cannot package hide.app: Electron RunAsNode fuse returned invalid hcoord JSON");
}
if (hcoordProbe?.ok !== true || typeof hcoordProbe?.value?.hcoordVersion !== "string") {
  fs.rmSync(appPath, { recursive: true, force: true });
  throw new Error("cannot package hide.app: Electron RunAsNode fuse did not confirm the bundled hcoord version");
}

run("/usr/bin/codesign", ["--force", "--deep", "--sign", "-", "--timestamp=none", appPath], { stdio: "inherit" });
run("/usr/bin/codesign", ["--verify", "--deep", "--strict", "--verbose=2", appPath], { stdio: "inherit" });

const archiveName = `hide-v${version}-macos-${arch}.zip`;
const archive = path.join(out, archiveName);
fs.rmSync(archive, { force: true });
run("/usr/bin/ditto", ["-c", "-k", "--keepParent", appPath, archive]);
const digest = createHash("sha256").update(fs.readFileSync(archive)).digest("hex");
fs.writeFileSync(`${archive}.sha256`, `${digest}  ${archiveName}\n`);

console.log(`bundle=${appPath}`);
console.log(`hide_version=${version}`);
console.log(`archive=${archive}`);
console.log(`archive_sha256=${digest}`);
