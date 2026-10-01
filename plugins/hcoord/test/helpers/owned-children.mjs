// The one way an e2e test starts a process. A live child keeps its test file's
// process alive, so a child that outlived its test once held the whole
// `node --test` run until CI cancelled it (48 and 156 minutes). Every child
// started here is ended by its owner, and whatever an owner missed is killed
// at the end of the file and reported as a failure naming it, so a leak fails
// the file instead of hanging it.
import { spawn } from "node:child_process";
import { after } from "node:test";

/** How long a child may take to exit after its stop signal; a daemon's clean stop takes well under a second. */
const STOP_BOUND_MS = 10_000;
const live = new Map();
const exited = (child) => child.exitCode !== null || child.signalCode !== null;
const exit = (child) => new Promise((resolve) => { if (exited(child)) resolve(); else child.once("exit", () => resolve()); });

/** Starts `command` as a child of this test file, named by `label` in any report. */
export function spawnOwned(label, command, args, options) {
  const child = spawn(command, args, options);
  live.set(child, `${label} (pid ${child.pid})`);
  child.once("exit", () => live.delete(child));
  return child;
}

/** Sends `signal` and waits for the exit; a child still running after the bound is killed and reported. */
export async function stopOwned(child, signal = "SIGTERM") {
  if (exited(child)) return;
  const name = live.get(child) ?? `pid ${child.pid}`;
  child.kill(signal);
  let timer;
  const late = await Promise.race([exit(child).then(() => false), new Promise((resolve) => { timer = setTimeout(() => resolve(true), STOP_BOUND_MS); })]);
  clearTimeout(timer);
  if (!late) return;
  child.kill("SIGKILL");
  await exit(child);
  throw new Error(`${name} did not exit within ${STOP_BOUND_MS} ms of ${signal}; killed it`);
}

after(async () => {
  if (live.size === 0) return;
  const names = [...live.values()];
  const children = [...live.keys()];
  for (const child of children) child.kill("SIGKILL");
  await Promise.all(children.map(exit));
  throw new Error(`still running after the file's last test, killed: ${names.join(", ")}`);
});
