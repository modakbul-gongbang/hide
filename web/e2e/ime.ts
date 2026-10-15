import type { Page } from "@playwright/test";

/**
 * Composes Korean text through the browser's input-method path into the
 * focused field, one syllable at a time, then commits it: each step is a
 * marked composition, as an input method shows it, and the last is committed.
 */
export async function composeKorean(page: Page, steps: string[][]): Promise<void> {
  const cdp = await page.context().newCDPSession(page);
  try {
    for (const syllable of steps) {
      for (const marked of syllable) await cdp.send("Input.imeSetComposition", { text: marked, selectionStart: marked.length, selectionEnd: marked.length });
      await cdp.send("Input.insertText", { text: syllable.at(-1) ?? "" });
    }
  } finally {
    await cdp.detach();
  }
}
