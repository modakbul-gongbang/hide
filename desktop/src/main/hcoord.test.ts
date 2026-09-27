import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { bundledHcoord, hcoordEnvironment, installHcoordShim, parseHcoordEnsure } from "./hcoord";

describe("the packaged hcoord runtime", () => {
  it("writes the stable shim for Electron-as-Node and the bundled Herdr", () => {
    const home = fs.mkdtempSync(path.join(os.tmpdir(), "hide-hcoord-"));
    const bundle = bundledHcoord("/Applications/hide.app/Contents/Resources", "/Applications/hide.app/Contents/MacOS/hide");
    const target = installHcoordShim(home, bundle);
    const text = fs.readFileSync(target, "utf8");
    expect(text).toContain("ELECTRON_RUN_AS_NODE=1");
    expect(text).toContain("Contents/Resources/herdr");
    expect(text).toContain("Contents/Resources/hcoord/dist/hcoord/cli.js");
    expect(fs.statSync(target).mode & 0o700).toBe(0o700);
    fs.rmSync(home, { recursive: true, force: true });
  });

  it("passes only runtime locations and reports manual stop without treating it as a failure", () => {
    const bundle = bundledHcoord("/R", "/A/hide");
    expect(hcoordEnvironment({ PATH: "/bin" }, "/H", bundle)).toMatchObject({ HOME: "/H", PATH: "/bin", ELECTRON_RUN_AS_NODE: "1", HERDR_BIN_PATH: "/R/herdr" });
    expect(parseHcoordEnsure({ code: 0, signal: null, stdout: '{"ok":true,"value":{"manualStop":true,"changed":false}}\n', stderr: "", timedOut: false, spawnError: null }))
      .toEqual({ ok: true, changed: false, manualStop: true, version: null });
  });
});
