// `hide browser` from a device pane over the isolated SSH server: the device's
// CLI reaches hided's relay through the reverse Workspace forward and reads
// and drives the display the same way a local pane does, and fails with the
// Workspace commands' own reason when no desktop is attached (PRD
// hide-browser-cli B34, B40). Setup as in remote-workspace.spec.ts: the two
// Herdr servers, daemon, helper install and desktop profile are private, and
// the device sessions get the private HOME desktop/e2e/device-home.ts proves.
import { expect } from "@playwright/test";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import type { AddressInfo } from "node:net";
import path from "node:path";
import { startHerdr, type HerdrFixture } from "../../web/e2e/herdr-fixture";
import { enterWorkspace } from "../../web/e2e/wire";
import { deviceHome, proveDeviceHome, resetDeviceHome, writeSshConfig } from "./device-home";
import { endChild, HIDE_CLI, hostLog, isolate, launch, test, type Isolated } from "./fixture";

test.describe.configure({ timeout: 300_000 });
test.skip(!process.env.HIDE_E2E_SSH_PORT, "an isolated SSH server is required");

const PAGE = '<!doctype html><meta charset="utf-8"><title>Device page</title><button onclick="document.querySelector(\'output\').textContent=\'clicked from the device\'">Press</button><output></output>';

function quote(value: string): string { return `'${value.replaceAll("'", "'\\''")}'`; }
/** Runs the `hide` the kit linked on the device, in the device's pane. */
async function deviceCli(remote: HerdrFixture, run: Isolated, bridge: string, args: string[], label: string): Promise<{ status: number; out: string }> {
  const out = path.join(remote.root, `${label}.out`), exit = path.join(remote.root, `${label}.exit`);
  const cli = path.join(run.env.HIDE_HOST_CLI_DIR!, "hide");
  const command = `HIDE_WORKSPACE_BRIDGE_DIR=${quote(bridge)} HIDE_STATE_DIR=${quote(path.join(run.root, "remote-cli-state"))} ${[cli, ...args].map(quote).join(" ")} > ${quote(out)}; printf '%s' "$?" > ${quote(exit)}\n`;
  const sent = spawnSync(remote.bin, ["pane", "send-text", remote.panes[0]!, command], { env: remote.env, encoding: "utf8", timeout: 10_000 });
  expect(sent.status, sent.stderr).toBe(0);
  await expect.poll(() => fs.existsSync(exit), { timeout: 60_000 }).toBe(true);
  return { status: Number(fs.readFileSync(exit, "utf8")), out: fs.readFileSync(out, "utf8") };
}
const lastJson = (out: string) => JSON.parse(out.trim().split("\n").at(-1) || "{}") as Record<string, unknown>;
const liveBridges = (bridge: string) => fs.existsSync(bridge) ? fs.readdirSync(bridge).filter((name) => fs.existsSync(path.join(bridge, name, "bootstrap.sock"))) : [];
const appliedFor = (log: string, device: string) => fs.readFileSync(log, "utf8").split("\n").some((line) => line.startsWith("{") && line.includes('"apply.completed"') && line.includes(`"${device}"`));

