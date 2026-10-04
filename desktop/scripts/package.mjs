// Packages the release app for the system this runs on: the Electron host
// with the release daemon, the hide CLI, the hook and device helpers, the
// pinned Herdr in its resources folder, archived beside a SHA-256
// checksum (the Electron release app PRD, D-03 and D-08).
//
// macOS builds hide.app, ad-hoc signed, as a zip. Windows x64 builds an
// unsigned folder as a zip and Linux x64 an unsigned folder as a tar.gz
// (operator decision 2026-10-03: no signing outside macOS for now). Each is
// built on its own system, because the release binaries come from that
// system's cargo build; nothing is cross-built.
//
// The CLI finds `hided` beside its own file and the daemon offers the device
// helper from its own directory, so all of them ship flat in the resources
// folder. A binary that is missing or cannot run stops the build here, by
// name, before any app exists (B12); nothing is substituted.

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

// Windows starts pnpm's `.cmd` shim only through a shell; the arguments here
// are fixed words, so the shell has nothing to reinterpret.
const pnpm = (args) => run("pnpm", args, { stdio: "inherit", shell: process.platform === "win32" });

/**
 * What differs per system. `arch` is the one architecture the pinned Herdr
 * asset for that system serves (`contracts/herdr-bundle.json`), so another
 * machine is refused rather than given a Herdr it cannot run. `herdr` fetches
 * the verified asset and answers its executable and the files that must sit
 * beside it.
 */
const SYSTEMS = {
  darwin: {
    label: "macos",
    arch: "arm64",
    herdr: () => ({ executable: run("zsh", ["scripts/fetch-herdr-runtime.sh"]), companions: [] }),
  },
  win32: {
    label: "windows",
    arch: "x64",
    // The Windows asset is a zip: herdr.exe finds its ConPTY runtime in the
    // `conpty` folder beside it, and the zip carries that runtime's notices.
    // Every entry ships as upstream laid it out.
    herdr: () => {
      const executable = run("pwsh", ["-NoProfile", "-File", "scripts/fetch-herdr-runtime.ps1"]);
      const folder = path.dirname(executable);
      const companions = fs.readdirSync(folder).filter((entry) => entry !== path.basename(executable)).map((entry) => path.join(folder, entry));
      return { executable, companions };
    },
  },
  linux: {
    label: "linux",
    arch: "x64",
    herdr: () => ({ executable: run("zsh", ["scripts/fetch-herdr-runtime.sh", "--platform", "linux-x86_64"]), companions: [] }),
  },
};

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

const platform = process.platform;
const system = SYSTEMS[platform];
if (!system) throw new Error(`hide has no package for ${platform}`);
const arch = process.arch;
if (arch !== system.arch) throw new Error(`the pinned Herdr serves ${system.label} ${system.arch} only, and this machine is ${arch}`);
const helperArch = { arm64: "aarch64", x64: "x86_64" }[arch];
const exe = platform === "win32" ? ".exe" : "";
const version = resolveVersion();

// A failed run leaves no app or archive behind, stale or otherwise (B12).
const packaged = path.join(out, `hide-${platform}-${arch}`);
fs.rmSync(packaged, { recursive: true, force: true });
fs.mkdirSync(out, { recursive: true });
const archivePattern = new RegExp(`^hide-v.*-${system.label}-.*\\.(zip|tar\\.gz)(\\.sha256)?$`);
for (const entry of fs.readdirSync(out)) {
  if (archivePattern.test(entry)) fs.rmSync(path.join(out, entry));
}

// Release hided embeds web/dist, so the web shell is built first; the cargo
// wrapper reuses the machine's toolchain and keeps output in this worktree.
pnpm(["--dir", "web", "build"]);
run("bash", ["scripts/verify-cargo.sh", "release"], { stdio: "inherit" });
const herdr = system.herdr();

const release = path.join(repo, "target", "release");
const shipped = [
  [`hided${exe}`, path.join(release, `hided${exe}`)],
  [`hide${exe}`, path.join(release, `hide${exe}`)],
  [`hide-agent-hooks${exe}`, path.join(release, `hide-agent-hooks${exe}`)],
  [`hide-host-helper-${system.label}-${helperArch}${exe}`, path.join(release, `hide-host-helper${exe}`)],
  [`herdr${exe}`, herdr.executable],
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
  throw new Error(`cannot package hide: not an executable file: ${missing.map(([name, source]) => `${name} (${source})`).join(", ")}`);
}

