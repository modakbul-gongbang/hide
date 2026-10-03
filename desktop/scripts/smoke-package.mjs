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
// embeds, installs the CLI and both agent hooks, and is replaced by a second
// package fixture with a changed daemon hash, before `hide stop` ends it. Every process runs with a private home
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
import { setTimeout as delay } from "node:timers/promises";

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

// Windows TEMP may use an 8.3 alias. Every fixture path, including the
// expected hook command, starts from the native canonical spelling.
const scratch = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "hide-smoke-")));
const unpacked = path.join(scratch, "unpacked");
const home = path.join(scratch, "home");
const state = path.join(scratch, "state");

/** Each fixture starts from the actual archive, as an operator's extraction does. */
function unpackInto(destination) {
  fs.mkdirSync(destination);
  if (process.platform === "win32") {
    execFileSync("pwsh", ["-NoProfile", "-Command", "Expand-Archive -LiteralPath $env:SMOKE_ARCHIVE -DestinationPath $env:SMOKE_DESTINATION"], {
      env: { ...process.env, SMOKE_ARCHIVE: path.resolve(archive), SMOKE_DESTINATION: destination },
      stdio: "inherit",
    });
  } else {
    execFileSync("tar", ["-xzf", path.resolve(archive), "-C", destination], { stdio: "inherit" });
  }
  const top = fs.readdirSync(destination);
  if (top.length !== 1) throw new Error(`the archive holds ${top.length} top-level entries, not one folder: ${top.join(", ")}`);
  return top[0];
}

