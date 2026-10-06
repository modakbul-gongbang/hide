import { expect, test, type Page } from "@playwright/test";
import { herdrHasFocus, startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { enterWorkspace } from "./wire";

type Operator = { client_id: string; sequence: number };
type Frame = { type?: string; payload?: { rest?: { focused?: { pane_id?: string; operator_focus?: Operator[] } } } };

/** Two frames on: the passive effect that follows a snapshot's focused pane has run by then. */
const settle = (page: Page) => page.evaluate(() => new Promise<void>((done) => requestAnimationFrame(() => requestAnimationFrame(() => done()))));

/**
 * The page's socket with the daemon's frames held back until the test hands
 * them over, in order. The test decides which snapshot the page has seen when
 * the operator's last click is already sent, so no timing is involved.
 */
async function holdSnapshots(page: Page) {
  const frames: string[] = [];
  const sent: Operator[] = [];
  let holding = false;
  let toPage: ((message: string | Buffer) => void) | undefined;
  await page.routeWebSocket(/\/ws$/, (socket) => {
    const server = socket.connectToServer();
    toPage = (message) => socket.send(message);
    socket.onMessage((message) => {
      if (typeof message === "string") {
        const event = JSON.parse(message) as { kind?: string; payload?: Partial<Operator> };
        if (event.kind === "focus_pane" && event.payload?.client_id) sent.push(event.payload as Operator);
      }
      server.send(message);
    });
    server.onMessage((message) => {
      if (holding) frames.push(message as string);
      else socket.send(message);
    });
  });
  const applied = (frame: string, client: string) =>
    (JSON.parse(frame) as Frame).payload?.rest?.focused?.operator_focus?.find((entry) => entry.client_id === client)?.sequence ?? 0;
  return {
    hold: () => { holding = true; },
    sent: () => sent,
    /** Held frames that already include operator focus number `sequence`. */
    includes: (sequence: number) => frames.filter((frame) => applied(frame, sent[0]?.client_id ?? "") >= sequence).length,
    /** Hands the page every held frame before the first one that includes `sequence`; with none given, all of them. */
    deliver: (sequence?: number) => {
      const client = sent[0]?.client_id ?? "";
      const stop = sequence === undefined ? frames.length : frames.findIndex((frame) => applied(frame, client) >= sequence);
      for (const frame of frames.splice(0, stop === -1 ? frames.length : stop)) toPage?.(frame);
    },
  };
}

test("a snapshot older than the last click does not take the keys back, and the one that includes it keeps them", { tag: "@platform" }, async ({ page }) => {
  const herdr = await startHerdr();
  let daemon: Daemon | undefined;
  try {
    daemon = await startHided(herdr, "stale-snapshot-focus");
    const net = await holdSnapshots(page);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    const panes = herdr.panes.map((pane) => page.locator(`[data-pane-view="${pane}"]`));
    const keys = panes.map((pane) => pane.locator(".xterm-helper-textarea"));
    for (const pane of panes) await expect(pane).toHaveAttribute("data-transport", "controlling");
    await expect.poll(() => herdrHasFocus(herdr, herdr.panes[0])).toBe(true);
    await expect(keys[0]).toBeFocused();
    const boxes = await Promise.all(panes.map((pane) => pane.boundingBox()));
    const click = (index: number) => page.mouse.click(boxes[index]!.x + 100, boxes[index]!.y + 100);

    net.hold();
    await click(1);
    await expect.poll(() => net.sent().length).toBe(1);
    await expect.poll(() => net.includes(1)).toBeGreaterThan(0);
    await click(0);
    await expect.poll(() => net.sent().length).toBe(2);
    await expect.poll(() => net.includes(2)).toBeGreaterThan(0);
    await expect(keys[0]).toBeFocused();

    // The page has sent click 2; the daemon's answers so far include only
    // click 1, which named the other pane. They arrive and the header follows
    // them, but the keys stay where the operator last clicked.
    net.deliver(2);
    await expect(panes[1]).toHaveAttribute("data-focused", "true");
    await settle(page);
    await expect(keys[0]).toBeFocused();
    await expect(keys[1]).not.toBeFocused();

    // The snapshot that includes click 2 agrees with the keys.
    net.deliver();
    await expect(panes[0]).toHaveAttribute("data-focused", "true");
    await settle(page);
    await expect(keys[0]).toBeFocused();
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});

test("a focus move the core followed from Herdr still takes the keys once the clicks are answered", { tag: "@platform" }, async ({ page }) => {
  const herdr = await startHerdr();
  let daemon: Daemon | undefined;
  try {
    daemon = await startHided(herdr, "stale-snapshot-external");
    const net = await holdSnapshots(page);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    const panes = herdr.panes.map((pane) => page.locator(`[data-pane-view="${pane}"]`));
    const keys = panes.map((pane) => pane.locator(".xterm-helper-textarea"));
    for (const pane of panes) await expect(pane).toHaveAttribute("data-transport", "controlling");
    await expect.poll(() => herdrHasFocus(herdr, herdr.panes[0])).toBe(true);
    const box = (await panes[1].boundingBox())!;
    await page.mouse.click(box.x + 100, box.y + 100);
    await expect(keys[1]).toBeFocused();
    await expect(panes[1]).toHaveAttribute("data-focused", "true");
    await expect.poll(() => net.sent().length).toBe(1);
    await expect.poll(() => herdrHasFocus(herdr, herdr.panes[1])).toBe(true);

    herdr.run(["pane", "focus", "--direction", "left", "--pane", herdr.panes[1]]);
    await expect(panes[0]).toHaveAttribute("data-focused", "true");
    await expect(keys[0]).toBeFocused();
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
