import fs from "node:fs";
import fsp from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterAll, describe, expect, it, vi } from "vitest";
import { MAX_PROBE_PATHS, executableHeader, openRoute, probe, probeRequest } from "./localPath";

// The physical spelling `fs.realpath` answers: on Windows the temporary
// folder's long name, where `os.tmpdir()` may give its 8.3 short one.
const root = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "hide-local-path-")));
afterAll(() => fs.rmSync(root, { recursive: true, force: true }));

function file(name: string, contents: string | Uint8Array, mode = 0o644): string {
  const target = path.join(root, name);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(target, contents);
  fs.chmodSync(target, mode);
  return target;
}

describe("probeRequest", () => {
  it("takes absolute and home paths, at most the cap", () => {
    expect(probeRequest(["/tmp/a", "~/b"])).toEqual(["/tmp/a", "~/b"]);
    expect(probeRequest(Array.from({ length: MAX_PROBE_PATHS }, () => "/a"))).toHaveLength(MAX_PROBE_PATHS);
  });

  it("refuses the whole request when any entry is not a path the shell may name", () => {
    expect(probeRequest(Array.from({ length: MAX_PROBE_PATHS + 1 }, () => "/a"))).toBeNull();
    expect(probeRequest(["/a", "relative/b"])).toBeNull();
    expect(probeRequest(["/a\0b"])).toBeNull();
    expect(probeRequest(["~b"])).toBeNull();
    expect(probeRequest("/a")).toBeNull();
    expect(probeRequest([42])).toBeNull();
  });
});

describe("probe", () => {
  it("keeps confirmed non-directory absence separate from filesystem failures", async () => {
    const note = file("probe-errors/note.md", "# note");
    expect(await probe([note + "/child"])).toEqual([null]);
    for (const method of ["realpath", "stat"] as const) {
      for (const code of ["EIO", "EACCES"]) {
        const mock = vi.spyOn(fsp, method).mockRejectedValueOnce(Object.assign(new Error("private path " + note), { code }));
        try {
          await expect(probe([note])).rejects.toMatchObject({ message: "native path probe failed: " + code });
        } finally { mock.mockRestore(); }
      }
    }
  });

  it("answers each path's physical spelling and kind, and null for missing or unsupported paths", async () => {
    const note = file("docs/note.md", "# note");
    fs.symlinkSync(path.join(root, "docs"), path.join(root, "linked"));
    const answers = await probe([note, path.join(root, "linked/./note.md"), path.join(root, "docs"), path.join(root, "missing.md"), "~/note.md"], root);
    expect(answers).toEqual([
      { real: note, kind: "file" },
      { real: note, kind: "file" },
      { real: path.join(root, "docs"), kind: "directory" },
      null,
      null,
    ]);
    // `~/` is the home the host was given.
    expect(await probe(["~/docs/note.md"], root)).toEqual([{ real: note, kind: "file" }]);
  });
});

describe("openRoute", () => {
  it("opens a plain document and a folder", async () => {
    expect(await openRoute(file("open/report.pdf", "%PDF-1.7"))).toEqual({ action: "open", kind: "file", reason: "document" });
    expect(await openRoute(file("open/notes.md", "# notes"))).toEqual({ action: "open", kind: "file", reason: "document" });
    expect(await openRoute(path.join(root, "open"))).toEqual({ action: "open", kind: "directory", reason: "folder" });
  });

  it("reveals every other type, a bundle, and a document that is a program", async () => {
    fs.mkdirSync(path.join(root, "Tool.app"));
    expect(await openRoute(path.join(root, "Tool.app"))).toMatchObject({ action: "reveal", reason: "bundle" });
    for (const name of ["setup.pkg", "run.command", "a.tcl", "b.pl", "c.py", "page.html", "sheet.csv", "disk.iso", "host.vncloc", "tool"]) {
      expect(await openRoute(file(name, "plain")), name).toMatchObject({ action: "reveal", reason: "type" });
    }
    // Windows keeps no execute permission (`chmod` sets only read-only), so a text file there is a document whatever its mode.
    const runnable = process.platform === "win32" ? { action: "open", reason: "document" } : { action: "reveal", reason: "execute_bit" };
    expect(await openRoute(file("runnable.txt", "plain", 0o755))).toMatchObject(runnable);
    expect(await openRoute(file("renamed.txt", new Uint8Array([0xcf, 0xfa, 0xed, 0xfe, 0, 0])))).toMatchObject({ action: "reveal", reason: "header" });
    expect(await openRoute(file("script.txt", "#!/bin/sh\necho hi"))).toMatchObject({ action: "reveal", reason: "header" });
  });

  it("refuses a path that is not its own physical spelling, so a link swapped in is never followed", async () => {
    const terminal = file("x.terminal", "<?xml version=\"1.0\"?>");
    fs.symlinkSync(terminal, path.join(root, "report.txt"));
    expect(await openRoute(path.join(root, "report.txt"))).toEqual({ action: "refuse", reason: "not_physical" });
    fs.symlinkSync(path.join(root, "open"), path.join(root, "shortcut"));
    expect(await openRoute(path.join(root, "shortcut/report.pdf"))).toEqual({ action: "refuse", reason: "not_physical" });
  });

  it("refuses a missing path", async () => {
    expect(await openRoute(path.join(root, "nope"))).toEqual({ action: "refuse", reason: "not_found" });
  });
});

describe("executableHeader", () => {
  it("knows Mach-O, fat, ELF, MZ and a script's #!", () => {
    for (const head of [[0xfe, 0xed, 0xfa, 0xcf], [0xca, 0xfe, 0xba, 0xbe], [0x7f, 0x45, 0x4c, 0x46], [0x4d, 0x5a, 0x90, 0x00], [0x23, 0x21, 0x2f, 0x62]]) {
      expect(executableHeader(new Uint8Array(head)), head.join(",")).toBe(true);
    }
    expect(executableHeader(new Uint8Array([0x25, 0x50, 0x44, 0x46]))).toBe(false);
    expect(executableHeader(new Uint8Array([]))).toBe(false);
  });
});
