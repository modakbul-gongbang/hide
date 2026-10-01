import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import test from "node:test";
import { controlOptions, sshArgs } from "../../dist/hcoord/remote.js";

const ssh = spawnSync("ssh", ["-V"], { encoding: "utf8" });

test("remote runs reuse one SSH connection per target and the options parse in the real ssh", { skip: process.platform === "win32" || ssh.error !== undefined }, () => {
  const args = sshArgs("example-target", ["take"]);
  const options = args.slice(0, args.indexOf("example-target"));
  for (const option of controlOptions()) assert.ok(options.includes(option), `${option} reaches every SSH run`);
  // `ssh -G` resolves the configuration without connecting anywhere.
  const resolved = spawnSync("ssh", ["-G", ...options, "example-target"], { encoding: "utf8" });
  assert.equal(resolved.status, 0, resolved.stderr);
  const config = Object.fromEntries(resolved.stdout.split("\n").map((line) => [line.slice(0, line.indexOf(" ")), line.slice(line.indexOf(" ") + 1)]));
  assert.equal(config.controlmaster, "auto");
  assert.equal(config.controlpersist, "60");
  assert.match(config.controlpath, /^\/tmp\/hcoord-ssh-\d+\/%C$|^\/tmp\/hcoord-ssh-\d+\/[0-9a-f]{40}$/);
  assert.ok(Buffer.byteLength(config.controlpath.replace("%C", "0".repeat(40))) + 17 < 104, "the socket path stays under the unix socket limit");
  assert.equal(fs.statSync(config.controlpath.slice(0, config.controlpath.lastIndexOf("/"))).mode & 0o077, 0, "the socket directory is private");
});
