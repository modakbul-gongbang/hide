// Proves a Windows or Linux package starts, without a window, on a CI runner
// of that system: no machine of either system is at hand to try a package on
// (operator decision 2026-10-03).
//
// Usage: node desktop/scripts/smoke-package.mjs <archive>
//
// It checks the archive against its `.sha256`, unpacks it into a new
// temporary folder (Windows with Expand-Archive, as a user's Extract All
// reads a zip, rather than the tar that wrote it), and from the unpacked
// folder: Herdr reports the pinned version, the device helper and the hook
// helper start, Electron runs the bundled hcoord as Node, and the bundled
// `hide connect` starts the bundled daemon, which serves the web shell it
// embeds, before `hide stop` ends it. Every process runs with a private home
// and state folder, so none reaches a daemon or Herdr of the runner's account.
//
// macOS is refused: a daemon run from inside hide.app installs the kit, which
// bootstraps hcoord's LaunchAgent in the account's own launchd domain, and the
// macOS package already runs its Electron runtime while it is built.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const LABELS = { win32: "windows", linux: "linux" };
const label = LABELS[process.platform];
if (!label) throw new Error(`smoke-package checks Windows and Linux packages only, not ${process.platform}`);
const exe = process.platform === "win32" ? ".exe" : "";

const archive = process.argv[2];
if (!archive || process.argv.length !== 3) throw new Error("usage: node desktop/scripts/smoke-package.mjs <archive>");

/** One named check: it passes, or the run stops with its name. */
function check(name, body) {
  try {
    const detail = body();
    console.log(`ok ${name}${detail ? `: ${detail}` : ""}`);
  } catch (error) {
    throw new Error(`${name} failed: ${error instanceof Error ? error.message : String(error)}`, { cause: error });
  }
}

/** A program's stdout; a non-zero exit, a timeout or a spawn failure throws with its stderr. */
const output = (file, args, env) => execFileSync(file, args, { env, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], timeout: 30_000 }).trim();

check("checksum", () => {
  const [digest, name] = fs.readFileSync(`${archive}.sha256`, "utf8").trim().split(/\s+/);
  const actual = createHash("sha256").update(fs.readFileSync(archive)).digest("hex");
  if (name !== path.basename(archive)) throw new Error(`the checksum names ${name}, not ${path.basename(archive)}`);
  if (actual !== digest) throw new Error(`the archive's SHA-256 is ${actual}, the checksum says ${digest}`);
});

const scratch = fs.mkdtempSync(path.join(os.tmpdir(), "hide-smoke-"));
const unpacked = path.join(scratch, "unpacked");
const home = path.join(scratch, "home");
const state = path.join(scratch, "state");
fs.mkdirSync(unpacked);
fs.mkdirSync(home);

check("unpack", () => {
  if (process.platform === "win32") {
    execFileSync("pwsh", ["-NoProfile", "-Command", "Expand-Archive -LiteralPath $env:SMOKE_ARCHIVE -DestinationPath $env:SMOKE_DESTINATION"], {
      env: { ...process.env, SMOKE_ARCHIVE: path.resolve(archive), SMOKE_DESTINATION: unpacked },
      stdio: "inherit",
    });
  } else {
    execFileSync("tar", ["-xzf", path.resolve(archive), "-C", unpacked], { stdio: "inherit" });
  }
  const top = fs.readdirSync(unpacked);
  if (top.length !== 1) throw new Error(`the archive holds ${top.length} top-level entries, not one folder: ${top.join(", ")}`);
  return top[0];
});

const app = path.join(unpacked, fs.readdirSync(unpacked)[0]);
const resources = path.join(app, "resources");
const bundled = (name) => path.join(resources, `${name}${exe}`);
const helper = `hide-host-helper-${label}-x86_64`;

const electron = path.join(app, `hide${exe}`);
const hcoordCli = path.join(resources, "hcoord", "dist", "hcoord", "cli.js");

check("bundled files", () => {
  const expected = [electron, ...["hide", "hided", "hide-agent-hooks", helper, "herdr"].map(bundled), hcoordCli];
  // herdr.exe finds its ConPTY runtime in the folder beside it.
  if (process.platform === "win32") expected.push(path.join(resources, "conpty", "conpty.dll"));
  const absent = expected.filter((file) => !fs.statSync(file, { throwIfNoEntry: false })?.isFile());
  if (absent.length > 0) throw new Error(`missing: ${absent.map((file) => path.relative(app, file)).join(", ")}`);
});

