import { describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import { ChildRunner, startDetached } from "./spawn";

describe("the child runner", () => {
  it("returns a real child's output and exit code", async () => {
    const result = await new ChildRunner().run("/bin/sh", ["-c", "echo out; echo err >&2; exit 3"], 5_000);
    expect(result).toMatchObject({ code: 3, stdout: "out\n", stderr: "err\n", timedOut: false, spawnError: null });
  });

  it("gives a child the environment it is handed, whole", async () => {
    const result = await new ChildRunner().run("/bin/sh", ["-c", 'printf "%s|%s" "$HERDR_BIN_PATH" "$HOME"'], 5_000, { HERDR_BIN_PATH: "/r/herdr", PATH: "/usr/bin:/bin" });
    expect(result.stdout).toBe("/r/herdr|");
  });

  it("kills a child past its timeout", async () => {
    const started = Date.now();
    const result = await new ChildRunner().run("/bin/sleep", ["10"], 200);
    expect(result.timedOut).toBe(true);
    expect(Date.now() - started).toBeLessThan(5_000);
  });

  it("reports a spawn that never started", async () => {
    const result = await new ChildRunner().run("/nonexistent/hide", ["connect"], 5_000);
    expect(result.spawnError).toMatch(/ENOENT/);
  });

  it("refuses a second child while one runs", async () => {
    const runner = new ChildRunner();
    const first = runner.run("/bin/sleep", ["10"], 5_000);
    await expect(runner.run("/bin/echo", [], 1_000)).rejects.toThrow(/over budget/);
    runner.stop();
    expect((await first).signal).toBe("SIGKILL");
  });
});

describe("a detached start", () => {
  it("leaves a child that leads its own process group", async () => {
    const started = await startDetached("/bin/sleep", ["30"], { PATH: "/usr/bin:/bin" });
    if (!("pid" in started)) throw new Error(started.spawnError);
    try {
      // macOS `ps` reports no session id, so the new session shows as a group the child leads.
      const group = (pid: number) => Number(spawnSync("/bin/ps", ["-o", "pgid=", "-p", String(pid)], { encoding: "utf8" }).stdout.trim());
      expect(group(started.pid)).toBe(started.pid);
      expect(group(process.pid)).not.toBe(started.pid);
    } finally {
      process.kill(started.pid, "SIGKILL");
    }
  });

  it("reports a binary that cannot start", async () => {
    expect(await startDetached("/nonexistent/herdr", ["server"], {})).toEqual({ spawnError: expect.stringMatching(/ENOENT/) });
  });
});
