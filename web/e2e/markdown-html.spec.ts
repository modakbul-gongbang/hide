// Markdown Live draws HTML blocks and images (PRD markdown-html): the
// repository's own README opens as the page GitHub shows, a caret inside a
// block shows its source, and what a document names but may not load (a
// script, a path outside the checkout, an http address) never reaches the
// page. Runs on an isolated pinned Herdr and hided; the operator's is never
// touched.

import { expect, test, type Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { toPage } from "../../desktop/src/main/wirePath";
import { startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { chord } from "./chords";
import { countSent, screenshot, showExplorer, showProjectRow } from "./wire";

/** A 1x1 PNG, so an image has real bytes to decode. */
const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==",
  "base64",
);

const UNSAFE = [
  "# Unsafe",
  "",
  '<p id="unsafe">kept text<script>window.pwned = "script"</script><iframe src="https://example.com"></iframe>',
  '<a href="javascript:window.pwned = 1">script link</a> <img src="shot.png" alt="onerror" onerror="window.pwned = 2"></p>',
  "",
  "![inside](shot.png)",
  "",
  "![outside](../outside.png)",
  "",
  "![gone](missing.png)",
  "",
  "![plain http](http://127.0.0.1:9/never.png)",
  "",
].join("\n");

type Stack = { herdr: HerdrFixture; daemon: Daemon; repo: string; sent: Map<string, number> };

/** Starts the isolated stack with `files` written into the checkout, and leaves the Explorer showing it. */
async function openStack(page: Page, files: Record<string, Buffer | string>): Promise<Stack> {
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const repoDir = path.join(herdr.root, "repo");
    fs.mkdirSync(repoDir, { recursive: true });
    for (const [name, contents] of Object.entries(files)) {
      fs.mkdirSync(path.dirname(path.join(repoDir, name)), { recursive: true });
      fs.writeFileSync(path.join(repoDir, name), contents);
    }
    fs.writeFileSync(path.join(herdr.root, "outside.png"), PNG);
    herdr.run(["workspace", "create", "--cwd", repoDir, "--label", "repo", "--env", `PATH=${herdr.fixturePath}`, "--no-focus"]);
    daemon = await startHided(herdr, "markdown-html");
    const sent = countSent(page, new Map());
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    const row = await showProjectRow(page, page.locator("[data-project]", { hasText: "repo" }).locator("[data-checkout]").first());
    await row.click();
    await showExplorer(page);
    return { herdr, daemon, repo: toPage(fs.realpathSync(repoDir)), sent };
  } catch (error) {
    daemon?.stop();
    herdr.stop();
    throw error;
  }
}

function close({ daemon, herdr }: Stack): void {
  daemon.stop();
  herdr.stop();
}

test.describe.configure({ timeout: 90_000 });

test("the repository README opens as drawn HTML and shows a block source under the caret", async ({ page }) => {
  const root = path.resolve("..");
  const stack = await openStack(page, {
    "README.md": fs.readFileSync(path.join(root, "README.md")),
    "design/brand/hide-icon-blinking.svg": fs.readFileSync(path.join(root, "design/brand/hide-icon-blinking.svg")),
    "site/assets/hide-workspace.webp": fs.readFileSync(path.join(root, "site/assets/hide-workspace.webp")),
  });
  const appRequests: string[] = [];
  page.on("request", (request) => {
    if (request.url().startsWith(stack.daemon.origin)) appRequests.push(new URL(request.url()).pathname);
  });
  try {
    await page.locator(`[data-explorer-row="${stack.repo}/README.md"]`).click();
    const blocks = page.locator(".cm-md-html-block");
    await expect(blocks.first()).toBeVisible({ timeout: 20_000 });
    expect(await blocks.count()).toBeGreaterThanOrEqual(6);
    const hero = page.locator('.cm-md-html img[alt^="The hide window"]');
    await expect(hero).toHaveAttribute("src", /^blob:/);
    // The logo and the hero screenshot come through the file-bytes path as blobs and decode.
    const logo = page.locator('.cm-md-html img[alt="hide owl app icon"]');
    await expect(logo).toBeVisible();
    await expect.poll(() => logo.evaluate((image: HTMLImageElement) => image.complete && image.naturalWidth > 0)).toBe(true);
    await expect(logo).toHaveAttribute("src", /^blob:/);
    await expect(page.locator(".cm-content")).toContainText("Run more agents. Hold less.");
    await expect(page.locator(".cm-content")).not.toContainText('<p align="center">');
    // Nothing was fetched from the app's own address for a relative image.
    expect(appRequests.filter((pathname) => pathname.includes("/design/") || pathname.includes("/site/"))).toEqual([]);
    await screenshot(page, "markdown-html-readme-live");

    // The same page in the light theme: tokens only, so nothing is left dark.
    await page.keyboard.press(chord("settings"));
    await page.locator('[data-settings-tab="general"]').click();
    await page.locator('[data-theme-option="light"]').click();
    await expect(page.locator("html")).toHaveClass(/(^|\s)light(\s|$)/);
    await page.keyboard.press("Escape");
    expect(await blocks.count()).toBeGreaterThanOrEqual(6);
    await screenshot(page, "markdown-html-readme-live-light");

    // A caret in the hero block shows that block's source and no other.
    await blocks.filter({ has: page.locator('img[alt^="The hide window"]') }).click();
    await expect(hero).toHaveCount(0);
    await expect(logo).toBeVisible();
    await expect(page.locator(".cm-content")).toContainText('<img src="site/assets/hide-workspace.webp"');
    await screenshot(page, "markdown-html-hero-source");
  } finally {
    close(stack);
  }
});

test("a document's script, outside path and http image never load, and a broken image shows its alt text", async ({ page }) => {
  const stack = await openStack(page, { "unsafe.md": UNSAFE, "shot.png": PNG });
  const external: string[] = [];
  page.on("request", (request) => {
    if (request.url().includes("127.0.0.1:9") || request.url().includes("example.com")) external.push(request.url());
  });
  try {
    await page.locator(`[data-explorer-row="${stack.repo}/unsafe.md"]`).click();
    // The id is not on the allowlist, so the block is found by its text.
    const block = page.locator(".cm-md-html-block", { hasText: "kept text" });
    await expect(block).toBeVisible({ timeout: 20_000 });
    await expect(block).not.toContainText("window.pwned");
    await expect(page.locator(".cm-md-html iframe, .cm-md-html script, .cm-md-html a[href^='javascript']")).toHaveCount(0);
    // The inside image loaded; the three others show their alt text and nothing else.
    await expect(page.locator('.cm-md-html img[alt="inside"]')).toHaveAttribute("src", /^blob:/);
    for (const alt of ["outside", "gone", "plain http"]) {
      await expect(page.locator(".cm-md-alt[data-state='failed']", { hasText: alt })).toBeVisible();
    }
    expect(await page.evaluate(() => (window as unknown as { pwned?: unknown }).pwned)).toBeUndefined();
    expect(external).toEqual([]);
    // shot.png twice (the tag and the Markdown image share one read) and missing.png; ../outside.png is never asked for.
    expect(stack.sent.get("file_bytes")).toBe(2);
    await screenshot(page, "markdown-html-broken-images");
  } finally {
    close(stack);
  }
});
