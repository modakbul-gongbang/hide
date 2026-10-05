import type { Page } from "@playwright/test";
import fs from "node:fs";

/**
 * Wait for the colour transitions and other finite animations the page has
 * running, then two frames, so a capture shows the state just asserted.
 * Looping animations (a spinner) are not waited for.
 */
export async function animationsFinished(page: Page): Promise<void> {
  await page.evaluate(async () => {
    const finite = document
      .getAnimations()
      .filter((animation) => animation.effect?.getComputedTiming().iterations !== Infinity);
    await Promise.allSettled(finite.map((animation) => animation.finished));
    await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
  });
}

/**
 * Wait until `read()` returns the same value for `frames` animation frames in a row: a state that has stopped moving.
 * Counted in frames, not time, and bounded: it throws after `limit` frames instead of running to the test timeout.
 * Use it for "the burst has finished" before a baseline is taken; a window in which something must not happen is `quietFor`.
 */
export async function unchangedForFrames(page: Page, read: () => unknown, frames = 30, limit = 1_200): Promise<void> {
  let last = JSON.stringify(await read());
  for (let waited = 0, still = 0; still < frames; waited += 1) {
    if (waited >= limit) throw new Error(`the value kept changing for ${limit} animation frames: ${last}`);
    await page.evaluate(() => new Promise<void>((resolve) => requestAnimationFrame(() => resolve())));
    const now = JSON.stringify(await read());
    still = now === last ? still + 1 : 0;
    last = now;
  }
}

async function sleep(page: Page, milliseconds: number): Promise<void> {
  // eslint-disable-next-line playwright/no-wait-for-timeout -- the only sleep a spec may use; the three helpers below name why
  await page.waitForTimeout(milliseconds);
}

/**
 * A window in which something must NOT happen: no event is sent, no row moves, nothing is drawn.
 * Absence has no state to wait for, so the window is time.
 * `why` states the claim, so a reviewer can judge the length.
 * Never use it to wait for something to appear or settle: wait for that state (a data attribute, a size, a diagnostic).
 */
export async function quietFor(page: Page, milliseconds: number, why: string): Promise<void> {
  void why;
  await sleep(page, milliseconds);
}

/**
 * The macOS compositor presents a frame some time after the page reports it, and a native
 * window capture reads what was presented. The page cannot observe that, so a capture waits.
 * Only for the native capture helpers; a DOM assertion never needs it.
 */
export async function compositorPresents(page: Page): Promise<void> {
  await sleep(page, 500);
}

/** An interval a measurement spans on purpose (idle CPU and memory sampling, pacing between latency samples). */
export async function measureFor(page: Page, milliseconds: number, why: string): Promise<void> {
  void why;
  await sleep(page, milliseconds);
}

/**
 * What a document the product saves holds on disk, or null while its name is
 * absent. A Windows save replaces the file with `ReplaceFileW`, which leaves
 * the name empty for a moment (`hide-host/src/save.rs`), so a poll reads that
 * as not yet; any other read error still fails the poll.
 */
export function onDisk(file: string): string | null {
  try {
    return fs.readFileSync(file, "utf8");
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return null;
    throw error;
  }
}
