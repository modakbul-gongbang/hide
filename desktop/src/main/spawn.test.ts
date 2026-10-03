import { describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import { ChildRunner, startDetached } from "./spawn";

// Node itself is the test program, so each child does the same on every system.
const node = process.execPath;
const script = (code: string): string[] => ["-e", code];
/** The environment a test hands a child: `extra` and, on Windows, the system folder Node needs to start. */
const handed = (extra: Record<string, string>): Record<string, string | undefined> => ({ ...extra, ...(process.env.SystemRoot ? { SystemRoot: process.env.SystemRoot } : {}) });

describe("the child runner", () => {
  it("returns a real child's output and exit code", async () => {
    const result = await new ChildRunner().run(node, script('process.stdout.write("out\\n"); process.stderr.write("err\\n"); process.exit(3)'), 5_000);
    expect(result).toMatchObject({ code: 3, stdout: "out\n", stderr: "err\n", timedOut: false, spawnError: null });
  });

  it("gives a child the environment it is handed, whole", async () => {
    const result = await new ChildRunner().run(node, script('process.stdout.write(`${process.env.HERDR_BIN_PATH}|${process.env.HOME ?? ""}`)'), 5_000, handed({ HERDR_BIN_PATH: "/r/herdr" }));
    expect(result.stdout).toBe("/r/herdr|");
  });

  it("kills a child past its timeout", async () => {
    const started = Date.now();
    const result = await new ChildRunner().run(node, script("setTimeout(() => {}, 10_000)"), 200);
    expect(result.timedOut).toBe(true);
    expect(Date.now() - started).toBeLessThan(5_000);
  });

  it("reports a spawn that never started", async () => {
    const result = await new ChildRunner().run("/nonexistent/hide", ["connect"], 5_000);
    expect(result.spawnError).toMatch(/ENOENT/);
  });

  it("refuses a second child while one runs", async () => {
    const runner = new ChildRunner();
    const first = runner.run(node, script("setTimeout(() => {}, 10_000)"), 5_000);
    await expect(runner.run(node, script(""), 1_000)).rejects.toThrow(/over budget/);
    runner.stop();
    expect((await first).signal).toBe("SIGKILL");
  });
});

describe("a detached start", () => {
  it("leaves a child that leads its own process group", async () => {
    const started = await startDetached(node, script("setTimeout(() => {}, 30_000)"), handed({}));
    if (!("pid" in started)) throw new Error(started.spawnError);
    try {
      // Windows starts a detached child in a new process group too
      // (`CREATE_NEW_PROCESS_GROUP`), but no tool there reads a process's
      // group back, so the group is read where `ps` can: macOS `ps` reports
      // no session id, so the new session shows as a group the child leads.
      if (process.platform !== "win32") {
        const group = (pid: number) => Number(spawnSync("/bin/ps", ["-o", "pgid=", "-p", String(pid)], { encoding: "utf8" }).stdout.trim());
        expect(group(started.pid)).toBe(started.pid);
        expect(group(process.pid)).not.toBe(started.pid);
      }
      expect(() => process.kill(started.pid, 0)).not.toThrow();
    } finally {
      process.kill(started.pid, "SIGKILL");
    }
  });

  it("reports a binary that cannot start", async () => {
    expect(await startDetached("/nonexistent/herdr", ["server"], {})).toEqual({ spawnError: expect.stringMatching(/ENOENT/) });
  });
});
