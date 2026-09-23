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
  execFileSync("git", ["-c", "user.email=e2e@example.com", "-c", "user.name=e2e", "-c", "commit.gpgsign=false", "commit", "-qm", "init"], { cwd: dir });
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
    fs.writeFileSync(
      path.join(repoDir, "notes.md"),
      "# Title\n\n- one\n- two\n\n- [ ] open\n\nsee [text](https://example.com) now\n\n```rust\nfn main() {}\n```\n",
    );
    // A block longer than the pane's eight lines, so the pane scrolls inside
    // itself rather than pushing the body down (D-11).
    fs.writeFileSync(
      path.join(repoDir, "post.md"),
      `---\ntitle: Post\n${Array.from({ length: 12 }, (_, index) => `key${index}: value`).join("\n")}\n---\n\n# Body\n`,
    );
    fs.writeFileSync(path.join(repoDir, ".gitignore"), "node_modules\n");
    fs.writeFileSync(path.join(repoDir, "shot.png"), PNG);
    fs.writeFileSync(path.join(repoDir, "doc.pdf"), minimalPdf());
    fs.copyFileSync(path.resolve("e2e/fixtures/tiny.mp4"), path.join(repoDir, "clip.mp4"));
    gitFixture(repoDir);
    // Sparse and untracked: past the editable cap, so the editor offers the
    // host OS handler instead of a buffer (D-12).
    fs.writeFileSync(path.join(repoDir, "huge.txt"), "");
    fs.truncateSync(path.join(repoDir, "huge.txt"), 17 * 1024 * 1024);
    const repo = fs.realpathSync(repoDir);
    herdr.run([
      "workspace", "create", "--cwd", repoDir, "--label", "repo",
      "--env", `PATH=${herdr.fixturePath}`, "--no-focus",
    ]);

    daemon = await startHided(herdr, "s3");
    const lastSent = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, lastSent);
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);

    // Focus the repository checkout, then show the right panel's Explorer,
    // which is where the tree lives now (D-13).
    await page.locator('[data-sidebar-mode="projects"]').click();
    const row = page.locator("[data-project]", { hasText: "repo" }).locator("[data-checkout]").first();
    await row.click();
    await expect(row).toHaveAttribute("aria-current", "true");
    await expect(page.locator('[data-right-panel="explorer"]')).toHaveCount(0);
    await page.keyboard.press("Meta+Shift+KeyB");
    await expect(page.locator('[data-right-panel="explorer"]')).toBeVisible();
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

    // Autosave: no chord, and the draft lands on disk after the idle delay,
    // with the tab clean again (D-10).
    await expect.poll(() => sent.get("file_save"), { timeout: 5_000 }).toBeGreaterThanOrEqual(1);
    expect(lastSent.get("file_save")).toMatchObject({ path: file, contents_utf8: "export const answer = 42;\n" });
    await expect.poll(() => fs.readFileSync(file, "utf8")).toBe("export const answer = 42;\n");
    await expect(page.locator('[data-editor-dirty="true"]')).toHaveCount(0);
    await screenshot(page, "s3-editor-saved");

    // ⌘S still saves at once.
    const savesBeforeChord = sent.get("file_save") ?? 0;
    await page.keyboard.press("Meta+KeyS");
    await expect.poll(() => sent.get("file_save")).toBe(savesBeforeChord + 1);

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

test("a refused save keeps the tab dirty and takes the saving mark off", async ({ page }) => {
  const fixture = await openCheckout(page);
  const { file, sent } = fixture;
  try {
    const content = page.locator('[data-editor-codemirror] .cm-content');
    await content.click();
    await page.keyboard.press("Meta+KeyA");
    await page.keyboard.type("export const answer = 42;\n");
    await expect.poll(() => sent.get("file_save"), { timeout: 5_000 }).toBeGreaterThanOrEqual(1);
    await expect.poll(() => fs.readFileSync(file, "utf8")).toBe("export const answer = 42;\n");

    // A file the process can no longer write: the next autosave is refused, so
    // the tab stops saying it is saving and stays dirty, with the detail in
    // the diagnostic log (B5, D-10).
    fs.chmodSync(file, 0o444);
    const savesBefore = sent.get("file_save") ?? 0;
    await content.click();
    await page.keyboard.press("Meta+KeyA");
    await page.keyboard.type("export const answer = 43;\n");
    await expect.poll(() => sent.get("file_save"), { timeout: 5_000 }).toBeGreaterThan(savesBefore);
    const tab = page.locator('[data-tab-kind="file"]');
    await expect(tab).toHaveAttribute("data-saving", "false", { timeout: 10_000 });
    await expect(page.locator('[data-editor-dirty="true"]')).toBeVisible();
    await expect.poll(() => fs.readFileSync(file, "utf8")).toBe("export const answer = 42;\n");
  } finally {
    fs.chmodSync(file, 0o644);
    close(fixture);
  }
});

