import assert from "node:assert/strict";
import fs from "node:fs";
import net from "node:net";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { spawnOwned, stopOwned } from "./owned-children.mjs";

const CLI = path.resolve(import.meta.dirname, "../../dist/hcoord/cli.js");
const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** One isolated HQ: a fake Herdr, a HOME, and a daemon the fake stops before it removes that HOME. */
export function hq(fake, host = "local", extraEnv = {}) {
  const env = fake.env(host, extraEnv);
  const dir = path.join(env.HOME, ".hcoord");
  let daemon = null;
  const api = {
    env, dir,
    run: (...args) => spawnSync(process.execPath, [CLI, ...args, "--json"], { env, encoding: "utf8" }),
    json(...args) { const result = api.run(...args); try { return JSON.parse(result.stdout); } catch { throw new Error(`${args.join(" ")}: ${result.status} ${result.stdout} ${result.stderr}`); } },
    ok(...args) { const parsed = api.json(...args); assert.equal(parsed.ok, true, `${args.join(" ")}: ${JSON.stringify(parsed)}`); return parsed.value; },
    async start() {
      daemon = spawnOwned(`hcoord daemon for ${host}`, process.execPath, [CLI, "daemon", "run"], { env, stdio: ["ignore", "ignore", "pipe"] });
      let stderr = "";
      daemon.stderr.on("data", (chunk) => { stderr += chunk; });
      for (let attempt = 0; attempt < 500; attempt += 1) {
        // A killed daemon leaves its socket file behind; only an accepted connection proves this one listens.
        if (await new Promise((resolve) => { const socket = net.createConnection(path.join(dir, "api.sock")); socket.once("connect", () => { socket.destroy(); resolve(true); }); socket.once("error", () => resolve(false)); })) return;
        if (daemon.exitCode !== null) throw new Error(`daemon exited: ${stderr}`);
        await wait(20);
      }
      throw new Error(`daemon did not listen: ${stderr}`);
    },
    async stop(signal = "SIGTERM") {
      if (!daemon) return;
      const old = daemon; daemon = null;
      await stopOwned(old, signal);
    },
    ledger: () => JSON.parse(fs.readFileSync(path.join(dir, "ledger.json"), "utf8")),
    outbox: () => fs.existsSync(path.join(dir, "outbox")) ? fs.readdirSync(path.join(dir, "outbox")).filter((name) => name.endsWith(".json")) : [],
    async until(predicate, message) {
      for (let attempt = 0; attempt < 150; attempt += 1) { if (predicate()) return; await wait(100); }
      assert.fail(message);
    },
  };
  fake.own(() => api.stop());
  return api;
}
