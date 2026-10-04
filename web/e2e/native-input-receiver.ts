import { expect } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import type { HerdrFixture } from "./herdr-fixture";
import { fixtureExecutable, fixturePosixShell, fixtureShellCommand } from "./platform-fixture";

const MAX_BYTES = 64 * 1024;
type Receiver = { pane: string; pid: number; file: string; inputFlagsBefore: number; inputFlagsAfter: number };

/** Each receiver consumes the actual pane's native stdin and executes the
 * submitted shell line. Expected bytes never come from renderer/ws telemetry. */
export async function nativeInputReceivers(fixture: HerdrFixture, panes: string[]) {
  if (!panes.length || panes.length > 3 || new Set(panes).size !== panes.length) throw new Error("invalid native receiver pane inventory");
  const binary = path.join(fixture.root, "bin", fixtureExecutable("hide-input-receiver"));
  fs.copyFileSync(path.join(fixture.root, "bin", fixtureExecutable("claude")), binary);
  const receivers: Receiver[] = [];
  for (const pane of panes) {
    if (!/^w[\w-]+:p[\w-]+$/.test(pane)) throw new Error("invalid native receiver pane identity");
    const file = path.join(fixture.root, `received-${pane.replaceAll(":", "-")}.bytes`);
    execFileSync(fixture.bin, ["pane", "send-text", pane, fixtureShellCommand(binary, ["--fixture-receive-input", file, pane, fixturePosixShell()]) + "\n"], { env: fixture.env, timeout: 30_000 });
    let receiver: Receiver | undefined;
    await expect.poll(() => {
      if (!fs.existsSync(file + ".identity")) return false;
      const stat = fs.lstatSync(file + ".identity");
      if (!stat.isFile() || stat.size > 256) throw new Error("invalid native receiver identity file");
      const value = fs.readFileSync(file + ".identity", "utf8");
      const match = value.match(/^([^\s]+) (\d+) (\d+) (\d+)\n$/);
      if (!match || match[1] !== pane || !Number.isSafeInteger(Number(match[2])) || Number(match[2]) <= 0) throw new Error("native receiver launch identity mismatch");
      const pid = Number(match[2]);
      const answer = fixture.run(["pane", "process-info", "--pane", pane]) as {
        result: { process_info: { shell_pid: number | null; foreground_processes: { pid: number }[] } };
      };
      const observed = answer.result.process_info;
      if (observed.shell_pid !== pid && !observed.foreground_processes.some(process => process.pid === pid)) return false;
      const screen = execFileSync(fixture.bin, ["pane", "read", pane, "--source", "visible", "--format", "text"], { env: fixture.env, encoding: "utf8", timeout: 10_000 });
      if (!screen.includes("fixture input receiver ready")) return false;
      receiver = { pane, pid, file, inputFlagsBefore: Number(match[3]), inputFlagsAfter: Number(match[4]) };
      return true;
    }).toBe(true);
    if (!receiver) throw new Error("native receiver was not observed in its actual pane");
    receivers.push(receiver);
  }
  function read(): Map<string, Buffer> {
    return new Map(receivers.map(({ pane, file }) => {
      const stat = fs.lstatSync(file);
      if (!stat.isFile() || stat.size > MAX_BYTES) throw new Error("native receiver input file/byte cap exceeded");
      return [pane, fs.readFileSync(file)];
    }));
  }
  function confirm(expected: Map<string, Buffer>): void {
    const actual = read();
    if (expected.size !== actual.size || [...actual.keys()].some(pane => !expected.has(pane))) throw new Error("native receiver expected pane inventory mismatch");
    for (const [pane, received] of actual) {
      const wanted = expected.get(pane)!;
      if (!received.equals(wanted)) throw new Error(`native byte receipt mismatch for ${pane}: expected=${wanted.toString("hex")}, received=${received.toString("hex")}`);
    }
  }
  function receipt() {
    const rows = read();
    return { version: 1, os: process.platform, receivers: receivers.map(({ pane, pid, inputFlagsBefore, inputFlagsAfter }) => ({ pane, pid, inputFlagsBefore, inputFlagsAfter, receivedHex: rows.get(pane)!.toString("hex") })), capBytes: MAX_BYTES };
  }
  return {
    read, confirm, receipt,
    export() {
      const directory = process.env.HIDE_E2E_SCREENSHOT_DIR;
      if (!directory) return;
      fs.mkdirSync(directory, { recursive: true });
      fs.writeFileSync(path.join(directory, `${path.basename(fixture.root)}-native-input.json`),
        JSON.stringify(receipt()));
    },
  };
}
