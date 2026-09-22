// S3 Explorer and editor flows on an isolated pinned Herdr (PRD
// web-shell-pivot-s3 B1, B3, B4, B5): a real git checkout listed lazily in the
// Explorer, a folder expanded through the core's ui state, a file opened into
// the checkout's preview tab, then edited, saved, and saved again after the
// disk changed underneath it. Every command runs against a private server; the
// operator's Herdr is never touched.

import { expect, test, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, screenshot } from "./wire";

const SOURCE = "export const answer = 41;\n";

/** A 1x1 PNG, so an image viewer has real bytes to decode. */
const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==",
  "base64",
);

/** A minimal one-page PDF with a correct cross-reference table. */
function minimalPdf(): Buffer {
  const objects = [
    "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n",
    "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n",
    "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>\nendobj\n",
    "4 0 obj\n<< /Length 44 >>\nstream\nBT /F1 24 Tf 40 100 Td (Hello PDF) Tj ET\nendstream\nendobj\n",
    "5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n",
  ];
  let pdf = "%PDF-1.4\n";
  const offsets: number[] = [];
  for (const object of objects) {
    offsets.push(pdf.length);
    pdf += object;
  }
  const xref = pdf.length;
  pdf += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  for (const offset of offsets) pdf += `${String(offset).padStart(10, "0")} 00000 n \n`;
  pdf += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return Buffer.from(pdf, "latin1");
}

/** A committed file the working tree then changes, so the core reports it. */
function gitFixture(dir: string): void {
  execFileSync("git", ["init", "-q"], { cwd: dir });
  fs.writeFileSync(path.join(dir, "tracked.txt"), "one\n");
  execFileSync("git", ["add", "-A"], { cwd: dir });
  execFileSync("git", ["-c", "user.email=e2e@example.com", "-c", "user.name=e2e", "commit", "-qm", "init"], { cwd: dir });
  fs.writeFileSync(path.join(dir, "tracked.txt"), "two\n");
}

type Fixture = {
  herdr: HerdrFixture;
  daemon: Daemon;
  /** The checkout root as the core spells it (a resolved path). */
  repo: string;
  file: string;
  sent: Map<string, number>;
  lastSent: Map<string, Record<string, unknown>>;
};

/** Starts the isolated stack and leaves the Explorer showing `src/main.ts`. */
async function openCheckout(page: Page): Promise<Fixture> {
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const repoDir = path.join(herdr.root, "repo");
    fs.mkdirSync(path.join(repoDir, "src"), { recursive: true });
    fs.writeFileSync(path.join(repoDir, "src", "main.ts"), SOURCE);
    fs.writeFileSync(path.join(repoDir, "README.md"), "# repo\n");
    fs.writeFileSync(path.join(repoDir, ".gitignore"), "node_modules\n");
    fs.writeFileSync(path.join(repoDir, "shot.png"), PNG);
    fs.writeFileSync(path.join(repoDir, "doc.pdf"), minimalPdf());
    fs.copyFileSync(path.resolve("e2e/fixtures/tiny.mp4"), path.join(repoDir, "clip.mp4"));
    gitFixture(repoDir);
    const repo = fs.realpathSync(repoDir);
    herdr.run([
      "workspace", "create", "--cwd", repoDir, "--label", "repo",
      "--env", `PATH=${herdr.fixturePath}`, "--no-focus",
    ]);

    daemon = await startHided(herdr, "s3");
    const lastSent = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, lastSent);
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);

    // Focus the repository checkout, then the Explorer mode.
    await page.locator('[data-sidebar-mode="projects"]').click();
    const row = page.locator("[data-project]", { hasText: "repo" }).locator("[data-checkout]").first();
    await row.click();
    await expect(row).toHaveAttribute("aria-current", "true");
    await page.locator('[data-sidebar-mode="explorer"]').click();
    await expect(page.locator(`[data-explorer-row="${repo}/src"]`)).toBeVisible();

    // Expand src and open the file into the preview tab.
    await page.locator(`[data-explorer-row="${repo}/src"]`).click();
    await expect(page.locator(`[data-explorer-row="${repo}/src/main.ts"]`)).toBeVisible();
    await page.locator(`[data-explorer-row="${repo}/src/main.ts"]`).click();
    await expect(page.locator('[data-editor-codemirror] .cm-content')).toContainText("export const answer = 41;");
    return { herdr, daemon, repo, file: path.join(repo, "src/main.ts"), sent, lastSent };
  } catch (error) {
    daemon?.stop();
    herdr.stop();
    throw error;
  }
}

function close(fixture: Fixture): void {
  fixture.daemon.stop();
  fixture.herdr.stop();
}

test.describe.configure({ timeout: 90_000 });

