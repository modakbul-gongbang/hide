import type { Page } from "@playwright/test";

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
 * A window of time in which something must not happen, or a pause the machine
 * imposes and the page cannot report (the macOS compositor presenting a frame
 * before a native capture, an idle measurement interval).
 *
 * Absence has no state to wait for, so this is the one place a spec sleeps.
 * `why` names what must stay true or what is being waited out, so the call
 * reads as a claim and a reviewer can judge the length.
 * Never use it to wait for something to appear: wait for the state instead.
 */
export async function quietFor(page: Page, milliseconds: number, why: string): Promise<void> {
  void why;
  // eslint-disable-next-line playwright/no-wait-for-timeout -- the only sleep a spec may use, see above
  await page.waitForTimeout(milliseconds);
}