test("leaving a dirty tab saves it and never writes it into the next file", async ({ page }) => {
  const fixture = await openCheckout(page);
  const { repo, file } = fixture;
  const readme = path.join(repo, "README.md");
  const readmeBefore = fs.readFileSync(readme, "utf8");
  try {
    const content = page.locator('[data-editor-body] .cm-content');
    await content.click();
    await page.keyboard.press("Meta+KeyA");
    await page.keyboard.type("export const answer = 99;\n");

    // Leave within the idle window: the draft still reaches its own file, and
    // the tab that takes the screen never receives it (D-10, D-14).
    await page.locator(`[data-explorer-row="${repo}/README.md"]`).click();
    await expect.poll(() => fs.readFileSync(file, "utf8"), { timeout: 10_000 }).toBe("export const answer = 99;\n");
    expect(fs.readFileSync(readme, "utf8")).toBe(readmeBefore);
    await expect(page.locator('[data-editor-body] .cm-content')).not.toContainText("answer = 99");
  } finally {
    close(fixture);
  }
});

test("an undone edit is what the next save writes", async ({ page }) => {
  const fixture = await openCheckout(page);
  const { file, sent } = fixture;
  try {
    const content = page.locator('[data-editor-body] .cm-content');
    await content.click();
    await page.keyboard.press("Meta+ArrowDown");
    await page.keyboard.type("X");
    await expect.poll(() => fs.readFileSync(file, "utf8"), { timeout: 10_000 }).toBe(`${SOURCE}X`);

    // Undo that keystroke: the core hears it, so the save that follows writes
    // the text the editor shows rather than the text that was undone (B4).
    await page.keyboard.press("Meta+KeyZ");
    await expect(content).not.toContainText("X");
    const savesBefore = sent.get("file_save") ?? 0;
    await page.keyboard.press("Meta+KeyS");
    await expect.poll(() => sent.get("file_save")).toBeGreaterThan(savesBefore);
    await expect.poll(() => fs.readFileSync(file, "utf8")).toBe(SOURCE);
    await expect(page.locator('[data-editor-dirty="true"]')).toHaveCount(0);
  } finally {
    close(fixture);
  }
});

test("a find request does not follow into the next document", async ({ page }) => {
  const fixture = await openCheckout(page);
  const { repo } = fixture;
  try {
    await page.locator('[data-editor-body] .cm-content').click();
    await page.keyboard.press("Meta+KeyF");
    await expect(page.locator(".cm-search")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.locator(".cm-search")).toHaveCount(0);

    // The next document opens with the document, not with the last find bar.
    await page.locator(`[data-explorer-row="${repo}/README.md"]`).click();
    await expect(page.locator('[data-editor-body] .cm-content')).toContainText("repo");
    await expect(page.locator(".cm-search")).toHaveCount(0);
  } finally {
    close(fixture);
  }
});

test("closing a dirty tab saves the draft instead of dropping it", async ({ page }) => {
  const fixture = await openCheckout(page);
  const { file, lastSent } = fixture;
  try {
    // One keystroke, then the close: the idle timer cannot have saved it yet,
    // so the draft can only reach the disk through the close itself.
    const content = page.locator('[data-editor-body] .cm-content');
    await content.click();
    await page.keyboard.press("Meta+ArrowDown");
    await page.keyboard.type("X");

    const tab = page.locator('[data-tab-kind="file"]');
    await tab.hover();
    await tab.locator('button[aria-label^="Close tab"]').click();
    await expect
      .poll(() => (lastSent.get("file_close")?.pending_save as { contents_utf8?: string } | null)?.contents_utf8)
      .toBe(`${SOURCE}X`);
    await expect(page.locator('[data-tab-kind="file"]')).toHaveCount(0);
    await expect.poll(() => fs.readFileSync(file, "utf8"), { timeout: 10_000 }).toBe(`${SOURCE}X`);
  } finally {
    close(fixture);
  }
});