test("the Explorer lists a checkout, expands a folder and opens a preview tab", async ({ page }) => {
  const fixture = await openCheckout(page);
  const { repo, sent, lastSent } = fixture;
  try {
    // The tree lists the root: directories first, hidden names shown, `.git`
    // and the escaping names left out (B1).
    await expect(page.locator("[data-explorer]")).toHaveAttribute("data-explorer", repo);
    await expect(page.locator(`[data-explorer-row="${repo}/.gitignore"]`)).toBeVisible();
    await expect(page.locator(`[data-explorer-row="${repo}/README.md"]`)).toBeVisible();
    await expect(page.locator(`[data-explorer-row="${repo}/tracked.txt"]`)).toBeVisible();
    await expect(page.locator(`[data-explorer-row="${repo}/.git"]`)).toHaveCount(0);
    await screenshot(page, "s3-explorer-root");

    // The core's changed-file set colors the modified row (B1, D-06).
    await expect(page.locator(`[data-explorer-row="${repo}/tracked.txt"]`)).toHaveAttribute("data-decoration", "modified", { timeout: 20_000 });

    // Expanding a folder is one ui_state_update and one file_list; its
    // children appear without a reload (B1, B3).
    expect(lastSent.get("file_list")).toMatchObject({ root: repo, path: `${repo}/src` });
    await expect.poll(() => sent.get("ui_state_update")).toBeGreaterThanOrEqual(2);
    await screenshot(page, "s3-explorer-expanded");

    // A single click opened the checkout's preview tab (B3).
    const fileTab = page.locator('[data-tab-kind="file"]');
    await expect(fileTab).toHaveCount(1);
    await expect(fileTab).toHaveAttribute("data-preview", "true");
    expect(lastSent.get("file_open")).toMatchObject({ path: `${repo}/src/main.ts`, preview: true });
    await screenshot(page, "s3-preview-tab");

    // ⌘⇧K promotes the preview to an ordinary tab (B3).
    await page.keyboard.press("Meta+Shift+KeyK");
    await expect.poll(() => sent.get("file_keep_open")).toBe(1);
    await expect(fileTab).toHaveAttribute("data-preview", "false");

    // Closing the tab is one file_close and the terminal canvas returns.
    await fileTab.hover();
    await fileTab.locator("button").click();
    await expect.poll(() => sent.get("file_close")).toBe(1);
    await expect(page.locator('[data-tab-kind="file"]')).toHaveCount(0);
    await expect(page.locator("[data-canvas]")).toBeVisible();
  } finally {
    close(fixture);
  }
});

test("editing a document marks it dirty, saves it, and a disk change asks how to resolve", async ({ page }) => {
  const fixture = await openCheckout(page);
  const { file, sent, lastSent } = fixture;
  try {
    const content = page.locator('[data-editor-codemirror] .cm-content');
    await content.click();
    await page.keyboard.press("Meta+KeyA");
    await page.keyboard.type("export const answer = 42;\n");

    // The edit is one file_draft and the tab shows the core's dirty badge (B4).
    await expect.poll(() => sent.get("file_draft")).toBeGreaterThanOrEqual(1);
    expect(lastSent.get("file_draft")).toMatchObject({ contents_utf8: "export const answer = 42;\n" });
    await expect(page.locator('[data-editor-dirty="true"]')).toBeVisible();

    // ⌘S is one file_save with the open document's timestamp, and the disk
    // now holds the draft (B4).
    await page.keyboard.press("Meta+KeyS");
    await expect.poll(() => sent.get("file_save")).toBe(1);
    expect(lastSent.get("file_save")).toMatchObject({ path: file, contents_utf8: "export const answer = 42;\n" });
    await expect.poll(() => fs.readFileSync(file, "utf8")).toBe("export const answer = 42;\n");
    await expect(page.locator('[data-editor-dirty="true"]')).toHaveCount(0);
    await screenshot(page, "s3-editor-saved");

    // A change on disk after the open makes the next save a conflict, and the
    // tab offers the two choices (B5).
    fs.writeFileSync(file, "export const answer = 0;\n");
    const ahead = new Date(Date.now() + 5_000);
    fs.utimesSync(file, ahead, ahead);
    await content.click();
    await page.keyboard.press("Meta+KeyA");
    await page.keyboard.type("export const answer = 43;\n");
    await page.keyboard.press("Meta+KeyS");
    await expect(page.locator("[data-editor-conflict]")).toBeVisible();
    await expect.poll(() => fs.readFileSync(file, "utf8")).toBe("export const answer = 0;\n");
    await screenshot(page, "s3-editor-conflict");

    // Reload takes the disk contents into the editor.
    await page.locator('[data-conflict-action="reload"]').click();
    await expect(page.locator("[data-editor-conflict]")).toHaveCount(0);
    await expect(content).toContainText("export const answer = 0;");
    await expect.poll(() => sent.get("file_conflict")).toBe(1);
    expect(lastSent.get("file_conflict")).toMatchObject({ action: "reload" });
  } finally {
    close(fixture);
  }
});

test("images, PDFs and videos render from hided file bytes", async ({ page }) => {
  const fixture = await openCheckout(page);
  const { repo, sent } = fixture;
  try {
    // An image decodes from the bytes hided streamed.
    await page.locator(`[data-explorer-row="${repo}/shot.png"]`).click();
    const image = page.locator('[data-viewer="image"] img');
    await expect(image).toBeVisible();
    await expect.poll(async () => image.evaluate((node) => (node as HTMLImageElement).naturalWidth)).toBeGreaterThan(0);
    await expect.poll(() => sent.get("file_bytes")).toBe(1);
    await screenshot(page, "s3-viewer-image");

    // A PDF renders one canvas page through pdf.js.
    await page.locator(`[data-explorer-row="${repo}/doc.pdf"]`).click();
    await expect(page.locator('[data-viewer="pdf"]')).toBeVisible();
    await expect(page.locator('[data-pdf-page="1"]')).toBeVisible({ timeout: 20_000 });
    await screenshot(page, "s3-viewer-pdf");

    // A video plays from its bytes, and seeking moves the playhead (B7).
    await page.locator(`[data-explorer-row="${repo}/clip.mp4"]`).click();
    const video = page.locator('[data-viewer="video"] video');
    await expect(video).toBeVisible();
    await expect.poll(async () => video.evaluate((node) => (node as HTMLVideoElement).duration), { timeout: 20_000 }).toBeGreaterThan(0);
    await video.evaluate((node) => {
      (node as HTMLVideoElement).currentTime = 0.5;
    });
    await expect.poll(async () => video.evaluate((node) => (node as HTMLVideoElement).currentTime)).toBeGreaterThan(0.4);
    await screenshot(page, "s3-viewer-video");
  } finally {
    close(fixture);
  }
});
