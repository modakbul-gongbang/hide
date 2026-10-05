import { expect, test } from "@playwright/test";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fixtureExecutable } from "./platform-fixture";
import { runInPane, startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { enterWorkspace } from "./wire";
import { toPage } from "../../desktop/src/main/wirePath";

test.describe.configure({ timeout: 90_000 });

type CliView = { view_id: string; area_id: string; kind: string; target: string };
type CliAnswer = {
  ok: boolean;
  reason?: string;
  result: {
    context: { checkout_path: string };
    capabilities?: string[];
    view_id?: string;
    views?: CliView[];
  };
};

async function fromPane(herdr: HerdrFixture, daemon: Daemon, args: string[], sequence: number, expectedStatus = 0): Promise<CliAnswer> {
  const hide = path.resolve("..", "target", "debug", fixtureExecutable("hide"));
  const ran = await runInPane(herdr, herdr.panes[0], `cli-${sequence}`, { env: { HIDE_STATE_DIR: daemon.stateDir }, argv: [hide, ...args], stdout: true });
  expect(ran.status, ran.stderr).toBe(expectedStatus);
  return JSON.parse(ran.stdout) as CliAnswer;
}

test("pane CLI opens its own file and diff while another Workspace remains in front", async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | null = null;
  try {
    const first = path.join(herdr.root, "fixture");
    // The CLI answers in the wire spelling (`/` between names).
    const checkout = toPage(fs.realpathSync(first));
    const file = path.join(first, "보고서.md");
    const changed = path.join(first, "changed.txt");
    const expectedFile = `${checkout}/보고서.md`;
    const expectedChanged = `${checkout}/changed.txt`;
    fs.writeFileSync(file, "한글 report\n");
    fs.writeFileSync(changed, "baseline\n");
    const git = (args: string[]) => {
      const result = spawnSync("git", ["-C", first, ...args], { encoding: "utf8" });
      expect(result.status, result.stderr).toBe(0);
    };
    git(["init", "-q"]);
    git(["add", "changed.txt"]);
    git(["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "baseline"]);
    fs.writeFileSync(changed, "modified\n");
    const beta = path.join(herdr.root, "beta");
    fs.mkdirSync(beta);
    fs.writeFileSync(path.join(beta, "README.md"), "Second Workspace\n");
    for (const args of [
      ["init", "-q"],
      ["add", "README.md"],
      ["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "baseline"],
    ]) {
      const result = spawnSync("git", ["-C", beta, ...args], { encoding: "utf8" });
      expect(result.status, result.stderr).toBe(0);
    }
    herdr.run(["workspace", "create", "--cwd", beta, "--label", "beta", "--env", `PATH=${herdr.fixturePath}`, "--focus"]);

    daemon = await startHided(herdr, "workspace-cli");
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await enterWorkspace(page);
    await page.locator('[data-sidebar-mode="projects"]').click();
    const betaRow = page.locator("[data-project]", { hasText: "beta" }).locator("[data-checkout]").first();
    await betaRow.click();
    await expect(betaRow).toHaveAttribute("aria-current", "true");
    const foregroundPane = await page.locator('[data-pane-view][data-focused="true"]').getAttribute("data-pane-view");
    expect(foregroundPane).toBeTruthy();

    const info = await fromPane(herdr, daemon, ["workspace", "info"], 1);
    expect(info.ok).toBe(true);
    expect(info.result.context.checkout_path).toBe(checkout);
    expect(info.result.capabilities).toContain("file.open");
    expect(info.result.capabilities).not.toContain("browser.open");
    const unsupported = await fromPane(herdr, daemon, ["browser", "open", "https://example.com/"], 20, 2);
    expect(unsupported).toMatchObject({ ok: false, reason: "browser_unsupported" });
    const opened = await fromPane(herdr, daemon, ["file", "open", "보고서.md"], 2);
    expect(opened.ok).toBe(true);
    expect(opened.result.context.checkout_path).toBe(checkout);
    expect(opened.result.view_id).toBeTruthy();
    const diff = await fromPane(herdr, daemon, ["diff", "open", "changed.txt"], 3);
    expect(diff.ok).toBe(true);
    const views = await fromPane(herdr, daemon, ["view", "list"], 4);
    expect(views.ok).toBe(true);
    expect(views.result.views).toEqual(expect.arrayContaining([
      expect.objectContaining({ view_id: opened.result.view_id, kind: "file", target: expectedFile }),
      expect.objectContaining({ view_id: diff.result.view_id, kind: "diff", target: expectedChanged }),
    ]));
    const fileView = views.result.views!.find((view) => view.view_id === opened.result.view_id);
    expect(fileView).toBeDefined();
    const retryId = `${Date.now()}-fixture-split`;
    const command = ["view", "split", fileView!.view_id, "--area", fileView!.area_id, "--edge", "right", "--request-id", retryId];
    const split = await fromPane(herdr, daemon, command, 5);
    expect(split.ok).toBe(true);
    expect(await fromPane(herdr, daemon, command, 6)).toEqual(split);
    await expect(betaRow).toHaveAttribute("aria-current", "true");
    await expect(page.locator('[data-pane-view][data-focused="true"]')).toHaveAttribute("data-pane-view", foregroundPane!);
    if (process.env.HIDE_E2E_SCREENSHOT_DIR) {
      await page.screenshot({ path: path.join(process.env.HIDE_E2E_SCREENSHOT_DIR, "workspace-cli-background.png") });
    }
    const firstRow = page.locator("[data-project]", { hasText: "fixture" }).locator("[data-checkout]").first();
    await firstRow.click();
    // Opens without --reveal leave the Workspace's File Views as it was: off.
    const workspace = page.locator("[data-workspace-screen]");
    await expect(workspace).toHaveAttribute("data-file-views", "off");
    await page.locator('[data-column-toggle="views"]').click();
    await expect(workspace).toHaveAttribute("data-file-views", "shown");
    await expect(page.locator('[data-view-tab-bar] [role="tab"][aria-label*="/보고서.md"]')).toHaveCount(1);
    await expect(page.locator('[data-view-tab-bar] [role="tab"][aria-label*="/changed.txt"]')).toHaveCount(1);
    await expect(page.locator("[data-view-area-id]")).toHaveCount(2);
    const diffArea = page.locator("[data-view-area-id]").filter({ has: page.locator('[data-tab-kind="diff"]') });
    await expect(diffArea).toContainText("modified", { timeout: 10_000 });
    if (process.env.HIDE_E2E_SCREENSHOT_DIR) {
      await page.screenshot({ path: path.join(process.env.HIDE_E2E_SCREENSHOT_DIR, "workspace-cli-views.png") });
    }
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