test("a closed tab's draft is not written back when the file is reopened", async ({ page }) => {
  const fixture = await openCheckout(page);
  const { repo, file, sent } = fixture;
  try {
    const content = page.locator('[data-editor-body] .cm-content');
    await content.click();
    await page.keyboard.press("Meta+KeyA");
    await page.keyboard.type("export const answer = 5;\n");
    const tab = page.locator('[data-tab-kind="file"]');
    await tab.hover();
    await tab.locator('button[aria-label^="Close tab"]').click();
    await expect(page.locator('[data-tab-kind="file"]')).toHaveCount(0);
    await expect.poll(() => fs.readFileSync(file, "utf8"), { timeout: 10_000 }).toBe("export const answer = 5;\n");

    // An agent rewrites the file, the operator reopens it, and saves with
    // nothing typed: the closed tab's draft must not come back with it (B5).
    fs.writeFileSync(file, "export const answer = 0;\n");
    await page.locator(`[data-explorer-row="${repo}/src/main.ts"]`).click();
    await expect(content).toContainText("answer = 0");
    const savesBefore = sent.get("file_save") ?? 0;
    await page.keyboard.press("Meta+KeyS");
    await expect.poll(() => sent.get("file_save")).toBeGreaterThan(savesBefore);
    await expect.poll(() => fs.readFileSync(file, "utf8")).toBe("export const answer = 0;\n");
  } finally {
    close(fixture);
  }
});

test("a conflicted background tab is not closed away with its draft", async ({ page }) => {
  const fixture = await openCheckout(page);
  const { repo, file, sent } = fixture;
  const readme = path.join(repo, "README.md");
  try {
    // Make the first tab conflicted, then move to a second tab so the
    // conflicted one is no longer the showing document.
    const content = page.locator('[data-editor-body] .cm-content');
    await content.click();
    await page.keyboard.press("Meta+KeyA");
    await page.keyboard.type("export const answer = 8;\n");
    fs.writeFileSync(file, "export const answer = 0;\n");
    const ahead = new Date(Date.now() + 5_000);
    fs.utimesSync(file, ahead, ahead);
    await page.keyboard.press("Meta+KeyS");
    await expect(page.locator("[data-editor-conflict]")).toBeVisible();

    await page.locator(`[data-explorer-row="${repo}/README.md"]`).click();
    await page.locator('[data-editor-body] .cm-content').click();
    await page.keyboard.type("edit");

    // Closing the conflicted tab from the strip must not discard its draft:
    // the close-save is refused, so the tab stays with the conflict showing.
    const conflicted = page.locator(`[data-tab-kind="file"]`, { hasText: "main.ts" });
    await conflicted.hover();
    await conflicted.locator('button[aria-label^="Close tab"]').click();
    await expect.poll(() => sent.get("file_close")).toBe(1);
    await expect(page.locator(`[data-tab-kind="file"]`, { hasText: "main.ts" })).toHaveCount(1);
    await expect.poll(() => fs.readFileSync(file, "utf8")).toBe("export const answer = 0;\n");

    // The refused close kept the tab, the draft and the choice to resolve it.
    await page.locator(`[data-tab-kind="file"]`, { hasText: "main.ts" }).click();
    await expect(page.locator("[data-editor-conflict]")).toBeVisible();
    await expect(page.locator('[data-editor-body] .cm-content')).toContainText("answer = 8");
    // The other tab kept its own edit and never received this one.
    expect(fs.readFileSync(readme, "utf8")).not.toContain("answer = 8");
  } finally {
    close(fixture);
  }
});

