import { expect, type ElectronApplication } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { startHerdr } from "../../web/e2e/herdr-fixture";
import { enterWorkspace } from "../../web/e2e/wire";
import { openServerButton, openSessions, startServer, writeConversation } from "../../web/e2e/server-session-fixture";
import { assertIsolated, isolate, launch, shellPage, test } from "./fixture";
import { bootoutTestLabel, hcoordLabel, launchdPid, OPERATOR_HCOORD_LABEL } from "./device-home";

test.describe.configure({ timeout: 180_000 });
test.use({ actionTimeout: 15_000 });
// Test-only candidate selection: a packaged executable runs its own bundled
// host and daemon. Absent this path, CI exercises the current desktop build.
const executablePath = process.env.HIDE_E2E_PACKAGED_APP;

test("native fixture refuses a shared or mismatched coordinator before launch", () => {
  const run = isolate({ socket: path.join(os.tmpdir(), "fixture.sock"), bin: "/usr/bin/true" }, "coordinator-guard");
  try {
    expect(() => assertIsolated(run.env)).not.toThrow();
    expect(hcoordLabel(run.env.HCOORD_HOME!)).not.toBe(OPERATOR_HCOORD_LABEL);
    expect(() => assertIsolated({ ...run.env, HCOORD_HOME: "" })).toThrow("HCOORD_HOME");
    expect(() => assertIsolated({ ...run.env, HCOORD_HOME: path.join(run.root, "other-coordinator") })).toThrow("HCOORD_HOME");
  } finally {
    // This case launches no process and never invokes a daemon/kit command.
    fs.rmSync(run.root, { recursive: true, force: true });
  }
});

