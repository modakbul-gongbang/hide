// Real core, private state and pinned Herdr. Memory UI has no platform-specific behavior.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { expect, type ElectronApplication } from "@playwright/test";
import { startHerdr } from "../../web/e2e/herdr-fixture";
import { countSent, showProjectRow, showTool } from "../../web/e2e/wire";
import { isolate, launch, fitWindow, test } from "./fixture";
import { captureNativeWindow } from "./native-window";
import { seedReconciliation } from "./memory-reconcile-fixture";

test("Memory compares sources, limits a batch and groups a confirmed PR", async () => {
  const herdr = await startHerdr({ agents: false });
  const run = isolate(herdr, "memory-docs");
  let app: ElectronApplication | null = null;
  try {
    const repo = path.join(run.env.HOME!, "projects", "sample");
    fs.mkdirSync(repo);
    execFileSync("git", ["init", "-q", "-b", "main"], { cwd: repo });
    fs.writeFileSync(path.join(repo, "AGENTS.md"), "Use one unpooled connection per write.\n");
    execFileSync("git", ["add", "AGENTS.md"], { cwd: repo });
    execFileSync("git", ["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "Initial guide"], { cwd: repo });
    herdr.run(["workspace", "create", "--cwd", repo, "--label", "sample", "--env", `PATH=${herdr.fixturePath}`, "--no-focus"]);
    const launched = await launch(run.env);
    app = launched.app;
    const page = launched.page;
    const sent = new Map<string, Record<string, unknown>>();
    countSent(page, sent);
    await (await showProjectRow(page, page.locator("[data-project]", { hasText: "sample" }).locator("[data-checkout]").first())).click();
    await showTool(page, "memory");
    await expect(page.locator('[data-memory="off"]')).toBeVisible();
    await page.locator('[data-memory-switch="off"]').click();
    await expect(page.locator('[data-memory="on"]')).toBeVisible({ timeout: 30000 });
    seedReconciliation(run);
    await page.reload();
    await showTool(page, "memory");
    const tab = page.locator('[data-tool="memory"]');
    await expect(tab.locator('[data-memory="on"]')).toBeVisible();
    await expect(tab.locator('[data-memory-pr-group="72"]')).toContainText("m9 · m10");
    await expect(tab).toContainText("Personal");
    await expect(tab).toContainText("Outdated");
    await expect(tab.locator('[data-memory-select="3"]')).toBeDisabled();
    const facts = { home: run.env.HOME, state: run.env.HIDE_STATE_DIR, socket: herdr.socket, fixture: "invented cards, no public remote", daemonPid: run.daemonPid() };
    await captureNativeWindow(app, "memory-compact-dark", facts);
    await tab.locator('[data-memory-compare="2"]').click();
    await expect(tab.locator("[data-memory-comparison]")).toContainText("Use a pooled connection for all writes.");
    await captureNativeWindow(app, "memory-compare-dark", facts);
    await tab.locator('[data-memory-compare="2"]').click();
    await tab.locator('[data-memory-select="1"]').click();
    await tab.locator('[data-memory-select="2"]').click();
    await expect(tab.locator('[data-memory-selection="2"]')).toBeVisible();
    await captureNativeWindow(app, "memory-two-selected-dark", facts);
    await tab.locator('[data-memory-promote="3"]').click();
    await expect(tab.locator('[data-memory-select="3"]')).toBeEnabled();
    for (const number of [6, 7, 8]) await tab.locator(`[data-memory-select="${number}"]`).click();
    await expect(tab.locator('[data-memory-selection="5"]')).toBeVisible();
    await expect(tab.locator('[data-memory-select="3"]')).toBeDisabled();
    await tab.locator('[data-memory-pr-group="72"]').click();
    await expect(page.locator("[data-overview-screen]")).toBeVisible();
    // Back to the same Memory and selection, then the compact light/narrow state.
    await showTool(page, "memory");
    await page.evaluate(() => { document.documentElement.classList.remove("dark"); document.documentElement.classList.add("light"); });
    await fitWindow(app, { width: 900, height: 760 });
    await showTool(page, "memory");
    await captureNativeWindow(app, "memory-compact-light-narrow", facts);
    // Optional bounded interactive inspection of this same private daemon through Aside.
    const inspection = process.env.HIDE_MEMORY_INSPECTION_FILE;
    if (inspection) {
      fs.writeFileSync(inspection, JSON.stringify({ url: page.url(), root: run.root, pid: app.process().pid }));
      await expect.poll(() => fs.existsSync(`${inspection}.done`), { timeout: 180_000 }).toBe(true);
    }
    console.log("Memory overflow", await tab.evaluate((tab) => Array.from(tab.querySelectorAll("*")).filter(e => e.scrollWidth > e.clientWidth + 1).map(e => ({ tag: e.tagName, css: e.className, width: e.clientWidth, scroll: e.scrollWidth, text: e.textContent?.slice(0, 60) })).slice(0, 20)));
    await expect.poll(() => tab.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);
  } finally {
    if (app) await app.close();
    run.cleanup();
    herdr.stop();
  }
});