// Staged under out/ so a name inside the package can differ from the build's.
const staged = path.join(out, "resources");
fs.rmSync(staged, { recursive: true, force: true });
fs.mkdirSync(staged, { recursive: true });
const extraResource = shipped.map(([name, source]) => {
  const target = path.join(staged, name);
  fs.copyFileSync(source, target);
  fs.chmodSync(target, 0o755);
  return target;
});
for (const companion of herdr.companions) {
  const target = path.join(staged, path.basename(companion));
  fs.cpSync(companion, target, { recursive: true });
  extraResource.push(target);
}
const notices = path.join(resources, "THIRD_PARTY_NOTICES");
if (!fs.existsSync(notices)) throw new Error(`third-party notices are missing: ${notices}`);
extraResource.push(notices);

const [outputDir] = await packager({
  dir: root,
  out,
  name: "hide",
  appBundleId: "me.grab.hide.desktop",
  appVersion: version,
  // macOS's CFBundleVersion and Windows' file version must be dotted
  // integers; the numeric prefix is that.
  buildVersion: /^[0-9.]+/.exec(version)[0],
  // No extension: the packager takes hide.icns on macOS and hide.ico on
  // Windows; a Linux executable carries no icon.
  icon: path.join(resources, "hide"),
  platform,
  arch,
  overwrite: true,
  asar: true,
  prune: true,
  extraResource,
  // Only the bundles and the status page ship; sources, tests and tooling do not.
  ignore: (file) => file !== "" && file !== "/package.json" && !file.startsWith("/dist"),
});

// The packager answers with the per-platform directory. On macOS the bundle
// is inside it and is what ships; elsewhere the directory itself ships, with
// the Electron executable at its top and the resources folder beside it.
const bundle = platform === "darwin" ? path.join(outputDir, "hide.app") : outputDir;
const bundledResources = platform === "darwin" ? path.join(bundle, "Contents", "Resources") : path.join(bundle, "resources");
for (const [name] of shipped) {
  const file = path.join(bundledResources, name);
  fs.accessSync(file, fs.constants.X_OK);
  if (!fs.statSync(file).isFile()) throw new Error(`${name} did not land as a file in ${bundledResources}`);
}
for (const companion of herdr.companions) {
  const name = path.basename(companion);
  if (!fs.existsSync(path.join(bundledResources, name))) throw new Error(`Herdr's ${name} did not land in ${bundledResources}`);
}
if (platform === "darwin") {
  run("/usr/bin/codesign", ["--force", "--deep", "--sign", "-", "--timestamp=none", bundle], { stdio: "inherit" });
  run("/usr/bin/codesign", ["--verify", "--deep", "--strict", "--verbose=2", bundle], { stdio: "inherit" });
}

// Each archive holds one folder: hide.app, or the packaged directory.
const archiveName = `hide-v${version}-${system.label}-${arch}.${platform === "linux" ? "tar.gz" : "zip"}`;
const archive = path.join(out, archiveName);
fs.rmSync(archive, { force: true });
if (platform === "darwin") {
  run("/usr/bin/ditto", ["-c", "-k", "--keepParent", bundle, archive]);
} else if (platform === "win32") {
  // The system's own tar (bsdtar, in every Windows since 10 1803) writes a
  // zip when the name ends in .zip; Git's GNU tar earlier on PATH cannot.
  if (!process.env.SystemRoot) throw new Error("SystemRoot is unset, so the system's tar.exe cannot be found");
  run(path.join(process.env.SystemRoot, "System32", "tar.exe"), ["-a", "-c", "-f", archive, "-C", out, path.basename(bundle)]);
} else {
  // A tar keeps the executable bits a zip extracted by most tools would drop.
  run("tar", ["-czf", archive, "-C", out, path.basename(bundle)]);
}
const digest = createHash("sha256").update(fs.readFileSync(archive)).digest("hex");
fs.writeFileSync(`${archive}.sha256`, `${digest}  ${archiveName}\n`);

console.log(`bundle=${bundle}`);
console.log(`hide_version=${version}`);
console.log(`archive=${archive}`);
console.log(`archive_sha256=${digest}`);