test("server picker and conversation search in a background native window", async () => {
  const herdr = await startHerdr({ agents: false });
  const run = isolate(herdr, "server-search");
  const privateLabel = hcoordLabel(run.env.HCOORD_HOME!);
  expect(privateLabel).not.toBe(OPERATOR_HCOORD_LABEL);
  expect(() => assertIsolated({ ...run.env, HCOORD_HOME: "" })).toThrow("HCOORD_HOME");
  expect(() => assertIsolated({ ...run.env, HCOORD_HOME: path.join(run.root, "other-coordinator") })).toThrow("HCOORD_HOME");
  const operatorDaemon = launchdPid(OPERATOR_HCOORD_LABEL);
  let app: ElectronApplication | null = null;
  const servers = [];
  try {
    if (executablePath) {
      run.env.HIDE_CLI_PATH = path.resolve(executablePath, "../../Resources/hide");
      delete run.env.HIDED_UI_DIR;
    }
    const cwd = fs.realpathSync(path.join(herdr.root, "fixture"));
    execFileSync("git", ["init", "-q"], { cwd });
    execFileSync("git", ["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-q", "--allow-empty", "-m", "Fixture"], { cwd });
    const source = writeConversation(run.env.HOME!, cwd);
    const original = fs.readFileSync(source);
    servers.push(await startServer(cwd), await startServer(cwd, "::1"));
    const foreign = path.join(herdr.root, "foreign-project");
    fs.mkdirSync(foreign);
    servers.push(await startServer(foreign, "::1", servers[0]!.port));
    ({ app } = await launch(run.env, { executablePath }));
    if (executablePath) await expect.poll(() => launchdPid(privateLabel)).not.toBeNull();
    expect(launchdPid(OPERATOR_HCOORD_LABEL)).toBe(operatorDaemon);
    const page = await shellPage(app);
    await enterWorkspace(page, "fixture");
    const candidate = await app.evaluate(({ BrowserWindow }) => ({ pid: process.pid, executable: process.execPath, resources: process.resourcesPath, source: BrowserWindow.getAllWindows()[0]!.getMediaSourceId(), windows: BrowserWindow.getAllWindows().length }));
    expect(candidate.windows).toBe(1);
    const dir = process.env.HIDE_E2E_SCREENSHOT_DIR;
    if (dir) fs.writeFileSync(path.join(dir, "native-candidate.json"), JSON.stringify(candidate, null, 2));
    const shot = async (name: string) => {
      if (!dir) return;
      // The DOM assertion precedes the macOS compositor presenting the frame.
      // Match the other native capture fixtures before reading the exact window.
      await page.waitForTimeout(400);
      execFileSync("/usr/sbin/screencapture", ["-x", "-o", "-l", candidate.source.split(":")[1]!, path.join(dir, `${name}.png`)]);
    };
    const theme = async (value: "light" | "dark") => {
      await page.getByRole("button", { name: /^Settings \(/ }).click();
      await page.locator('[data-settings-tab="appearance"]').click();
      await page.locator(`[data-theme-option="${value}"]`).click();
      await page.keyboard.press("Escape");
      await expect(page.locator("html")).toHaveClass(new RegExp(value));
    };
    for (const value of ["dark", "light"] as const) {
      await theme(value);
      const globe = openServerButton(page);
      await expect(globe).toHaveCount(1);
      await expect.poll(async () => { await globe.click(); await page.locator("[data-server-port]").first().waitFor({state:"visible",timeout:1000}).catch(() => {}); const count = await page.locator("[data-server-port]").count(); if (count !== 2) await page.keyboard.press("Escape"); return count; }, { timeout: 20_000 }).toBe(2);
      await shot(`native-server-picker-${value}`);
      await page.getByRole("button", { name: `127.0.0.1:${servers[0]!.port}`, exact: true }).click();
      await expect.poll(() => app!.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.contentView.children.map((view) => (view as unknown as { webContents?: Electron.WebContents }).webContents?.getURL()))).toContain(`http://127.0.0.1:${servers[0]!.port}/`);
      await expect(globe).toHaveCount(1);
      await globe.click();
      await expect(page.locator("[data-server-port]")).toHaveCount(2);
      await page.getByRole("button", { name: `[::1]:${servers[1]!.port}`, exact: true }).click();
      await expect(page.locator("[data-browser-address]")).toHaveValue(`[::1]:${servers[1]!.port}`);
      await expect.poll(() => app!.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.contentView.children.some((view) => (view as unknown as { webContents?: Electron.WebContents }).webContents?.getTitle() === "Workspace preview"))).toBe(true);
      await expect.poll(() => app!.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.contentView.children.map((view) => (view as unknown as { webContents?: Electron.WebContents }).webContents?.getURL()))).toContain(`http://[::1]:${servers[1]!.port}/`);
      await shot(`native-server-opened-${value}`);
    }
    await openSessions(page);
    const search = page.getByRole("searchbox", { name: "Search sessions" });
    for (const value of ["light", "dark"] as const) {
      await theme(value);
      await search.fill("화검");
      await expect(page.locator("[data-content-match]")).toHaveCount(1, { timeout: 20_000 });
      await page.locator('[data-session-row="search-session"]').click();
      await expect(page.locator('[data-search-match="true"]')).toBeVisible();
      await shot(`native-session-search-${value}`);
      const conversation = page.getByRole("list", { name: "Conversation" });
      await conversation.evaluate((element) => { element.scrollTop = 0; });
      await search.fill("foo_bar");
      await expect(page.locator("[data-content-match]")).toContainText("foo_bar");
      await expect.poll(() => conversation.evaluate((element) => element.scrollTop)).toBe(0);
    }
    expect(fs.readFileSync(source)).toEqual(original);
    expect(launchdPid(OPERATOR_HCOORD_LABEL)).toBe(operatorDaemon);
  } finally {
    await app?.close();
    // Only the explicit private label is admitted by this existing helper.
    if (executablePath) bootoutTestLabel(privateLabel);
    for (const server of servers) server.child.kill();
    run.cleanup(); herdr.stop();
  }
});
