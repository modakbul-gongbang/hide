#!/usr/bin/env node
// A channel boundary: the provider passes an explicit request ID on every callback.
// The provider owns transport and authentication; this example owns no credentials.
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const cli = path.join(root, "dist", "hcoord", "cli.js");
const [action, requestId, answer] = process.argv.slice(2);
if (!["notify", "notify-only", "reply"].includes(action) || !/^r_[a-f0-9-]{36}$/.test(requestId ?? "") || (action === "reply" && !answer)) {
  process.stderr.write("usage: node channel-adapter.mjs notify|notify-only|reply <request-id> [answer]\n");
  process.exitCode = 2;
} else {
  const call = (args) => {
    const child = spawnSync(process.execPath, [cli, ...args, "--json"], { encoding: "utf8", timeout: 7000, maxBuffer: 1024 * 1024 });
    if (child.error || child.status !== 0) {
      const decoded = (() => { try { return JSON.parse(child.stdout); } catch { return null; } })();
      throw new Error(decoded?.error?.code ?? "coordinator_unavailable");
    }
    return JSON.parse(child.stdout).value;
  };
  try {
    if (action === "reply") {
      // A duplicate or competing callback is refused by the coordinator.
      // A canceled request records this as a late answer without reopening it.
      const result = call(["request", "reply", requestId, "--body", answer, "--as", "human"]);
      process.stdout.write(`${JSON.stringify({ requestId, status: result.status, lateRecorded: result.status === "canceled" })}\n`);
    } else {
      call(["request", "show", requestId]);
      process.stdout.write(`${JSON.stringify({ requestId, message: `hcoord request ${requestId}: open hcoord inbox`, replyPath: action === "notify-only" ? `hcoord request reply ${requestId} --as human --body <answer>` : "send the same requestId to this adapter's reply action" })}\n`);
    }
  } catch (error) {
    process.stderr.write(`${JSON.stringify({ requestId, error: String(error.message), nextAction: "inspect hcoord request show and hcoord inbox" })}\n`);
    process.exitCode = 1;
  }
}
