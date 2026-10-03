import { createHash } from "node:crypto";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { bootoutLabel, labelStatus } from "../../plugins/hcoord/src/support/launchd";

export const OPERATOR_HCOORD_LABEL = "com.hcoord.daemon";

/** hcoord's per-directory label; only a fixture-owned home may be passed here. */
export function hcoordLabel(hcoordHome: string): string {
  return `${OPERATOR_HCOORD_LABEL}.${createHash("sha256").update(path.resolve(hcoordHome)).digest("hex").slice(0, 12)}`;
}

/** Read-only operator identity checks also report unknown state rather than claiming absence. */
export function launchdPid(label: string): string | null {
  const printed = spawnSync("launchctl", ["print", `gui/${os.userInfo().uid}/${label}`], { encoding: "utf8", timeout: 10_000 });
  if (printed.status === 113) return null;
  if (printed.error || printed.status !== 0) throw new Error(`launchctl could not read ${label}: ${printed.error ?? printed.stderr}`);
  return printed.stdout.match(/^\s*pid = (\d+)/m)?.[1] ?? "loaded";
}

/** Idempotent, bounded unload of one exact test label, using the product's settlement check. */
export function bootoutTestLabel(label: string): void {
  if (!/^com\.hcoord\.daemon\.[a-f0-9]{12}$/.test(label)) throw new Error(`${label} is not a test hcoord label`);
  const target = { label, plistPath: "" };
  const before = labelStatus(target);
  if (before.loaded === false) return;
  if (before.loaded === null) throw new Error(`cannot clean ${label}: ${before.detail}`);
  const removed = bootoutLabel(target);
  if (!removed.ok) throw new Error(`cannot clean ${label}: ${removed.detail}; preserve the test HOME`);
}
