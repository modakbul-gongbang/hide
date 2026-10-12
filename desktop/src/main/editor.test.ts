import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterAll, describe, expect, it } from "vitest";
import { editorTarget, findEditor, VSCODE_BUNDLE_ID, type EditorProbeDeps } from "./editor";
import type { ChildResult } from "./spawn";

const answered = (stdout: string, code = 0): ChildResult => ({ code, signal: null, stdout, stderr: "", timedOut: false, spawnError: null });

function deps(platform: NodeJS.Platform, files: string[], spotlight?: ChildResult): EditorProbeDeps & { runs: string[][] } {
  const runs: string[][] = [];
  return {
    platform,
    home: "/Users/alice",
    searchPath: platform === "win32" ? ["C:\\Tools", "C:\\VSCode\\bin"] : ["/usr/local/bin", "/usr/bin"],
    exists: (file) => files.includes(file),
    isExecutable: (file) => files.includes(file),
    run: async (file, args) => {
      runs.push([file, ...args]);
      if (!spotlight) throw new Error("no child expected");
      return spotlight;
    },
    runs,
  };
}

describe("findEditor", () => {
  it("finds VS Code on macOS in either Applications folder without starting anything, and opens by bundle id", async () => {
    const probe = deps("darwin", ["/Users/alice/Applications/Visual Studio Code.app"]);
    expect(await findEditor(probe)).toEqual({ file: "/usr/bin/open", args: ["-b", VSCODE_BUNDLE_ID], detached: false, via: "bundle" });
    expect(probe.runs).toEqual([]);
  });

  it("asks Spotlight for the bundle id elsewhere on macOS, and says why it found none", async () => {
    expect(await findEditor(deps("darwin", [], answered("/Volumes/Apps/Visual Studio Code.app\n")))).toMatchObject({ via: "spotlight" });
    expect(await findEditor(deps("darwin", [], answered("")))).toEqual({ missing: "not_installed" });
    expect(await findEditor(deps("darwin", [], { ...answered(""), timedOut: true }))).toEqual({ missing: "spotlight_timed_out" });
    expect(await findEditor(deps("darwin", [], answered("", 1)))).toEqual({ missing: "spotlight_exit" });
  });

  it("takes the `code` launcher on Linux and `Code.exe` beside `code.cmd` on Windows, detached", async () => {
    expect(await findEditor(deps("linux", ["/usr/bin/code"]))).toEqual({ file: "/usr/bin/code", args: [], detached: false, via: "path" });
    expect(await findEditor(deps("linux", []))).toEqual({ missing: "not_installed" });
    const exe = path.resolve("C:\\VSCode\\bin", "..", "Code.exe");
    expect(await findEditor(deps("win32", [path.join("C:\\VSCode\\bin", "code.cmd"), exe]))).toEqual({ file: exe, args: [], detached: true, via: "path" });
    expect(await findEditor(deps("win32", [path.join("C:\\VSCode\\bin", "code.cmd")]))).toEqual({ missing: "not_installed" });
  });
});

describe("editorTarget", () => {
  const root = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "hide-editor-")));
  afterAll(() => fs.rmSync(root, { recursive: true, force: true }));
  const page = (native: string): string => native.split(path.sep).join("/");

  it("hands over a folder that exists, in this system's spelling", async () => {
    expect(await editorTarget(page(root))).toEqual({ path: root });
  });

  it("refuses a relative path, a file and a folder that is gone", async () => {
    const file = path.join(root, "notes.md");
    fs.writeFileSync(file, "notes");
    expect(await editorTarget("checkout")).toEqual({ refused: "path" });
    expect(await editorTarget(page(file))).toEqual({ refused: "not_directory" });
    expect(await editorTarget(page(path.join(root, "gone")))).toEqual({ refused: "missing" });
  });
});