test("the close chord closes the file tab that is showing", async ({ page }) => {
  const fixture = await openCheckout(page);
  const { sent } = fixture;
  try {
    await page.locator('[data-editor-body] .cm-content').click();
    await page.keyboard.press("Alt+KeyW");
    await expect.poll(() => sent.get("file_close")).toBe(1);
    await expect(page.locator('[data-tab-kind="file"]')).toHaveCount(0);
    // The terminal tab the file tab was covering is still there.
    await expect(page.locator('[data-tab-kind="herdr"]').first()).toBeVisible();
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

test("Markdown opens in Live and toggles to source", async ({ page }) => {
  const fixture = await openCheckout(page);
  const { repo, sent } = fixture;
  try {
    await page.locator(`[data-explorer-row="${repo}/notes.md"]`).click();
    const content = page.locator('[data-editor-codemirror] .cm-content');
    await expect(page.locator('[data-markdown-mode="live"]')).toBeVisible();
    // The caret opens at the top; move it to the end so the first block is
    // not the touched one and its markup is hidden (B6).
    await content.click();
    await page.keyboard.press("Meta+ArrowDown");

    // Live draws the heading and hides its hashes, draws the list bullets and
    // the task checkbox, styles the link text and the fenced code (B6).
    await expect(page.locator(".cm-md-heading-1")).toHaveCount(1, { timeout: 20_000 });
    await expect(page.locator(".cm-md-heading-1")).toHaveText("Title");
    await expect(page.locator(".cm-md-bullet")).toHaveCount(3);
    await expect(page.locator(".cm-md-checkbox")).toHaveCount(1);
    await expect(page.locator(".cm-md-link")).toHaveText("text");
    await expect(page.locator(".cm-md-code")).toHaveCount(1);
    await expect(page.locator(".cm-md-hidden-line")).toHaveCount(2);
    await screenshot(page, "s3-markdown-live");

    // Live hides markup from the DOM, not from the document: Source mode
    // shows the source again, unchanged.
    await page.locator('[data-markdown-mode="live"]').click();
    await expect.poll(() => sent.get("file_view")).toBe(1);
    await expect(page.locator('[data-markdown-mode="source"]')).toBeVisible();
    await expect(page.locator(".cm-md-heading-1")).toHaveCount(0);
    await expect(page.locator(".cm-md-bullet")).toHaveCount(0);
    await expect(content).toContainText("# Title");
    await expect(content).toContainText("[text](https://example.com)");

    // Back to Live is one more file_view.
    await page.locator('[data-markdown-mode="source"]').click();
    await expect.poll(() => sent.get("file_view")).toBe(2);
    await expect(page.locator(".cm-md-heading-1")).toHaveCount(1);
  } finally {
    close(fixture);
  }
});

test("Markdown Live draws frontmatter in its own scrolling pane", async ({ page }) => {
  const fixture = await openCheckout(page);
  const { repo, lastSent } = fixture;
  try {
    await page.locator(`[data-explorer-row="${repo}/post.md"]`).click();
    await expect(page.locator('[data-markdown-mode="live"]')).toBeVisible();
    const pane = page.locator("[data-editor-frontmatter]");
    const body = page.locator("[data-editor-body] .cm-content");
    await expect(pane.locator(".cm-content")).toContainText("title: Post");
    // The block left the body: it is drawn once, in the pane (D-11).
    await expect(body).not.toContainText("title: Post");
    // Live hides the heading's hash when the caret is elsewhere, so the body
    // reads as its text.
    await expect(body).toContainText("Body");

    // The pane is about eight lines tall and scrolls inside itself, so the
    // body below it does not move with the block (D-11).
    const measured = await pane.evaluate((element) => ({
      client: element.clientHeight,
      scroll: element.querySelector(".cm-scroller")?.scrollHeight ?? 0,
    }));
    const lineHeight = await pane.evaluate(() => Number.parseFloat(getComputedStyle(document.querySelector("[data-editor-frontmatter] .cm-content")!).fontSize) * 1.5);
    expect(measured.scroll).toBeGreaterThan(measured.client);
    expect(measured.client).toBeGreaterThan(lineHeight * 6);
    expect(measured.client).toBeLessThan(lineHeight * 9);

    // Editing the pane edits the document: the draft carries the whole file,
    // block first (D-11, B4).
    await pane.locator(".cm-content").click();
    await page.keyboard.type("x");
    await expect.poll(() => lastSent.get("file_draft")?.contents_utf8 ?? "").toContain("x");
    const draft = String(lastSent.get("file_draft")?.contents_utf8 ?? "");
    expect(draft.startsWith("---\n")).toBe(true);
    expect(draft).toContain("# Body");

    // Source mode is unchanged: one buffer, the block inline (D-11).
    await page.locator('[data-markdown-mode="live"]').click();
    await expect(page.locator("[data-editor-frontmatter]")).toHaveCount(0);
    await expect(page.locator("[data-editor-codemirror] .cm-content")).toContainText("title: Post");

    // And Live brings the pane back.
    await page.locator('[data-markdown-mode="source"]').click();
    await expect(page.locator("[data-editor-frontmatter]")).toBeVisible();
    await expect(page.locator("[data-editor-body] .cm-content")).not.toContainText("title: Post");
  } finally {
    close(fixture);
  }
});

/** A drag of HTML5 `draggable` rows, which Playwright's mouse drag does not
 * drive: the source's dragstart carries a DataTransfer to the folder's drop. */
async function dragRow(page: Page, from: string, to: string): Promise<void> {
  await page.evaluate(
    ([fromPath, toPath]) => {
      const source = document.querySelector(`[data-explorer-row="${fromPath}"]`);
      const target = document.querySelector(`[data-explorer-row="${toPath}"]`);
      if (!source || !target) throw new Error(`drag ${fromPath} -> ${toPath}: rows not found`);
      const data = new DataTransfer();
      source.dispatchEvent(new DragEvent("dragstart", { bubbles: true, dataTransfer: data }));
      target.dispatchEvent(new DragEvent("dragover", { bubbles: true, cancelable: true, dataTransfer: data }));
      target.dispatchEvent(new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: data }));
      source.dispatchEvent(new DragEvent("dragend", { bubbles: true, dataTransfer: data }));
    },
    [from, to],
  );
}

test("the Explorer creates, renames, moves and trashes entries", async ({ page }) => {
  const fixture = await openCheckout(page);
  const { repo, sent, lastSent } = fixture;
  try {
    // New File in `src`: one file_create, and the core opens the created file.
    await page.locator(`[data-explorer-row="${repo}/src"]`).click({ button: "right" });
    await expect(page.locator("[data-explorer-menu]")).toBeVisible();
    await page.locator('[data-menu-item="new-file"]').click();
    const create = page.locator('[data-explorer-draft="create"] input');
    await expect(create).toBeVisible();
    await create.fill("added.ts");
    await create.press("Enter");
    await expect.poll(() => sent.get("file_create")).toBe(1);
    expect(lastSent.get("file_create")).toMatchObject({ root: repo, parent: `${repo}/src`, name: "added.ts" });
    await expect(page.locator(`[data-explorer-row="${repo}/src/added.ts"]`)).toBeVisible();
    // The created file opened as a tab beside the one already open.
    await expect(page.locator('[data-tab-kind="file"]').filter({ hasText: "added.ts" })).toHaveCount(1);
    await screenshot(page, "s3-explorer-created");

    // A name that already exists fails and says so under the row it started
    // from; nothing is created.
    await page.locator(`[data-explorer-row="${repo}/src"]`).click({ button: "right" });
    await page.locator('[data-menu-item="new-file"]').click();
    const clash = page.locator('[data-explorer-draft="create"] input');
    await clash.fill("main.ts");
    await clash.press("Enter");
    await expect(page.locator(`[data-explorer-failure="${repo}/src/main.ts"]`)).toBeVisible({ timeout: 20_000 });
    await expect(page.locator(`[data-explorer-row="${repo}/src/main.ts"]`)).toHaveCount(1);
    await screenshot(page, "s3-explorer-failure");

    // Rename: one path_rename, and the row takes the new name.
    await page.locator(`[data-explorer-row="${repo}/src/added.ts"]`).click({ button: "right" });
    await page.locator('[data-menu-item="rename"]').click();
    const rename = page.locator('[data-explorer-draft="rename"] input');
    await rename.fill("renamed.ts");
    await rename.press("Enter");
    await expect.poll(() => sent.get("path_rename")).toBe(1);
    expect(lastSent.get("path_rename")).toMatchObject({ root: repo, path: `${repo}/src/added.ts`, name: "renamed.ts" });
    await expect(page.locator(`[data-explorer-row="${repo}/src/renamed.ts"]`)).toBeVisible();

    // New Folder in the root, then drag the file onto it: one path_move.
    await page.locator("[data-explorer-tree]").click({ button: "right", position: { x: 20, y: 400 } });
    await page.locator('[data-menu-item="new-folder"]').click();
    const folder = page.locator('[data-explorer-draft="create"] input');
    await folder.fill("dest");
    await folder.press("Enter");
    await expect.poll(() => sent.get("dir_create")).toBe(1);
    await expect(page.locator(`[data-explorer-row="${repo}/dest"]`)).toBeVisible();
    await dragRow(page, `${repo}/src/renamed.ts`, `${repo}/dest`);
    await expect.poll(() => sent.get("path_move")).toBe(1);
    expect(lastSent.get("path_move")).toMatchObject({ root: repo, path: `${repo}/src/renamed.ts`, destination: `${repo}/dest` });

    // Trash behind the confirmation: nothing goes out until it is confirmed.
    await page.locator(`[data-explorer-row="${repo}/dest"]`).click({ button: "right" });
    await page.locator('[data-menu-item="trash"]').click();
    const dialog = page.locator("[data-confirm-trash]");
    await expect(dialog).toBeVisible();
    expect(sent.get("path_trash") ?? 0).toBe(0);
    await screenshot(page, "s3-explorer-trash");
    await page.locator("[data-trash-confirm]").click();
    await expect.poll(() => sent.get("path_trash")).toBe(1);
    // The tree selects the removed folder's next sibling, which is `src`.
    expect(lastSent.get("path_trash")).toMatchObject({ root: repo, path: `${repo}/dest`, select_after: `${repo}/src` });
    await expect(page.locator(`[data-explorer-row="${repo}/dest"]`)).toHaveCount(0);
  } finally {
    close(fixture);
  }
});

test("a file past the editing cap offers the default app instead of an editor", async ({ page }) => {
  const fixture = await openCheckout(page);
  const { repo, sent } = fixture;
  try {
    await page.locator(`[data-explorer-row="${repo}/huge.txt"]`).click();
    const body = page.locator("[data-editor-preview-only]");
    await expect(body).toBeVisible();
    await expect(body).toContainText("preview-only");
    await screenshot(page, "s3-preview-only");

    await page.locator("[data-editor-open-external]").click();
    await expect.poll(() => sent.get("open_external")).toBe(1);
    await expect(page.locator("[data-editor-open-failed]")).toHaveCount(0);
  } finally {
    close(fixture);
  }
});

test("a preview-only document closes without a save", async ({ page }) => {
  const fixture = await openCheckout(page);
  const { repo, sent, lastSent } = fixture;
  try {
    // A stale recovery buffer is exactly the trap: a preview-only document has
    // no draft the core would accept, so the restore must decline it rather
    // than hand it back as a close-save the core refuses (D-14).
    const stale = `${repo}\u0000${repo}/huge.txt`;
    await page.evaluate(
      async ({ root, path, id }) => {
        await new Promise<void>((resolve, reject) => {
          const request = indexedDB.open("hide-shell", 2);
          request.onupgradeneeded = () => {
            const database = request.result;
            if (database.objectStoreNames.contains("buffers")) database.deleteObjectStore("buffers");
            database.createObjectStore("buffers", { keyPath: "id" });
          };
          request.onerror = () => reject(request.error);
          request.onsuccess = () => {
            const database = request.result;
            const transaction = database.transaction("buffers", "readwrite");
            transaction.objectStore("buffers").put({
              id,
              root,
              path,
              contents: "stale draft\n",
              updated_at: Date.now(),
            });
            transaction.oncomplete = () => {
              database.close();
              resolve();
            };
            transaction.onerror = () => reject(transaction.error);
          };
        });
      },
      { root: repo, path: `${repo}/huge.txt`, id: stale },
    );
    await page.locator(`[data-explorer-row="${repo}/huge.txt"]`).click();
    await expect(page.locator("[data-editor-preview-only]")).toBeVisible();
    // The stale buffer was declined rather than planted as a draft.
    await expect.poll(() => sent.get("file_draft") ?? 0, { timeout: 2_000 }).toBe(0);

    // No draft the core would accept exists here, so the close is one step.
    await page.locator('[data-tab-kind="file"]').hover();
    await page.locator('[data-tab-kind="file"] button[aria-label^="Close tab"]').click();
    await expect(page.locator('[data-tab-kind="file"]')).toHaveCount(0);
    await expect.poll(() => sent.get("file_close")).toBe(1);
    expect(lastSent.get("file_close")?.pending_save ?? null).toBeNull();
  } finally {
    close(fixture);
  }
});

test("a change in an expanded folder refreshes the tree without a reload", async ({ page }) => {
  const fixture = await openCheckout(page);
  const { repo } = fixture;
  try {
    // `src` is expanded and watched; a file written into it appears on its own.
    // The watcher arms a moment after the ui state reaches the daemon, so a
    // missed first write is retried with a fresh name rather than assumed.
    await expect(page.locator(`[data-explorer-row="${repo}/src/main.ts"]`)).toBeVisible();
    let created = "";
    await expect(async () => {
      created = `watched-${Date.now()}.ts`;
      fs.writeFileSync(path.join(repo, "src", created), "export const watched = true;\n");
      await expect(page.locator(`[data-explorer-row="${repo}/src/${created}"]`)).toBeVisible({ timeout: 1_500 });
    }).toPass({ timeout: 30_000 });
    await screenshot(page, "s3-watch-refresh");

    // Deleting it disappears the same way.
    fs.rmSync(path.join(repo, "src", created));
    await expect(page.locator(`[data-explorer-row="${repo}/src/${created}"]`)).toHaveCount(0, { timeout: 15_000 });
  } finally {
    close(fixture);
  }
});

test("⌘P opens a file by name and ⌘K switches checkout", async ({ page }) => {
  const fixture = await openCheckout(page);
  const { repo, sent } = fixture;
  try {
    // Collapse the folder first: the palette open must reveal the document's
    // row again through the core's expanded set (B3).
    await page.locator(`[data-explorer-row="${repo}/src"]`).click();
    await expect(page.locator(`[data-explorer-row="${repo}/src/main.ts"]`)).toHaveCount(0);

    // ⌘P: hided indexes the checkout, ranks the typed name and opens it in the
    // preview tab (B12).
    await page.keyboard.press("Meta+KeyP");
    const input = page.locator('[data-palette="Open file"] [data-palette-input]');
    await expect(input).toBeVisible();
    await input.fill("main");
    const row = page.locator(`[data-palette-row="${repo}/src/main.ts"]`);
    await expect(row).toBeVisible({ timeout: 20_000 });
    await screenshot(page, "s3-palette-files");
    await row.click();
    await expect(page.locator("[data-palette-input]")).toHaveCount(0);
    await expect(page.locator('[data-tab-kind="file"]').filter({ hasText: "main.ts" })).toHaveCount(1);
    await expect(page.locator(`[data-explorer-row="${repo}/src/main.ts"]`)).toHaveAttribute("data-selected", "true", { timeout: 10_000 });
    expect(sent.get("file_index") ?? 0).toBeGreaterThanOrEqual(1);

    // ⌘K: the snapshot's projects and checkouts are searched here, and picking
    // one focuses it (B13).
    const focusBefore = sent.get("focus_checkout") ?? 0;
    await page.keyboard.press("Meta+KeyK");
    const search = page.locator('[data-palette="Search"] [data-palette-input]');
    await expect(search).toBeVisible();
    await search.fill("fixture");
    const projectRow = page.locator("[data-palette-list] button").filter({ hasText: "fixture" }).first();
    await expect(projectRow).toBeVisible();
    await screenshot(page, "s3-palette-search");
    await projectRow.click();
    await expect(page.locator("[data-palette-input]")).toHaveCount(0);
    await expect.poll(() => sent.get("focus_checkout")).toBeGreaterThan(focusBefore);
  } finally {
    close(fixture);
  }
});

test("a dropped file reaches the terminal as an attachment", async ({ page }) => {
  const fixture = await openCheckout(page);
  const { herdr, sent } = fixture;
  try {
    // The fixture checkout's panes run the shim that logs every PTY byte, so
    // the pasted token is observable there; openCheckout focused the repository.
    await page.locator('[data-sidebar-mode="projects"]').click();
    const project = page.locator("[data-project]", { hasText: "fixture" });
    await project.locator("[data-checkout]").first().click();
    await page.locator('[data-tab-kind="herdr"]').first().click();
    const pane = page.locator("[data-pane-view]").first();
    await expect(pane).toBeVisible();

    // A file drop is bytes the browser can read but cannot name; the shell
    // uploads them, and the core pastes the staged path into the PTY (B14).
    await page.evaluate(async (base64) => {
      const binary = Uint8Array.from(atob(base64), (character) => character.charCodeAt(0));
      const file = new File([binary], "dropped.png", { type: "image/png" });
      const data = new DataTransfer();
      data.items.add(file);
      const target = document.querySelector("[data-pane-view]") as HTMLElement;
      target.dispatchEvent(new DragEvent("dragover", { bubbles: true, cancelable: true, dataTransfer: data }));
      target.dispatchEvent(new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: data }));
    }, PNG.toString("base64"));

    await expect.poll(() => sent.get("attachment_stage")).toBe(1);
    await expect.poll(() => sent.get("attachment_commit")).toBe(1);

    // The shim logs every byte its PTY received, so the pasted token shows up.
    const logs = herdr.inputLogs.map((file) => file);
    const read = () => logs.map((file) => (fs.existsSync(file) ? fs.readFileSync(file, "utf8") : "")).join("\n");
    await expect.poll(read, { timeout: 15_000 }).toContain("dropped.png");

    // ⌘V of an image: the same flow, with the image staged at the path the core
    // reads a clipboard attachment from (B14).
    await page.evaluate((base64) => {
      const binary = Uint8Array.from(atob(base64), (character) => character.charCodeAt(0));
      const file = new File([binary], "clipboard.png", { type: "image/png" });
      const data = new DataTransfer();
      data.items.add(file);
      const target = document.querySelector("[data-terminal-host]") as HTMLElement;
      target.dispatchEvent(new ClipboardEvent("paste", { bubbles: true, cancelable: true, clipboardData: data }));
    }, PNG.toString("base64"));
    await expect.poll(read, { timeout: 15_000 }).toContain("TerminalClipboard");
    await expect(page.locator("[data-pane-attachment-refusal]")).toHaveCount(0);
    await screenshot(page, "s3-attachment-drop");
  } finally {
    close(fixture);
  }
});

test("an unsaved edit survives a socket drop and reconnect", async ({ page }) => {
  const fixture = await openCheckout(page);
  const { repo } = fixture;
  try {
    await page.locator(`[data-explorer-row="${repo}/notes.md"]`).click();
    const content = page.locator('[data-editor-codemirror] .cm-content');
    await expect(content).toContainText("# Title");
    await content.click();
    await page.keyboard.press("Meta+ArrowDown");
    await page.keyboard.type("edited across a reconnect\n");
    await expect(page.locator('[data-editor-dirty="true"]')).toBeVisible();

    // A server-side drop makes the shell reconnect on its own; the core keeps
    // the draft, and the buffer reconciles against it (B8). A live connection
    // draws no badge at all.
    await page.evaluate(() => window.__hideProbe?.dropSocket());
    await expect(page.locator("[data-connection]")).toHaveText(/reconnecting/, { timeout: 15_000 });
    await expect(page.locator("[data-connection]")).toHaveCount(0, { timeout: 20_000 });
    await expect(content).toContainText("edited across a reconnect");
    await expect(page.locator('[data-editor-dirty="true"]')).toBeVisible({ timeout: 10_000 });
    await screenshot(page, "s3-buffer-reconnect");
  } finally {
    close(fixture);
  }
});
