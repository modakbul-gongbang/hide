// A device gets the same install kit as this Mac (PRD device-parity B13,
// B16, B8, D-26, B22, B23, B25): over a real SSH connection to an isolated
// sshd whose sessions get a private HOME (desktop/e2e/device-home.ts), with a
// private daemon, desktop profile and two Herdr servers, one standing in for
// the device's own.

import { expect, type Page } from "@playwright/test";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr, type HerdrFixture } from "../../web/e2e/herdr-fixture";
import { enterWorkspace } from "../../web/e2e/wire";
import {
  bootoutTestLabel, claudeSettings, codexHooks, deviceHome, hcoordLabel, launchdPid, OPERATOR_HCOORD_LABEL,
  proveDeviceHome, readSettings, resetDeviceHome, stageBuild, writeSshConfig, type AgentSettings,
} from "./device-home";
import { hostLog, isolate, launch, screenshot, test } from "./fixture";

test.describe.configure({ timeout: 300_000 });
test.skip(!process.env.HIDE_E2E_SSH_PORT, "an isolated SSH server is required");

const DEVICE = "ssh-kit";
const ALIAS = "isolated-kit";
const PARTS = ["cli", "claude_code_hook", "codex_hook", "labels", "hcoord"];
const LABELS_ID = "hide.agent-context-labels";

function quote(value: string): string { return `'${value.replaceAll("'", "'\\''")}'`; }

type DaemonEvent = { kind?: string; device_id?: string; reason?: string; components?: { id: string; state?: string; outcome?: unknown; reason?: string | null }[] };

function daemonEvents(log: string): DaemonEvent[] {
  return fs.readFileSync(log, "utf8").split("\n").filter((line) => line.startsWith("{")).map((line) => JSON.parse(line) as DaemonEvent);
}

async function sendFrame(page: Page, state: { port: number; token: string }, frame: Record<string, unknown>): Promise<void> {
  await page.evaluate(async ({ port, token, frame }) => {
    await new Promise<void>((resolve, reject) => {
      const ws = new WebSocket(`ws://127.0.0.1:${port}/ws`);
      const timer = setTimeout(() => reject(new Error("frame timed out")), 10_000);
      ws.onerror = () => { clearTimeout(timer); reject(new Error("frame socket failed")); };
      ws.onopen = () => ws.send(JSON.stringify({ token, schema_version: 2 }));
      ws.onmessage = () => {
        ws.send(JSON.stringify({ schema_version: 2, ...frame }));
        clearTimeout(timer); ws.close(); resolve();
      };
    });
  }, { ...state, frame });
}

/** The plugin list the device's Herdr answers, as JSON text. */
function devicePlugins(herdr: HerdrFixture): string {
  const listed = spawnSync(herdr.bin, ["plugin", "list", "--json"], { env: herdr.env, encoding: "utf8", timeout: 10_000 });
  expect(listed.status, listed.stderr).toBe(0);
  return listed.stdout;
}

/** Hide's command for `event`, as the kit wrote it into an agent runtime's file. */
function hideCommand(settings: AgentSettings, event: string, hooks: string): string {
  const command = settings.hooks[event]?.flatMap((group) => group.hooks).find((hook) => hook.command.includes(hooks))?.command;
  expect(command, `no Hide ${event} hook`).toBeTruthy();
  return command!;
}

/** The file with every entry naming Hide's hook binary taken out, as an operator deleting it by hand would. */
function withoutHide(settings: AgentSettings, hooks: string): AgentSettings {
  const events = Object.fromEntries(Object.entries(settings.hooks)
    .map(([event, groups]) => [event, groups
      .map((group) => ({ ...group, hooks: group.hooks.filter((hook) => !hook.command.includes(hooks)) }))
      .filter((group) => group.hooks.length > 0)])
    .filter(([, groups]) => groups.length > 0));
  return { ...settings, hooks: events };
}

/** Runs a shell line in the device's first pane and returns its exit status and output. */
async function inDevicePane(herdr: HerdrFixture, line: string, label: string): Promise<{ status: number; stdout: string }> {
  const result = path.join(herdr.root, `${label}.out`);
  const exit = path.join(herdr.root, `${label}.exit`);
  const sent = spawnSync(herdr.bin, ["pane", "send-text", herdr.panes[0], `${line} > ${quote(result)}; printf '%s' "$?" > ${quote(exit)}\n`], { env: herdr.env, encoding: "utf8", timeout: 10_000 });
  expect(sent.status, sent.stderr).toBe(0);
  await expect.poll(() => fs.existsSync(exit), { timeout: 30_000 }).toBe(true);
  return { status: Number(fs.readFileSync(exit, "utf8")), stdout: fs.readFileSync(result, "utf8") };
}

