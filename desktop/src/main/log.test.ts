import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { HostLog, loadFailureFields } from "./log";

describe("the host log", () => {
  it("never writes the daemon token from a failed load", () => {
    // The shape Electron 44 rejects `loadURL` with.
    const error = Object.assign(new Error("ERR_CONNECTION_REFUSED (-102) loading 'http://127.0.0.1:7001/#token=SECRET123'"), {
      code: "ERR_CONNECTION_REFUSED",
    });
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hide-desktop-log-"));
    const log = new HostLog(dir);
    const stderr = process.stderr.write;
    let echoed = "";
    process.stderr.write = ((chunk: string) => ((echoed += chunk), true)) as typeof process.stderr.write;
    try {
      log.event("window.load_failed", { state: "attached", ...loadFailureFields(error) });
    } finally {
      process.stderr.write = stderr;
    }
    const written = fs.readFileSync(log.path, "utf8");
    expect(JSON.parse(written)).toMatchObject({ event: "window.load_failed", state: "attached", code: "ERR_CONNECTION_REFUSED" });
    expect(written).not.toContain("SECRET123");
    expect(echoed).not.toContain("SECRET123");
    expect(loadFailureFields("not an error")).toEqual({ code: "unknown" });
    fs.rmSync(dir, { recursive: true, force: true });
  });
});