try {
  fs.mkdirSync(home);
  check("unpack", () => unpackInto(unpacked));

  const app = path.join(unpacked, fs.readdirSync(unpacked)[0]);
  let resources = path.join(app, "resources");
  const bundled = (name) => path.join(resources, `${name}${exe}`);
  // Cleanup always uses the known complete first package, even if preparing
  // or connecting the upgrade fails before its CLI can run.
  const controlCli = bundled("hide");
  const helper = `hide-host-helper-${label}-x86_64`;

  const electron = path.join(app, `hide${exe}`);
  const hcoordCli = path.join(resources, "hcoord", "dist", "hcoord", "cli.js");

  check("bundled files", () => {
    const expected = [path.join(resources, "app.asar"), electron, ...["hide", "hided", "hide-agent-hooks", helper, "herdr"].map(bundled), hcoordCli];
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
    CODEX_HOME: path.join(home, ".codex"),
    CLAUDE_CONFIG_DIR: path.join(home, ".claude"),
    HCOORD_HOME: path.join(home, ".hide", "hcoord"),
    HERDR_SOCKET_PATH: path.join(scratch, "herdr.sock"),
    HIDE_STATE_DIR: state,
    HIDE_TAILSCALE_BIN: path.join(home, "no-tailscale"),
    // What the desktop host hands every `hide` child it starts.
    HERDR_BIN_PATH: bundled("herdr"),
  });
  delete isolated.XDG_STATE_HOME;

  // Both runtimes are configured, with an unrelated hook we must preserve.
  for (const [folder, file] of [[".claude", "settings.json"], [".codex", "hooks.json"]]) {
    fs.mkdirSync(path.join(home, folder));
    fs.writeFileSync(path.join(home, folder, file), JSON.stringify({
      smokeForeign: "preserve",
      hooks: { SessionStart: [{ hooks: [{ type: "command", command: "echo smoke-foreign" }] }] },
    }));
  }

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

  // Prepare the complete second package before either daemon holds an
  // executable open. PE and ELF permit an overlay after the image; this
  // changes the production build hash without a second release build.
  const upgraded = path.join(scratch, "new 한글 package");
  check("prepare a second complete package", () => {
    const secondUnpacked = path.join(scratch, "upgrade-unpacked");
    const folder = unpackInto(secondUnpacked);
    fs.renameSync(path.join(secondUnpacked, folder), upgraded);
    for (const file of ["app.asar", ...["hide", "hided", "hide-agent-hooks", "herdr"].map((name) => `${name}${exe}`)]) {
      if (!fs.statSync(path.join(upgraded, "resources", file), { throwIfNoEntry: false })?.isFile()) throw new Error(`second package is missing resources/${file}`);
    }
    fs.appendFileSync(path.join(upgraded, "resources", `hided${exe}`), "\nHide package smoke next-build fixture\n");
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

  /** Wait on the completed pass, not a fixed startup delay or a live PID. */
  const log = path.join(state, "Logs", "core.jsonl");
  async function kitInstalled(after = 0) {
    const deadline = Date.now() + 25_000;
    let last = "no kit completion";
    while (Date.now() < deadline) {
      const records = (fs.existsSync(log) ? fs.readFileSync(log).subarray(after).toString("utf8") : "").split("\n");
      for (const line of records) {
        let record;
        try { record = JSON.parse(line); } catch { continue; } // the last write may be partial
        if (record.component !== "kit" || record.kind !== "apply.completed") continue;
        last = JSON.stringify(record.components);
        const parts = new Map(record.components.map((part) => [part.id, part]));
        if (["cli", "claude_code_hook", "codex_hook"].every((id) => parts.get(id)?.state === "installed")
          && parts.get("hcoord")?.state === "absent" && parts.get("hcoord")?.reason) return;
      }
      await delay(100);
    }
    throw new Error(`kit installation did not complete: ${last}`);
  }

  function installedFiles() {
    const localBin = path.join(home, ".local", "bin");
    const command = path.join(localBin, process.platform === "win32" ? "hide.cmd" : "hide");
    const shellEnv = { ...isolated, PATH: `${localBin}${path.delimiter}${isolated.PATH ?? isolated.Path ?? ""}` };
    // Windows environment names are case-insensitive; Node otherwise chooses
    // the first spelling and may silently discard our temporary PATH.
    for (const key of Object.keys(shellEnv)) if (key.toLowerCase() === "path" && key !== "PATH") delete shellEnv[key];
    const resolved = process.platform === "win32"
      ? output("pwsh", ["-NoProfile", "-Command", "(Get-Command hide -CommandType Application).Source"], shellEnv)
      : output("/bin/sh", ["-c", "command -v hide"], shellEnv);
    const resolvedId = fs.statSync(resolved, { bigint: true });
    const commandId = fs.statSync(command, { bigint: true });
    if (resolvedId.dev !== commandId.dev || resolvedId.ino !== commandId.ino) throw new Error(`new shell resolved ${resolved}, expected ${command}`);
    const answer = process.platform === "win32"
      ? output("pwsh", ["-NoProfile", "-Command", "hide status --json"], shellEnv)
      : output("/bin/sh", ["-c", "hide status --json"], shellEnv);
    if (JSON.parse(answer).running !== true) throw new Error(`installed command answered ${answer}`);
    const destination = process.platform === "win32" ? path.join(localBin, ".hide-kit") : command;
    const expected = process.platform === "win32" ? resources : bundled("hide");
    // A Windows junction can resolve to the long spelling while TEMP keeps
    // its 8.3 spelling. Compare the objects, not equivalent path strings.
    const actualId = fs.statSync(destination, { bigint: true });
    const expectedId = fs.statSync(expected, { bigint: true });
    if (actualId.dev !== expectedId.dev || actualId.ino !== expectedId.ino) {
      throw new Error(`the command leads to ${fs.realpathSync(destination)}, expected ${fs.realpathSync(expected)} (file identities differ)`);
    }
    for (const [folder, file] of [[".claude", "settings.json"], [".codex", "hooks.json"]]) {
      const config = JSON.parse(fs.readFileSync(path.join(home, folder, file), "utf8"));
      const hooks = Object.values(config.hooks).flat().flatMap((group) => group.hooks ?? []);
      if (config.smokeForeign !== "preserve" || !hooks.some((hook) => hook.command === "echo smoke-foreign")) throw new Error(`${folder} lost unrelated settings`);
      const owned = hooks.filter((hook) => JSON.stringify(hook).includes("--source hide-subagents@"));
      if (owned.length === 0 || owned.some((hook) => ![hook.command, ...(hook.args ?? [])].join(" ").includes(bundled("hide-agent-hooks")))) throw new Error(`${folder} hook paths are missing or stale`);
      if (process.platform === "win32" && owned.some((hook) => folder === ".claude"
        ? hook.command !== "powershell.exe" || !hook.args?.includes("-Command")
        : !hook.command.startsWith("if (Test-Path -LiteralPath"))) throw new Error(`${folder} hooks do not use PowerShell`);
    }
    if (fs.existsSync(path.join(home, ".hide", "hcoord"))) throw new Error("hcoord was installed on an unsupported system");
    return "command resolves in a fresh shell; both hooks installed; foreign settings preserved; hcoord absent";
  }

  let connected = null;
  try {
    connected = hide(["connect"]);
    check("hide connect starts the bundled daemon", () => {
      if (connected.ok !== true) throw new Error(JSON.stringify(connected));
      return `pid ${connected.pid}, port ${connected.port}`;
    });
    await kitInstalled();
    check("first launch installs the kit", installedFiles);
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
    resources = path.join(upgraded, "resources");
    const beforeUpgrade = fs.statSync(log).size;
    const previous = connected;
    connected = hide(["connect"]);
    check("a newer package replaces the running daemon", () => {
      if (connected.ok !== true || connected.pid === previous.pid) throw new Error("new package did not replace the old daemon");
      return "new daemon PID";
    });
    const nextHealth = await (await get(`http://127.0.0.1:${connected.port}`, "/health")).json();
    check("the new daemon answers with the new build hash", () => {
      const expected = createHash("sha256").update(fs.readFileSync(bundled("hided"))).digest("hex");
      if (nextHealth.pid !== connected.pid || nextHealth.build !== expected || nextHealth.build === health.build) throw new Error("the second package's daemon did not answer");
    });
    await kitInstalled(beforeUpgrade);
    check("upgrade moves the command and both hooks to the new package", installedFiles);
    check("reconnecting the same package keeps its daemon", () => {
      if (hide(["connect"]).pid !== connected.pid) throw new Error("same-build connect replaced the daemon");
    });

    // A plain development-shaped copy still cannot replace a packaged daemon.
    const standalone = path.join(scratch, "target", "release");
    fs.mkdirSync(standalone, { recursive: true });
    for (const name of ["hide", "hided"]) fs.copyFileSync(path.join(app, "resources", `${name}${exe}`), path.join(standalone, `${name}${exe}`));
    check("a standalone build refuses replacement", () => {
      let refused = false;
      try { output(path.join(standalone, `hide${exe}`), ["connect"], isolated); }
      catch (error) { refused = error.status !== 0 && String(error.stdout).includes("other_build"); }
      if (!refused || hide(["status", "--json"]).pid !== connected.pid) throw new Error("standalone connect did not leave the packaged daemon alone");
    });
  } finally {
    // Asked whatever happened above: a daemon a failed connect left running is
    // this run's too, and stopping none is not an error. A failure here is
    // reported, after the scratch folder is gone, beside any failure above.
    try {
      execFileSync(controlCli, ["stop"], { env: isolated, stdio: "inherit", timeout: 30_000 });
      if (connected?.ok === true) {
        check("hide stop ends it", () => {
          const status = JSON.parse(output(controlCli, ["status", "--json"], isolated));
          if (status.running !== false) throw new Error(JSON.stringify(status));
        });
      }
    } catch (error) {
      console.error(`hide stop failed: ${error instanceof Error ? error.message : String(error)}`);
      process.exitCode = 1;
    }
  }
} catch (error) {
  console.error(error);
  process.exitCode = 1;
} finally {
  try {
    fs.rmSync(scratch, { recursive: true, force: true, maxRetries: 5, retryDelay: 500 });
  } catch (error) {
    console.error(`scratch cleanup failed: ${error instanceof Error ? error.message : String(error)}`);
    process.exitCode = 1;
  }
}