test("a device gets this Mac's kit, keeps a part the operator removed out until Reinstall, and gives the kit back on removal", async () => {
  const home = deviceHome();
  const local = await startHerdr({ agents: false });
  const device = await startHerdr({ agents: false });
  const run = isolate(local, "kit");
  const bridge = fs.mkdtempSync("/tmp/hide-kb-");
  const helper = path.join(run.root, "device-helper");
  const cliDir = path.join(run.root, "device-bin");
  run.env.HIDE_WORKSPACE_BRIDGE_DIR = bridge;
  run.env.HIDE_HOST_HELPER_ROOT = helper;
  run.env.HIDE_HOST_CLI_DIR = cliDir;
  writeSshConfig(run.env.HOME!, [ALIAS]);
  const hcoordHome = proveDeviceHome(run.env, ALIAS, home);
  const label = hcoordLabel(hcoordHome);
  const operatorDaemon = launchdPid(OPERATOR_HCOORD_LABEL);
  const original = resetDeviceHome(home, label);
  const build = stageBuild(run.root);
  const daemonLog = path.join(run.root, "daemon.log");
  const daemonOutput = fs.openSync(daemonLog, "w");
  const daemon = spawn(path.join(build, "hided"), [], { env: run.env, stdio: ["ignore", daemonOutput, daemonOutput] });
  fs.closeSync(daemonOutput);
  let app: Awaited<ReturnType<typeof launch>>["app"] | undefined;
  try {
    await expect.poll(() => run.hide(["status", "--json"]).stdout.includes('"running":true'), { timeout: 30_000 }).toBe(true);
    ({ app } = await launch(run.env));
    const page = await app.firstWindow();
    await enterWorkspace(page, "fixture");
    const state = JSON.parse(fs.readFileSync(path.join(run.env.HIDE_STATE_DIR!, "hided.json"), "utf8")) as { port: number; token: string };

    // B13: allowing the helper is the only question; the connection installs every part.
    await sendFrame(page, state, { kind: "register_device", payload: {
      id: DEVICE, label: "SSH kit fixture", ssh_alias: ALIAS, herdr_socket_path: device.socket, host_consent: true,
    } });
    const applied = () => daemonEvents(daemonLog).filter((line) => line.kind === "apply.completed" && line.device_id === DEVICE);
    await expect.poll(() => applied().length, { timeout: 120_000 }).toBeGreaterThan(0);
    expect(applied()[0]!.components).toEqual(PARTS.map((id) => expect.objectContaining({ id, state: "installed" })));

    // B16: every part names the helper root's `current`, which outlives a build.
    const current = path.join(fs.realpathSync(helper), "current");
    const hooks = path.join(current, "hide-agent-hooks");
    expect(fs.readlinkSync(path.join(cliDir, "hide"))).toBe(path.join(current, "hide"));
    for (const [file, before] of [[claudeSettings(home), original.claude], [codexHooks(home), original.codex]] as const) {
      const now = readSettings(file);
      hideCommand(now, "SessionStart", hooks);
      // Another tool's entries and settings stay as they were.
      expect(now.hooks.SessionStart).toEqual(expect.arrayContaining(before.hooks.SessionStart!));
      expect({ ...now, hooks: undefined }).toEqual({ ...before, hooks: undefined });
    }
    expect(devicePlugins(device)).toContain(LABELS_ID);
    expect(devicePlugins(device)).toContain(path.join(home, ".hide", "kit", "plugins", "agent-context-labels"));
    expect(fs.readFileSync(path.join(home, ".hcoord", "bin", "hcoord"), "utf8")).toContain(`HCOORD_HOME=${quote(hcoordHome)}`);
    expect(launchdPid(label)).not.toBeNull();
    expect(launchdPid(OPERATOR_HCOORD_LABEL)).toBe(operatorDaemon);

    await page.locator("[data-open-settings]").click();
    await expect(page.locator('[data-settings="true"]')).toBeVisible();
    await page.locator('[data-settings-tab="devices"]').click();
    for (const id of PARTS) await expect(page.locator(`[data-kit-part="${DEVICE}:${id}:installed"]`)).toBeVisible();
    await expect(page.locator(`[data-kit-reinstall="${DEVICE}"]`)).toHaveCount(0);
    await screenshot(page, "device-kit-installed");
    await page.locator('[data-settings-tab="agents"]').click();
    await expect(page.locator('[data-settings-tab="agents"]')).toHaveAttribute("data-state", "active");
    await screenshot(page, "agents-hooks-per-machine");

    // B25, B26: the hook the kit installed runs in a device pane and names that device's Workspace, with no Memory there.
    const session = await inDevicePane(device, `printf '{}' | HIDE_STATE_DIR=${quote(path.join(run.root, "device-cli-state"))} HIDE_WORKSPACE_BRIDGE_DIR=${quote(bridge)} sh -c ${quote(hideCommand(readSettings(claudeSettings(home)), "SessionStart", hooks))}`, "device-session-start");
    expect(session.status).toBe(0);
    const context = (JSON.parse(session.stdout) as { hookSpecificOutput: { additionalContext: string } }).hookSpecificOutput.additionalContext;
    expect(context).toContain("Hide Workspace control is available for this session's checkout");
    expect(context).not.toContain("hide-memory-context");

    // D-26, B8: an entry the operator deleted stays deleted until Reinstall, which puts back only that part.
    fs.writeFileSync(claudeSettings(home), `${JSON.stringify(withoutHide(readSettings(claudeSettings(home)), hooks), null, 2)}\n`);
    await page.locator('[data-settings-tab="devices"]').click();
    await expect(page.locator(`[data-kit-part="${DEVICE}:claude_code_hook:removed"]`)).toBeVisible({ timeout: 60_000 });
    await expect(page.locator(`[data-kit-reinstall="${DEVICE}"]`)).toBeVisible();
    await screenshot(page, "device-kit-removed-part");
    expect(readSettings(claudeSettings(home))).toEqual(original.claude);
    const reinstalls = applied().length;
    await page.locator(`[data-kit-reinstall="${DEVICE}"]`).click();
    await expect(page.locator(`[data-kit-part="${DEVICE}:claude_code_hook:installed"]`)).toBeVisible({ timeout: 60_000 });
    await expect(page.locator(`[data-kit-reinstall="${DEVICE}"]`)).toHaveCount(0);
    expect(applied().length).toBeGreaterThan(reinstalls);
    hideCommand(readSettings(claudeSettings(home)), "SessionStart", hooks);

    // B22, B23: removal says what comes off and what stays, then takes only Hide's parts off.
    await page.locator(`[data-device-remove="${DEVICE}"]`).click();
    await expect(page.locator(`[data-device-remove-confirm="${DEVICE}"]`)).toBeVisible();
    await expect(page.locator(`[data-device-remove-kit="${DEVICE}"]`)).toContainText("hcoord stays");
    await screenshot(page, "device-remove-confirm");
    const beforeRemoval = daemonEvents(daemonLog).length;
    await page.locator('[data-device-remove-go="true"]').click();
    await expect.poll(() => daemonEvents(daemonLog).slice(beforeRemoval)
      .find((line) => line.device_id === DEVICE && /^device\.kit_(removed|left)$/.test(line.kind ?? ""))?.kind, { timeout: 60_000 }).toBe("device.kit_removed");
    expect(readSettings(claudeSettings(home))).toEqual(original.claude);
    expect(readSettings(codexHooks(home))).toEqual(original.codex);
    expect(fs.lstatSync(path.join(cliDir, "hide"), { throwIfNoEntry: false })).toBeUndefined();
    expect(fs.existsSync(helper)).toBe(false);
    expect(devicePlugins(device)).not.toContain(LABELS_ID);
    // hcoord stays: other tools on the device drive agents through it.
    expect(fs.existsSync(path.join(home, ".hcoord", "bin", "hcoord"))).toBe(true);
    expect(launchdPid(label)).not.toBeNull();
    expect(launchdPid(OPERATOR_HCOORD_LABEL)).toBe(operatorDaemon);
  } catch (error) {
    console.log(hostLog(run.env).map((line) => JSON.stringify(line)).join("\n"));
    console.log(fs.readFileSync(daemonLog, "utf8"));
    throw error;
  } finally {
    await app?.close().catch(() => undefined);
    run.cleanup();
    if (daemon.exitCode === null) daemon.kill("SIGTERM");
    bootoutTestLabel(label);
    fs.rmSync(bridge, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
    device.stop();
    local.stop();
  }
});
