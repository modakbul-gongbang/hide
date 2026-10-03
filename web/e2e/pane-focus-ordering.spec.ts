import { expect, test } from "@playwright/test";
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
  let release: (() => void) | undefined;
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
      const forward = () => {
        const upstream = net.connect(herdr.socket, () => upstream.write(buffer));
        peers.add(upstream);
        upstream.on("close", () => peers.delete(upstream));
        upstream.on("error", () => client.destroy());
        client.on("close", () => upstream.destroy());
        if (focus) upstream.once("data", () => { active -= 1; });
        client.pipe(upstream).pipe(client);
      };
      if (focus && armed) {
        armed = false;
        release = forward;
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
    release: () => { const forward = release; release = undefined; forward?.(); },
    stop: async () => {
      for (const peer of peers) peer.destroy();
      await new Promise<void>((resolve) => server.close(() => resolve()));
    },
  };
}

test("rapid pane clicks coalesce behind one request and leave keys on the last pane", { tag: "@platform" }, async ({ page }) => {
  const herdr = await startHerdr();
  let gate: Awaited<ReturnType<typeof focusGate>> | undefined;
  let daemon: Daemon | undefined;
  try {
    gate = await focusGate(herdr);
    daemon = await startHided({ ...herdr, socket: gate.socket }, "focus-ordering");
    type Diagnostic = { kind: string; message: string; occurred_at: number };
    let diagnostics: Diagnostic[] = [];
    page.on("websocket", (socket) => socket.on("framereceived", ({ payload }) => {
      const frame = JSON.parse(String(payload)) as {
        type: string; payload?: { status?: { diagnostics?: Diagnostic[] } };
      };
      if ((frame.type === "snapshot" || frame.type === "delta") && frame.payload?.status?.diagnostics) {
        diagnostics = frame.payload.status.diagnostics;
      }
    }));
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
      const last = diagnostics.filter((entry) => entry.kind.startsWith("pane.focus")).at(-1);
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
    gate.release();
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
    await expect.poll(() => diagnostics.some((entry) => entry.kind === "pane.focus.followed")).toBe(true);
  } finally {
    daemon?.stop();
    await gate?.stop();
    herdr.stop();
  }
});