// What every process below runs with: the account's places moved under the
// scratch home, and nothing inherited that names a Herdr or a Hide.
const isolated = Object.fromEntries(Object.entries(process.env).filter(([key]) => !/^(HERDR_|HIDE_)/i.test(key)));
Object.assign(isolated, {
  HOME: home,
  USERPROFILE: home,
  APPDATA: path.join(home, "AppData", "Roaming"),
  LOCALAPPDATA: path.join(home, "AppData", "Local"),
  XDG_CONFIG_HOME: path.join(home, ".config"),
  HIDE_STATE_DIR: state,
  HIDE_TAILSCALE_BIN: path.join(home, "no-tailscale"),
  // What the desktop host hands every `hide` child it starts.
  HERDR_BIN_PATH: bundled("herdr"),
});
delete isolated.XDG_STATE_HOME;

check("herdr --version", () => {
  const pinned = JSON.parse(fs.readFileSync(path.join(repo, "contracts", "herdr-bundle.json"), "utf8")).version;
  const reported = output(bundled("herdr"), ["--version"], isolated);
  if (reported.split(/\s+/).at(-1) !== pinned) throw new Error(`reports '${reported}', the pin is ${pinned}`);
  return reported;
});

check(`${helper} --version`, () => {
  const reported = output(bundled(helper), ["--version"], isolated);
  if (!reported.startsWith("hide-host-helper ")) throw new Error(`answered '${reported}'`);
  return reported;
});

check("hide-agent-hooks doctor --json", () => {
  JSON.parse(output(bundled("hide-agent-hooks"), ["doctor", "--json"], isolated));
});

check("Electron runs the bundled hcoord as Node", () => {
  const answer = JSON.parse(output(electron, [hcoordCli, "version", "--json"], { ...isolated, ELECTRON_RUN_AS_NODE: "1" }));
  if (answer?.ok !== true || typeof answer?.value?.hcoordVersion !== "string") throw new Error(`answered ${JSON.stringify(answer)}`);
  return answer.value.hcoordVersion;
});

/**
 * The bundled `hide`'s one JSON line, also when it exits non-zero (a refused
 * connect prints its reason). A `hide` that had to be killed answered nothing:
 * its output staying open past the bound is the failure, whatever it printed.
 */
function hide(args) {
  try {
    return JSON.parse(output(bundled("hide"), args, isolated));
  } catch (error) {
    const exited = typeof error?.status === "number" && error.signal == null && error.code !== "ETIMEDOUT";
    if (exited && typeof error.stdout === "string" && error.stdout.trim().startsWith("{")) return JSON.parse(error.stdout);
    throw new Error(`hide ${args.join(" ")}: ${error?.stderr || error?.message || error}`);
  }
}

/** A GET to the daemon, its failure named by the path asked. */
const get = (origin, route) =>
  fetch(`${origin}${route}`, { signal: AbortSignal.timeout(10_000) }).catch((error) => {
    throw new Error(`GET ${route}: ${error.message}`);
  });

let connected = null;
try {
  connected = hide(["connect"]);
  check("hide connect starts the bundled daemon", () => {
    if (connected.ok !== true) throw new Error(JSON.stringify(connected));
    return `pid ${connected.pid}, port ${connected.port}`;
  });
  const origin = `http://127.0.0.1:${connected.port}`;
  const health = await (await get(origin, "/health")).json();
  check("the daemon answers /health", () => {
    if (health.pid !== connected.pid) throw new Error(`answered ${JSON.stringify(health)}`);
    return `build ${health.build}`;
  });
  const page = await get(origin, "/");
  const html = await page.text();
  const script = /<script[^>]+src="(\/assets\/[^"]+\.js)"/.exec(html)?.[1];
  const asset = script ? await get(origin, script) : null;
  check("the daemon serves the web shell it embeds", () => {
    if (page.status !== 200 || !html.includes('<div id="root">')) throw new Error(`/ answered ${page.status} without the shell's root`);
    if (!script) throw new Error("the shell's page names no script under /assets");
    if (asset.status !== 200) throw new Error(`${script} answered ${asset.status}`);
    return script;
  });
} finally {
  // Asked whatever happened above: a daemon a failed connect left running is
  // this run's too, and stopping none is not an error. A failure here is
  // reported, after the scratch folder is gone, beside any failure above.
  try {
    execFileSync(bundled("hide"), ["stop"], { env: isolated, stdio: "inherit", timeout: 30_000 });
    if (connected?.ok === true) {
      check("hide stop ends it", () => {
        const status = hide(["status", "--json"]);
        if (status.running !== false) throw new Error(JSON.stringify(status));
      });
    }
  } catch (error) {
    console.error(`hide stop failed: ${error instanceof Error ? error.message : String(error)}`);
    process.exitCode = 1;
  } finally {
    fs.rmSync(scratch, { recursive: true, force: true, maxRetries: 5, retryDelay: 500 });
  }
}
