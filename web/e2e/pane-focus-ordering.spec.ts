import { expect, test, type CDPSession, type Page } from "@playwright/test";
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
  // Herdr maps a filesystem path to the Windows named-pipe namespace.
  const endpoint = (address: string) => process.platform === "win32" ? `\\\\.\\pipe\\${address}` : address;
  const clientSocket = socket.replace(/\.sock$/, "-client.sock");
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
        const upstream = net.connect(endpoint(herdr.socket), () => upstream.write(buffer));
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
  const listeners = [server];
  const markers: string[] = [];
  const listen = (listener: net.Server, address: string) => new Promise<void>((resolve, reject) => {
    listener.once("error", reject);
    listener.listen(endpoint(address), resolve);
  });
  const stop = async () => {
    for (const peer of peers) peer.destroy();
    await Promise.all(listeners.map((listener) => new Promise<void>((resolve) => listener.close(() => resolve()))));
    for (const marker of markers) fs.rmSync(marker, { force: true });
  };
  try {
    // Herdr's terminal CLI uses the separate client socket, not JSON control.
    if (process.platform === "win32") {
      // HERDR_SOCKET_PATH takes precedence over a client-only override.
      // Forward the CLI's derived endpoint as bytes, without JSON gating.
      const clientServer = net.createServer((client) => {
        const upstream = net.connect(endpoint(herdr.socket.replace(/\.sock$/, "-client.sock")));
        for (const peer of [client, upstream]) {
          peers.add(peer);
          peer.on("close", () => { peers.delete(peer); client.destroy(); upstream.destroy(); });
          peer.on("error", () => { client.destroy(); upstream.destroy(); });
        }
        client.pipe(upstream).pipe(client);
      });
      clientServer.maxConnections = 64;
      listeners.push(clientServer);
      await listen(clientServer, clientSocket);
    } else {
      fs.symlinkSync(herdr.socket.replace(/\.sock$/, "-client.sock"), clientSocket);
    }
    await listen(server, socket);
    if (process.platform === "win32") {
      // The core checks these marker paths before connecting to the pipes.
      for (const marker of [socket, clientSocket]) {
        fs.writeFileSync(marker, `${process.pid}:${Date.now()}`, { flag: "wx" });
        markers.push(marker);
      }
    }
  } catch (error) {
    await stop();
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
    stop,
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

/** Passive evidence only: retain the last 64 outgoing focus messages without
 * changing the page's send, focus actions, or the test's behavior assertions. */
function observeFocusFrames(page: Page) {
  const limit = 64;
  const records: { occurred_at: number; kind: string; pane_id: string }[] = [];
  let evicted = 0;
  let lastPane: string | undefined;
  let changes = 0;
  let completedClicks = 0;
  page.on("websocket", (socket) => socket.on("framesent", ({ payload }) => {
    if (typeof payload !== "string") return;
    let frame: { kind?: string; payload?: { pane_id?: string; action?: string } };
    try {
      const parsed: unknown = JSON.parse(payload);
      if (!parsed || typeof parsed !== "object") return;
      frame = parsed as typeof frame;
    } catch {
      // Unstructured traffic supplies no focus evidence.
      return;
    }
    const kind = frame.kind;
    if (kind === "terminal_click") {
      completedClicks += 1;
      return;
    }
    const paneId = frame.payload?.pane_id;
    if (kind !== "focus_pane" && kind !== "remote_control") return;
    if (kind === "remote_control" && frame.payload?.action !== "focus_pane") return;
    if (typeof paneId !== "string") return;
    if (paneId !== lastPane) {
      changes += 1;
      lastPane = paneId;
    }
    if (records.length === limit) {
      records.shift();
      evicted += 1;
    }
    records.push({ occurred_at: Date.now(), kind, pane_id: paneId });
  }));
  return {
    completedClicks: () => completedClicks,
    changes: () => changes,
    lastPane: () => lastPane,
    save: () => test.info().attach("pane-focus-sent", {
      contentType: "application/json",
      body: Buffer.from(JSON.stringify({ record_limit: limit, evicted, records })),
    }),
  };
}

// Both cases exercise real terminal input/focus through the platform's Herdr.
test("rapid pane clicks coalesce behind one request and leave keys on the last pane", { tag: ["@platform", "@flaky"], annotation: { type: "issue", description: "https://github.com/modakbul-gongbang/hide/issues/397" } }, async ({ page }) => {
  const herdr = await startHerdr();
  const focusFrames = observeFocusFrames(page);
  // Two fixed bursts produce at most 32 stage records, with no input contents.
  const stages: { stage: string; at: number }[] = [];
  const mark = (stage: string) => { stages.push({ stage, at: Date.now() }); };
  let firstFocusAt: number | undefined;
  let mouseSession: CDPSession | undefined;
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
    // Protocol setup and detach belong outside the held Herdr request.
    // Detach alone consumed two seconds of its five-second budget on macOS.
    mark("cdp.connect.before");
    const mouse = await page.context().newCDPSession(page);
    mouseSession = mouse;
    mark("cdp.connect.after");
    // These polls read captured diagnostics in memory, without browser or
    // Herdr I/O. Observe acceptance promptly within the unchanged timeout.
    const focusObservation = { intervals: [25] };
    let lastClickAt = 0;
    const click = async (index: number) => {
      const box = boxes[index]!;
      lastClickAt = Date.now();
      await page.mouse.click(box.x + 100, box.y + 100);
    };
    const clickBurst = async (mouse: CDPSession) => {
      const completedAfter = focusFrames.completedClicks() + 40;
      const inputs: Promise<unknown>[] = [];
      lastClickAt = Date.now();
      // Use the same trusted Chromium input as page.mouse, queued in order.
      // Awaiting 40 separate browser round trips can outlast the held
      // request's real five-second deadline on a loaded macOS runner.
      for (let i = 0; i < 20; i += 1) {
        for (const index of [1, 0]) {
          const box = boxes[index]!;
          const point = { x: box.x + 100, y: box.y + 100 };
          inputs.push(mouse.send("Input.dispatchMouseEvent", { ...point, type: "mouseMoved", buttons: 0 }));
          inputs.push(mouse.send("Input.dispatchMouseEvent", { ...point, type: "mousePressed", button: "left", buttons: 1, clickCount: 1 }));
          inputs.push(mouse.send("Input.dispatchMouseEvent", { ...point, type: "mouseReleased", button: "left", buttons: 0, clickCount: 1 }));
        }
      }
      mark("cdp.inputs.before");
      await Promise.all(inputs);
      mark("cdp.inputs.after");
      // ACKs from the separate CDP session do not fence frame observation.
      // Each real mouseup sends terminal_click after its focus event on the
      // same WebSocket. The last click fences every earlier focus frame.
      mark("cdp.processed.before");
      await expect.poll(() => focusFrames.completedClicks(), focusObservation).toBe(completedAfter);
      mark("cdp.processed.after");
    };
    const requested = () => diagnostics().filter((entry) => entry.kind === "pane.focus.requested");
    const confirmed = () => {
      const last = diagnostics().filter((entry) => entry.kind.startsWith("pane.focus")).at(-1);
      return last?.kind === "pane.focus"
        && last.message.startsWith(`Pane ${first} focus confirmed`)
        && last.occurred_at >= lastClickAt;
    };

    mark("gate.arm");
    const held = gate.arm();
    mark("first.click.before");
    await click(1);
    mark("first.click.after");
    await held;
    mark("gate.held");
    mark("first.requested.before");
    await expect.poll(() => requested().at(-1)?.message, focusObservation).toBe(`Focusing pane ${second}`);
    firstFocusAt = requested()[0]?.occurred_at;
    mark("first.requested.after");
    await expect.poll(() => focusFrames.completedClicks(), focusObservation).toBe(1);
    const acceptedBefore = requested().length;
    const sentBefore = focusFrames.changes();
    await clickBurst(mouse);
    expect(focusFrames.lastPane()).toBe(first);
    // Count changes actually sent by the UI, not clicks: focusing an already
    // focused textarea sends nothing, and repeated targets share one intent.
    // Observe every accepted change before releasing the gate; a stale first-
    // pane snapshot or a later focus diagnostic cannot satisfy this barrier.
    const acceptedAfter = acceptedBefore + focusFrames.changes() - sentBefore;
    mark("burst.accepted.before");
    await expect.poll(() => requested().length, focusObservation).toBe(acceptedAfter);
    mark("burst.accepted.after");
    expect(requested().at(-1)?.message).toBe(`Focusing pane ${first}`);
    expect(diagnostics().some((entry) => entry.kind === "pane.focus.unknown")).toBe(false);
    expect(gate.requests).toEqual([second]);
    mark("focused.attribute.before");
    await expect(panes[0]).toHaveAttribute("data-focused", "true");
    mark("focused.attribute.after");
    mark("gate.release.before");
    await gate.release();
    mark("gate.release.after");
    mark("first.confirmed.before");
    await expect.poll(confirmed, focusObservation).toBe(true);
    mark("first.confirmed.after");
    mark("cdp.detach.before");
    await mouse.detach();
    mouseSession = undefined;
    mark("cdp.detach.after");
    expect(gate.requests).toEqual([second, first]);
    expect(gate.maximum()).toBe(1);
    expect(herdrHasFocus(herdr, first)).toBe(true);

    // The same rapid clicks without a held request prove the ordinary path.
    mouseSession = await page.context().newCDPSession(page);
    await clickBurst(mouseSession);
    await expect.poll(confirmed, focusObservation).toBe(true);
    await expect(panes[0]).toHaveAttribute("data-focused", "true");
    await expect.poll(() => herdrHasFocus(herdr, first)).toBe(true);
    const marker = "FOCUS_ORDERING_INPUT";
    await page.keyboard.type(marker);
    await expect.poll(() => fs.readFileSync(herdr.inputLogs[0], "utf8")).toContain(marker);
    expect(fs.readFileSync(herdr.inputLogs[1], "utf8")).not.toContain(marker);
    expect(gate.maximum()).toBe(1);
    await screenshot(page, "pane-focus-ordering-last-click");

    // With the local burst confirmed, a real external Herdr focus is followed.
    herdr.run(["pane", "focus", "--direction", "right", "--pane", first]);
    await expect(panes[1]).toHaveAttribute("data-focused", "true");
    await expect.poll(() => diagnostics().some((entry) => entry.kind === "pane.focus.followed")).toBe(true);
  } finally {
    try {
      mark("cdp.detach.before");
      await mouseSession?.detach();
      mark("cdp.detach.after");
    } finally {
      console.log("[focus-burst-timing]", JSON.stringify({
        first_focus_at: firstFocusAt,
        stages: stages.map(({ stage, at }) => ({ stage, at, since_first_focus_ms: firstFocusAt === undefined ? null : at - firstFocusAt })),
      }));
      daemon?.stop();
      await gate?.stop();
      herdr.stop();
      await focusFrames.save();
    }
  }
});

test("an unknown focus result ends the burst before a delayed mutation is released", { tag: "@platform" }, async ({ page }) => {
  const herdr = await startHerdr();
  const focusFrames = observeFocusFrames(page);
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
    await focusFrames.save();
  }
});
