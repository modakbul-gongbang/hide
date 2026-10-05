import fs from "node:fs";
import net from "node:net";
import path from "node:path";
import type { HerdrFixture } from "./herdr-fixture";

export type GateParams = Record<string, unknown>;

/**
 * Holds one Herdr request at the real socket boundary and lets the test
 * release it, so the test decides the order two controls reach Herdr in.
 * Every request is still forwarded to the pinned Herdr, which gives every
 * answer; the gate only records what arrived (`params`, `maximum`) and delays
 * the one request `arm` asked for.
 *
 * The reference use is `pane-focus-ordering.spec.ts`. An order rule the core
 * decides belongs in `herdr-core/src/runtime/tests/control_order.rs`; use the
 * gate for the one journey that shows the pieces are connected.
 */
export async function herdrGate(herdr: HerdrFixture) {
  const socket = path.join(herdr.root, "gate.sock");
  // Herdr maps a filesystem path to the Windows named-pipe namespace.
  const endpoint = (address: string) => process.platform === "win32" ? `\\\\.\\pipe\\${address}` : address;
  const clientSocket = socket.replace(/\.sock$/, "-client.sock");
  const peers = new Set<net.Socket>();
  const calls: { method: string; params: GateParams }[] = [];
  const active = new Map<string, number>();
  const maximums = new Map<string, number>();
  let holding: string | undefined;
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
        method: string; params: GateParams;
      };
      const method = request.method;
      calls.push({ method, params: request.params ?? {} });
      const running = (active.get(method) ?? 0) + 1;
      active.set(method, running);
      maximums.set(method, Math.max(maximums.get(method) ?? 0, running));
      const forward = (received?: () => void, failed?: (error: Error) => void) => {
        const upstream = net.connect(endpoint(herdr.socket), () => upstream.write(buffer));
        peers.add(upstream);
        upstream.on("close", () => peers.delete(upstream));
        upstream.on("error", (error) => { client.destroy(); failed?.(error); });
        client.on("close", () => upstream.destroy());
        upstream.once("data", () => {
          active.set(method, (active.get(method) ?? 1) - 1);
          received?.();
          if (client.destroyed) upstream.destroy();
        });
        if (!client.destroyed) client.pipe(upstream).pipe(client);
      };
      if (holding === method) {
        holding = undefined;
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
    socket,
    /** The params of every `method` request that arrived since `arm`, in arrival order. */
    params: (method: string) => calls.filter((call) => call.method === method).map((call) => call.params),
    /** The most requests of `method` that were unanswered at one moment. */
    maximum: (method: string) => maximums.get(method) ?? 0,
    /** Holds the next `method` request and resolves when it is held. Starts a new record. */
    arm: (method: string) => {
      holding = method;
      calls.length = 0;
      maximums.clear();
      active.clear();
      return new Promise<void>((resolve) => { held = resolve; });
    },
    /** Forwards the held request to Herdr and resolves when Herdr answers. */
    release: async () => { const forward = release; release = undefined; await forward?.(); },
    stop,
  };
}