test("hide browser from a device pane reads and clicks its page, and fails like a Workspace command without a desktop", async () => {
  const local = await startHerdr({ agents: false });
  const remote = await startHerdr({ agents: false });
  const run = isolate(local, "ssh-browser");
  const bridge = fs.mkdtempSync("/tmp/hide-wb-");
  run.env.HIDE_WORKSPACE_BRIDGE_DIR = bridge;
  run.env.HIDE_HOST_HELPER_ROOT = path.join(run.root, "remote-helper");
  run.env.HIDE_HOST_CLI_DIR = path.join(run.root, "remote-bin");
  writeSshConfig(run.env.HOME!, ["isolated-workspace"]);
  const deviceAccount = deviceHome();
  proveDeviceHome(run.env, "isolated-workspace", deviceAccount);
  resetDeviceHome(deviceAccount);
  const daemonLog = path.join(run.root, "daemon.log");
  const daemonOutput = fs.openSync(daemonLog, "w");
  const daemon = spawn(path.join(path.dirname(HIDE_CLI), "hided"), [], { env: run.env, stdio: ["ignore", daemonOutput, daemonOutput] });
  fs.closeSync(daemonOutput);
  const server = http.createServer((_request, response) => { response.writeHead(200, { "content-type": "text/html; charset=utf-8" }); response.end(PAGE); });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const port = (server.address() as AddressInfo).port;
  let app: Awaited<ReturnType<typeof launch>>["app"] | undefined;
  try {
    await expect.poll(() => run.hide(["status", "--json"]).stdout.includes('"running":true'), { timeout: 30_000 }).toBe(true);
    ({ app } = await launch(run.env));
    const page = await app.firstWindow();
    await enterWorkspace(page, "fixture");
    const state = JSON.parse(fs.readFileSync(path.join(run.env.HIDE_STATE_DIR!, "hided.json"), "utf8")) as { port: number; token: string };
    await page.evaluate(async ({ port, token, socket }) => {
      await new Promise<void>((resolve, reject) => {
        const ws = new WebSocket(`ws://127.0.0.1:${port}/ws`);
        const timer = setTimeout(() => reject(new Error("device registration timed out")), 10_000);
        ws.onerror = () => { clearTimeout(timer); reject(new Error("device registration socket failed")); };
        ws.onopen = () => ws.send(JSON.stringify({ token, schema_version: 2 }));
        ws.onmessage = () => {
          ws.send(JSON.stringify({ schema_version: 2, kind: "register_device", payload: {
            id: "ssh-browser", label: "SSH browser fixture", ssh_alias: "isolated-workspace", herdr_socket_path: socket, host_consent: true,
          } }));
          clearTimeout(timer); ws.close(); resolve();
        };
      });
    }, { port: state.port, token: state.token, socket: remote.socket });
    await expect.poll(() => liveBridges(bridge).length, { timeout: 60_000 }).toBe(1);
    await expect.poll(() => appliedFor(daemonLog, "ssh-browser"), { timeout: 120_000 }).toBe(true);

    const opened = await deviceCli(remote, run, bridge, ["browser", "open", `http://localhost:${port}/device.html`, "--reveal", "--wait"], "device-open");
    expect(opened.status, opened.out).toBe(0);
    const display = (lastJson(opened.out).result as { view_id: string }).view_id;
    const snapshot = await deviceCli(remote, run, bridge, ["browser", "snapshot", display], "device-snapshot");
    expect(snapshot.status, snapshot.out).toBe(0);
    expect(snapshot.out).toMatch(/^# Device page\n# http:\/\/127\.0\.0\.1:\d+\/device\.html\n\n/);
    const press = /(@\d+) button "Press"/.exec(snapshot.out)?.[1];
    expect(press, snapshot.out).toBeDefined();
    const clicked = await deviceCli(remote, run, bridge, ["browser", "click", display, press!], "device-click");
    expect(clicked.status, clicked.out).toBe(0);
    expect(lastJson(clicked.out)).toMatchObject({ ok: true, clicked: press });
    expect(String(lastJson(clicked.out).changed)).toContain("clicked from the device");

    await app.close();
    app = undefined;
    const view = lastJson((await deviceCli(remote, run, bridge, ["view", "list"], "device-view-list")).out);
    const browser = await deviceCli(remote, run, bridge, ["browser", "snapshot", display], "device-snapshot-detached");
    expect(browser.status).not.toBe(0);
    expect(view).toMatchObject({ ok: false, reason: "renderer_unavailable" });
    expect(lastJson(browser.out)).toMatchObject({ ok: false, reason: view.reason, next_action: view.next_action });
  } catch (error) {
    console.log(hostLog(run.env).map((line) => JSON.stringify(line)).join("\n"));
    console.log(fs.readFileSync(daemonLog, "utf8"));
    throw error;
  } finally {
    await app?.close().catch(() => undefined);
    await new Promise<void>((resolve) => server.close(() => resolve()));
    run.cleanup();
    await endChild(daemon).finally(() => { remote.stop(); local.stop(); });
    fs.rmSync(bridge, { recursive: true, force: true });
  }
});
