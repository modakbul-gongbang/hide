import { expect, test, type Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { enterWorkspace, screenshot } from "./wire";
import { chord } from "./chords";

async function openSettings(page: Page, daemon: Daemon) {
  // A hash-only navigation after restart does not remount the connection.
  if (page.url().startsWith(daemon.origin)) await page.goto("about:blank");
  await page.goto(`${daemon.origin}/#token=${daemon.token}`);
  await enterWorkspace(page, "fixture");
  await page.locator("[data-open-settings]").click();
  await expect(page.locator("[data-interface-language]")).toBeEnabled();
}

async function choose(page: Page, language: string) {
  await page.locator("[data-interface-language]").click();
  await page.locator(`[data-language-option="${language}"]`).click();
  await expect(page.locator("[data-interface-language]")).toHaveAttribute("data-interface-language", language);
  await expect(page.locator("[data-interface-language]")).toBeEnabled();
}

test("one confirmed choice follows every window, persists, and resets to each system language", async ({ browser }) => {
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  const korean = await browser.newContext({ locale: "ko-KR" });
  const japanese = await browser.newContext({ locale: "ja-JP" });
  try {
    daemon = await startHided(herdr, "interface-language");
    const left = await korean.newPage();
    const right = await japanese.newPage();
    await openSettings(left, daemon);
    await openSettings(right, daemon);
    await expect(left.locator("html")).toHaveAttribute("lang", "ko");
    await expect(right.locator("html")).toHaveAttribute("lang", "ja");
    await expect(left.locator("[data-interface-language]")).toHaveAttribute("data-interface-language", "system");
    // The answers are product copy, independent of the translation catalog.
    const keycaps = await left.locator(".sidebar-command-keycap").allTextContents();
    for (const [language, heading, general, overview, projects, agents, all, scope, close, change, clear, projectCommand, agentCommand] of [
      ["en", "Settings", "General", "Overview", "Projects", "Agents", "All projects", "Overview scope", "Close Overview", "Change the shortcut for Overview, now {chord}", "Clear the shortcut for Overview", "Projects sidebar", "Agents sidebar"],
      ["ko", "설정", "일반", "개요", "프로젝트", "에이전트", "모든 프로젝트", "개요 범위", "개요 닫기", "개요 단축키 변경, 현재 {chord}", "개요 단축키 해제", "프로젝트 사이드바", "에이전트 사이드바"],
      ["zh-CN", "设置", "常规", "概览", "项目", "智能体", "所有项目", "概览范围", "关闭概览", "更改概览的快捷键，当前为 {chord}", "清除概览的快捷键", "项目侧边栏", "智能体侧边栏"],
      ["ja", "設定", "一般", "概要", "プロジェクト", "エージェント", "すべてのプロジェクト", "概要の範囲", "概要を閉じる", "概要のショートカットを変更、現在 {chord}", "概要のショートカットを解除", "プロジェクトサイドバー", "エージェントサイドバー"],
    ]) {
      await choose(left, language!);
      for (const page of [left, right]) {
        await expect(page.locator("html")).toHaveAttribute("lang", language!);
        await expect(page.locator("[data-settings] h2")).toHaveText(heading!);
        await expect(page.locator('[data-settings-tab="general"]')).toHaveText(general!);
      }
      // User-owned names and paths remain byte-for-byte unchanged.
      await expect(left.locator("[data-sidebar-title-name]")).toHaveText("This Mac");
      await expect(left.locator("[data-settings]")).toContainText(path.join(daemon.stateDir, "core-state.json"));
      await expect(left.locator('[data-sidebar-mode="projects"] span').first()).toHaveText(projects!);
      await expect(left.locator('[data-sidebar-mode="agents"] span').first()).toHaveText(agents!);
      await expect(left.locator(".sidebar-command-keycap")).toHaveText(keycaps);
      await left.locator('[data-settings-tab="shortcuts"]').click();
      // The chip names the command and the chord it holds; the clear control its command.
      const overviewChord = (await left.locator('[data-shortcut-effective="overview"]').textContent())!;
      await expect(left.locator('[data-shortcut-record="overview"]')).toHaveAttribute("aria-label", change!.replace("{chord}", overviewChord));
      await expect(left.locator('[data-shortcut-clear="overview"]')).toHaveAttribute("aria-label", clear!);
      await left.keyboard.press("Escape");
      await left.locator("[data-open-overview]").click();
      const modal = left.getByRole("dialog", { name: overview!, exact: true });
      await expect(modal).toBeVisible();
      await expect(modal.getByRole("tablist", { name: scope!, exact: true })).toBeVisible();
      await expect(modal.getByRole("tab", { name: all!, exact: true })).toHaveAttribute("aria-selected", "true");
      await expect(modal.getByRole("tab", { name: "fixture", exact: true })).toBeVisible();
      await expect(modal.getByRole("button", { name: close!, exact: true })).toHaveText("Esc");
      await screenshot(left, `interface-${language}`);
      await left.keyboard.press("Escape");
      // Command labels translate while command ids and effective chords stay stable.
      await left.keyboard.press(chord("shortcuts"));
      for (const [id, title] of [["overview", overview], ["sidebar_projects", projectCommand], ["sidebar_agents", agentCommand]]) {
        await expect(left.locator(`[data-shortcut="${id}"]`).locator("span").first()).toHaveText(title!);
      }
      await left.keyboard.press("Escape");
      await left.locator("[data-open-settings]").click();
      await left.locator('[data-settings-tab="general"]').click();
    }
    await right.reload();
    await expect(right.locator("html")).toHaveAttribute("lang", "ja");
    daemon = await daemon.restart();
    await openSettings(left, daemon);
    await openSettings(right, daemon);
    await expect(left.locator("html")).toHaveAttribute("lang", "ja");
    await choose(right, "system");
    await expect(left.locator("html")).toHaveAttribute("lang", "ko");
    await expect(right.locator("html")).toHaveAttribute("lang", "ja");
    // Reset is durable as an absent explicit choice, not a cached resolved language.
    daemon = await daemon.restart();
    await openSettings(left, daemon);
    await expect(left.locator("html")).toHaveAttribute("lang", "ko");
  } finally {
    try {
      await korean.close();
      await japanese.close();
    } finally {
      daemon?.stop();
      herdr.stop();
    }
  }
});

test("unsupported systems use English, and an invalid stored preference is diagnosed and retained", async ({ browser }) => {
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  const context = await browser.newContext({ locale: "fr-FR" });
  try {
    daemon = await startHided(herdr, "interface-language-invalid");
    const page = await context.newPage();
    await openSettings(page, daemon);
    await expect(page.locator("html")).toHaveAttribute("lang", "en");
    await choose(page, "ko");
    daemon = await daemon.restart((directory) => {
      const file = path.join(directory, "core-state.json");
      const stored = JSON.parse(fs.readFileSync(file, "utf8"));
      stored.interface_language = "unsupported";
      fs.writeFileSync(file, JSON.stringify(stored));
    });
    await openSettings(page, daemon);
    await expect(page.locator("html")).toHaveAttribute("lang", "en");
    await expect(page.locator("[data-diagnostics]")).toContainText("ui_state.interface_language_invalid");
    await expect(page.locator("[data-interface-language]")).toHaveAttribute("data-interface-language", "en");
    expect(JSON.parse(fs.readFileSync(path.join(daemon.stateDir, "core-state.json"), "utf8")).interface_language).toBe("unsupported");
    // Choosing English repairs an invalid value even though English was already drawn.
    await choose(page, "system");
    await choose(page, "en");
    daemon = await daemon.restart();
    await openSettings(page, daemon);
    await expect(page.locator("[data-interface-language]")).toHaveAttribute("data-interface-language", "en");
    expect(JSON.parse(fs.readFileSync(path.join(daemon.stateDir, "core-state.json"), "utf8")).interface_language).toBe("en");
  } finally {
    try {
      await context.close();
    } finally {
      daemon?.stop();
      herdr.stop();
    }
  }
});
