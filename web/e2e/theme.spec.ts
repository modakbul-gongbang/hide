// The web design system reset on an isolated pinned Herdr and hided: Settings
// chooses System, Light or Dark and an accent, the page switches its tokens at
// once, an open terminal is re-colored without being recreated (B13), the
// choice survives a daemon restart (B6, B7), and the production build hided
// serves carries no /gallery (B14).

import { expect, test, type Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { enterWorkspace, screenshot } from "./wire";

test.describe.configure({ timeout: 120_000 });

async function open(page: Page, daemon: Daemon): Promise<void> {
  // The probe reads the terminal buffer, which xterm draws on a canvas.
  await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
}

/** What the focused terminal shows; xterm draws on a canvas, so the page's probe reads its buffer. */
function screenText(page: Page): Promise<string> {
  return page.evaluate(() => window.__hideProbe?.screenText() ?? "");
}

/** A color token as the page resolves it now, as the browser's rgb() form. */
function token(page: Page, name: string): Promise<string> {
  return page.evaluate((property) => {
    const probe = document.createElement("span");
    probe.style.setProperty("color", `var(${property})`);
    document.body.append(probe);
    const color = getComputedStyle(probe).color;
    probe.remove();
    return color;
  }, name);
}

/** A tokens.json hex in the same rgb() form. */
function rgb(hex: string): string {
  const [r, g, b] = [1, 3, 5].map((at) => parseInt(hex.slice(at, at + 2), 16));
  return `rgb(${r}, ${g}, ${b})`;
}

function tokenFile(): Record<string, { type: string; value: string; light?: string }> {
  return JSON.parse(fs.readFileSync(path.resolve("..", "design", "tokens.json"), "utf8")).tokens;
}

async function openAppearance(page: Page): Promise<void> {
  await page.keyboard.press("Alt+Comma");
  await expect(page.locator('[data-settings="true"]')).toBeVisible();
  await page.locator('[data-settings-tab="appearance"]').click();
}

test("the theme and accent switch at once, keep the terminal, and survive a restart", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const tokens = tokenFile();
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    daemon = await startHided(herdr, "theme");
    await open(page, daemon);
    await enterWorkspace(page);

    // The first paint is Dark, the default (D-14).
    const root = page.locator("html");
    await expect(root).toHaveClass(/(^|\s)dark(\s|$)/);
    expect(await token(page, "--background")).toBe(rgb(tokens["--background"]!.value));

    // A line typed into the terminal, and a mark on its element, tell a
    // re-colored terminal from a recreated one.
    const pane = page.locator('[data-pane-view][data-focused="true"]');
    await pane.locator(".xterm-helper-textarea").focus();
    await page.keyboard.type("echo theme-probe-7");
    await page.keyboard.press("Enter");
    await expect.poll(() => screenText(page), { timeout: 15_000 }).toContain("theme-probe-7");
    await pane.locator(".xterm").evaluate((element) => {
      (element as HTMLElement).dataset.themeProbe = "kept";
    });
    await pane.locator(".xterm-helper-textarea").focus();
    await page.keyboard.type("half-typed");

    await openAppearance(page);
    await page.locator('[data-theme-option="light"]').click();
    await expect(root).toHaveClass(/(^|\s)light(\s|$)/);
    await expect(root).not.toHaveClass(/(^|\s)dark(\s|$)/);
    expect(await token(page, "--background")).toBe(rgb(tokens["--background"]!.light!));
    await page.locator('[data-accent="sky"]').click();
    await expect.poll(() => token(page, "--primary")).toBe(rgb(tokens["--accent-choice-sky"]!.light!));
    await screenshot(page, "theme-light-settings");
    await page.keyboard.press("Escape");
    await expect(page.locator('[data-settings="true"]')).toHaveCount(0);

    // The terminal is the same element with the same content and the same
    // half-typed line (B13).
    await expect(pane.locator('.xterm[data-theme-probe="kept"]')).toHaveCount(1);
    const shown = await screenText(page);
    expect(shown).toContain("theme-probe-7");
    expect(shown).toContain("half-typed");
    await screenshot(page, "theme-light-workspace");

    // System follows the OS appearance live (D-14).
    await openAppearance(page);
    await page.locator('[data-theme-option="system"]').click();
    await page.emulateMedia({ colorScheme: "dark" });
    await expect(root).toHaveClass(/(^|\s)dark(\s|$)/);
    await page.emulateMedia({ colorScheme: "light" });
    await expect(root).toHaveClass(/(^|\s)light(\s|$)/);
    await page.locator('[data-theme-option="light"]').click();
    await page.keyboard.press("Escape");

    // The choice is the core's, so a restarted daemon and a reloaded page
    // bring it back.
    daemon = await daemon.restart();
    await open(page, daemon);
    await expect(page.locator("[data-main-screen]").or(page.locator("[data-workspace-screen]"))).toBeVisible({ timeout: 20_000 });
    await expect(root).toHaveClass(/(^|\s)light(\s|$)/);
    await expect.poll(() => token(page, "--primary")).toBe(rgb(tokens["--accent-choice-sky"]!.light!));
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});

test("the production build hided serves has no gallery", async ({ page }) => {
  // The gallery is a dev-only module (B14): no built asset carries it...
  const assets = path.resolve("dist", "assets");
  for (const file of fs.readdirSync(assets).filter((name) => name.endsWith(".js"))) {
    expect(fs.readFileSync(path.join(assets, file), "utf8"), file).not.toContain("data-gallery-section");
  }
  // ...and /gallery on the served page opens nothing of it.
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    daemon = await startHided(herdr, "gallery");
    await page.goto(`${daemon.origin}/gallery#token=${daemon.token}`);
    await page.waitForLoadState("networkidle");
    await expect(page.locator("[data-gallery-section]")).toHaveCount(0);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
