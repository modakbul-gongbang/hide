import { expect, test, type Page } from "@playwright/test";
import fs from "node:fs";
import net from "node:net";
import path from "node:path";
import { herdrHasFocus, startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { enterWorkspace, screenshot } from "./wire";

/** Holds one request at the real socket boundary, released by the test's
 * completed user-click burst. Every response still comes from pinned Herdr. */
async function focusGate(herdr: HerdrFixture) {
  const socket = path.join(herdr.root, "focus.sock");
  const peers = new Set<net.Socket>();
  const requests: string[] = [];
  let armed = false;
  let active = 0;
  let maximum = 0;
  let release: (() => Promise<void>) | undefined;
  let held: (() => void) | undefined;
  const server = net.createServer((client) => {
    peers.add(client);
    client.on("close", () => peers.delete(client));
    client.on("error", () => client.destroy());
    let buffer = Buffer.alloc(0);
    const first = (chunk: Buffer) => {
      buffer = Buffer.concat([buffer, chunk]);
      const end = buffer.indexOf(10);
      if (end < 0) return;
      client.off("data", first);
      const request = JSON.parse(buffer.subarray(0, end).toString()) as {
        method: string; params: { pane_id?: string };
      };
      const focus = request.method === "pane.focus";
      if (focus) {
        requests.push(request.params.pane_id!);
        maximum = Math.max(maximum, ++active);
      }
      const forward = (received?: () => void, failed?: (error: Error) => void) => {
        const upstream = net.connect(herdr.socket, () => upstream.write(buffer));
        peers.add(upstream);
        upstream.on("close", () => peers.delete(upstream));
        upstream.on("error", (error) => { client.destroy(); failed?.(error); });
        client.on("close", () => upstream.destroy());
        upstream.once("data", () => {
          if (focus) active -= 1;
          received?.();
          if (client.destroyed) upstream.destroy();
        });
        if (!client.destroyed) client.pipe(upstream).pipe(client);
      };
      if (focus && armed) {
        armed = false;
        release = () => new Promise<void>((resolve, reject) => forward(resolve, reject));
        held?.();
      } else forward();
    };
    client.on("data", first);
  });
  server.maxConnections = 64;
  try {
    // Herdr's terminal CLI uses the separate client socket, not JSON control.
    fs.symlinkSync(herdr.socket.replace(/\.sock$/, "-client.sock"), socket.replace(/\.sock$/, "-client.sock"));
    await new Promise<void>((resolve, reject) => {
      server.once("error", reject);
      server.listen(socket, resolve);
    });
  } catch (error) {
    server.close();
    throw error;
  }
  return {
    socket, requests,
    maximum: () => maximum,
    arm: () => {
      armed = true;
      requests.length = 0;
      return new Promise<void>((resolve) => { held = resolve; });
    },
    release: async () => { const forward = release; release = undefined; await forward?.(); },
    stop: async () => {
      for (const peer of peers) peer.destroy();
      await new Promise<void>((resolve) => server.close(() => resolve()));
    },
  };
}

type Diagnostic = { kind: string; message: string; occurred_at: number };

function observeDiagnostics(page: Page): () => Diagnostic[] {
  let diagnostics: Diagnostic[] = [];
  page.on("websocket", (socket) => socket.on("framereceived", ({ payload }) => {
    const frame = JSON.parse(String(payload)) as {
      type: string; payload?: { rest?: { status?: { diagnostics?: Diagnostic[] } } };
    };
    if ((frame.type === "snapshot" || frame.type === "delta") && frame.payload?.rest?.status?.diagnostics) {
      diagnostics = frame.payload.rest.status.diagnostics;
    }
  }));
  return () => diagnostics;
}

// Both cases exercise real terminal input/focus through the platform's Herdr.
test("rapid pane clicks coalesce behind one request and leave keys on the last pane", { tag: "@platform" }, async ({ page }) => {
  const herdr = await startHerdr();
  let gate: Awaited<ReturnType<typeof focusGate>> | undefined;
  let daemon: Daemon | undefined;
  try {
    gate = await focusGate(herdr);
    daemon = await startHided({ ...herdr, socket: gate.socket }, "focus-ordering");
    const diagnostics = observeDiagnostics(page);
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    const [first, second] = herdr.panes;
    const panes = herdr.panes.map((pane) => page.locator(`[data-pane-view="${pane}"]`));
    for (const pane of panes) await expect(pane).toHaveAttribute("data-transport", "controlling");
    await expect.poll(() => herdrHasFocus(herdr, first)).toBe(true);
    const boxes = await Promise.all(panes.map((pane) => pane.boundingBox()));
    let lastClickAt = 0;
    const click = async (index: number) => {
      const box = boxes[index]!;
      lastClickAt = Date.now();
      await page.mouse.click(box.x + 100, box.y + 100);
    };
    const confirmed = () => {
      const last = diagnostics().filter((entry) => entry.kind.startsWith("pane.focus")).at(-1);
      return last?.kind === "pane.focus"
        && last.message.startsWith(`Pane ${first} focus confirmed`)
        && last.occurred_at >= lastClickAt;
    };

    const held = gate.arm();
    await click(1);
    await held;
    for (let i = 0; i < 20; i += 1) {
      await click(1);
      await click(0);
    }
    await expect(panes[0]).toHaveAttribute("data-focused", "true");
    await gate.release();
    await expect.poll(confirmed).toBe(true);
    expect(gate.requests).toEqual([second, first]);
    expect(gate.maximum()).toBe(1);
    expect(herdrHasFocus(herdr, first)).toBe(true);

    // The same rapid clicks without a held request prove the ordinary path.
    for (let i = 0; i < 20; i += 1) {
      await click(1);
      await click(0);
    }
    await expect.poll(confirmed).toBe(true);
    await expect(panes[0]).toHaveAttribute("data-focused", "true");
    await expect.poll(() => herdrHasFocus(herdr, first)).toBe(true);
    const marker = "FOCUS_ORDERING_INPUT";
    await page.keyboard.type(marker);
    await expect.poll(() => fs.readFileSync(herdr.inputLogs[0], "utf8")).toContain(marker);
    expect(fs.readFileSync(herdr.inputLogs[1], "utf8")).not.toContain(marker);
    expect(gate.maximum()).toBe(1);
    await screenshot(page, "pane-focus-ordering-last-click");

    // With the local burst confirmed, a real external Herdr focus is followed.
    herdr.run(["pane", "focus", second]);
    await expect(panes[1]).toHaveAttribute("data-focused", "true");
    await expect.poll(() => diagnostics().some((entry) => entry.kind === "pane.focus.followed")).toBe(true);
  } finally {
    daemon?.stop();
    await gate?.stop();
    herdr.stop();
  }
});

test("an unknown focus result ends the burst before a delayed mutation is released", { tag: "@platform" }, async ({ page }) => {
  const herdr = await startHerdr();
  let gate: Awaited<ReturnType<typeof focusGate>> | undefined;
  let daemon: Daemon | undefined;
  try {
    gate = await focusGate(herdr);
    daemon = await startHided({ ...herdr, socket: gate.socket }, "focus-unknown");
    const diagnostics = observeDiagnostics(page);
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    const [last, delayed] = herdr.panes;
    const panes = herdr.panes.map((pane) => page.locator(`[data-pane-view="${pane}"]`));
    for (const pane of panes) await expect(pane).toHaveAttribute("data-transport", "controlling");
    await expect.poll(() => herdrHasFocus(herdr, last)).toBe(true);
    const boxes = await Promise.all(panes.map((pane) => pane.boundingBox()));
    const click = async (index: number) => {
      const box = boxes[index]!;
      await page.mouse.click(box.x + 100, box.y + 100);
    };
    const held = gate.arm();
    const started = Date.now();
    await click(1);
    await held;
    await click(0);
    await expect(panes[0]).toHaveAttribute("data-focused", "true");

    // Hold until the actual existing five-second transport budget reports
    // unknown, rather than releasing after an arbitrary delay.
    await expect.poll(() => diagnostics().some((entry) => entry.kind === "pane.focus.unknown"), { timeout: 10_000 }).toBe(true);
    expect(gate.requests).toEqual([delayed]);
    expect(diagnostics().some((entry) => entry.kind === "pane.focus" && entry.occurred_at >= started)).toBe(false);
    expect(herdrHasFocus(herdr, last)).toBe(true);
    await expect(panes[0]).toHaveAttribute("data-focused", "true");

    // A timeout did not cancel the sent effect. Deliver it to real Herdr now
    // and observe the documented late external move, without a queued B.
    await gate.release();
    await expect.poll(() => herdrHasFocus(herdr, delayed)).toBe(true);
    await expect(panes[1]).toHaveAttribute("data-focused", "true");
    await expect.poll(() => diagnostics().some((entry) => entry.kind === "pane.focus.followed" && entry.occurred_at >= started)).toBe(true);
    expect(gate.requests).toEqual([delayed]);
    expect(gate.maximum()).toBe(1);
    await screenshot(page, "pane-focus-unknown-late-effect");

    // A later explicit user selection starts a fresh, confirmed burst.
    const retryAt = Date.now();
    await click(0);
    await expect.poll(() => diagnostics().some((entry) => entry.kind === "pane.focus"
      && entry.message.startsWith(`Pane ${last} focus confirmed`) && entry.occurred_at >= retryAt)).toBe(true);
    await expect.poll(() => herdrHasFocus(herdr, last)).toBe(true);
    await expect(panes[0]).toHaveAttribute("data-focused", "true");
    expect(gate.requests).toEqual([delayed, last]);
  } finally {
    daemon?.stop();
    await gate?.stop();
    herdr.stop();
  }
});
