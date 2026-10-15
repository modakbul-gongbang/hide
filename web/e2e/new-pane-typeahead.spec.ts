// Keys typed into a split before its shell's line editor reads (PRD
// instant-pane-topology D-11): the core holds them until the new pane's shell
// reads keys itself, so the kernel's line mode never echoes them or erases a
// multi-byte character one byte at a time. The new shell is held at a gate in
// its `.zshrc` until the keys are typed, so the order is the test's, not the
// scheduler's (docs/TESTING.md, The test decides the order).

import { expect, test } from "@playwright/test";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { chord } from "./chords";
import { startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { enterWorkspace } from "./wire";

/** What `pane` shows now, as text. */
function visible(herdr: HerdrFixture, pane: string): string {
  const read = spawnSync(herdr.bin, ["pane", "read", pane, "--source", "visible", "--format", "text"], { env: herdr.env, encoding: "utf8", timeout: 10_000 });
  if (read.status !== 0) throw new Error(`pane read ${pane} failed: ${read.stderr}`);
  return read.stdout;
}

test("keys typed into a split before its shell reads arrive whole", async ({ page }) => {
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | undefined;
  try {
    // A shell started from now on waits in its startup file until `release`
    // exists, with its terminal still in the kernel's line mode.
    const home = herdr.env.HOME!;
    const release = path.join(home, "release");
    const rc = path.join(home, ".zshrc");
    fs.writeFileSync(rc, `while [[ ! -e ${release} ]]; do sleep 0.05; done\n${fs.readFileSync(rc, "utf8")}`);

    daemon = await startHided(herdr, "new-pane-typeahead");
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    const [first] = herdr.panes;
    const origin = page.locator(`[data-pane-view="${first}"]`);
    await expect(origin).toHaveAttribute("data-transport", "controlling");
    await origin.locator(".xterm-helper-textarea").focus();
    const paneIds = () => page.locator("[data-pane-view]").evaluateAll((views) => views.map((view) => view.getAttribute("data-pane-view")!));
    const before = await paneIds();

    // Split, then type at once: a syllable typed with the Korean input
    // source on, erased, and the command meant all along.
    await page.keyboard.press(chord("split_right"));
    await page.keyboard.insertText("ㅊ");
    await page.keyboard.press("Backspace");
    await page.keyboard.press("Backspace");
    await page.keyboard.type("cla");
    await expect(page.locator("[data-pane-view]")).toHaveCount(before.length + 1);
    const created = (await paneIds()).filter((id) => !before.includes(id));
    expect(created).toHaveLength(1);
    const split = created[0]!;
    // The shell is released once the keys' fate is decided while it is still
    // held: the core lists their request as waiting for the new pane's shell,
    // or the keys already reached its terminal in the kernel's line mode,
    // which echoes them. Both are seen soon after Herdr names the pane, well
    // inside the wait's two seconds, and neither depends on when the pane's
    // terminal session opens.
    const waitsForShell = () =>
      page.evaluate((pane) => window.__hideProbe!.inputRequests().some((request) => request.state === "pending" && request.held_for === pane), split);
    await expect.poll(async () => (await waitsForShell()) || visible(herdr, split).includes("cla"), { timeout: 20_000 }).toBe(true);
    fs.writeFileSync(release, "");

    // The line editor took the keys: the prompt holds the command, with no
    // broken character and no echo of it on a line of its own above.
    await expect.poll(() => visible(herdr, split), { timeout: 20_000 }).toMatch(/^fixture % cla\s*$/m);
    const screen = visible(herdr, split);
    expect(screen).not.toContain("<ffffffff>");
    expect(screen.split("\n").filter((line) => line.includes("cla"))).toEqual([expect.stringMatching(/^fixture % cla\s*$/)]);
  } finally {
    try {
      daemon?.stop();
    } finally {
      herdr.stop();
    }
  }
});
