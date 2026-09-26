import { describe, expect, it } from "vitest";
import { ChildRunner } from "./spawn";

describe("the child runner", () => {
  it("returns a real child's output and exit code", async () => {
    const result = await new ChildRunner().run("/bin/sh", ["-c", "echo out; echo err >&2; exit 3"], 5_000);
    expect(result).toMatchObject({ code: 3, stdout: "out\n", stderr: "err\n", timedOut: false, spawnError: null });
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
