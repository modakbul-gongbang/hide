import { describe, expect, it } from "vitest";
import { decorationFor, explorerRows, gitDecorations, relativeTo, rowTitle } from "./explorer";
import type { ChangedFileStatus, ChangesSnapshot } from "./snapshot";
import type { DirectoryList } from "./store";

const ROOT = "/checkout/hide";

function listing(folder: string, entries: [string, boolean][]): DirectoryList {
  return {
    kind: "file_list",
    root_path: folder,
    entries: entries.map(([name, isDirectory]) => ({
      name,
      path: `${folder}/${name}`,
      is_directory: isDirectory,
    })),
    truncated: false,
  };
}

function changes(entries: [string, ChangedFileStatus][]): ChangesSnapshot {
  return {
    root_path: ROOT,
    entries: entries.map(([relative, status]) => ({
      path: `${ROOT}/${relative}`,
      relative_path: relative,
      previous_relative_path: null,
      status,
      added_lines: null,
      removed_lines: null,
    })),
    committed: [],
    base_branch: null,
    selected_path: null,
    selected_committed: false,
    unavailable_reason: null,
  };
}

const TOP = listing(ROOT, [
  ["src", true],
  ["README.md", false],
  [".gitignore", false],
]);
const SRC = listing(`${ROOT}/src`, [
  ["a.ts", false],
  ["nested", true],
]);

describe("explorerRows", () => {
  it("shows a folder's children in the order hided sent them, at depth zero", () => {
    const rows = explorerRows({ rootPath: ROOT, listings: { [ROOT]: TOP }, expandedPaths: [], changes: null });
    expect(rows.map((row) => row.name)).toEqual(["src", "README.md", ".gitignore"]);
    expect(rows.every((row) => row.depth === 0)).toBe(true);
    expect(rows.map((row) => row.isDirectory)).toEqual([true, false, false]);
  });

  it("walks into a folder the core says is expanded and indents its children", () => {
    const rows = explorerRows({
      rootPath: ROOT,
      listings: { [ROOT]: TOP, [`${ROOT}/src`]: SRC },
      expandedPaths: [`${ROOT}/src`],
      changes: null,
    });
    expect(rows.map((row) => `${row.depth}:${row.name}`)).toEqual([
      "0:src",
      "1:a.ts",
      "1:nested",
      "0:README.md",
      "0:.gitignore",
    ]);
    expect(rows.map((row) => row.expanded)).toEqual([true, false, false, false, false]);
    expect(rows.map((row) => row.listing?.root_path ?? null)).toEqual([
      `${ROOT}/src`,
      null,
      null,
      null,
      null,
    ]);
  });

  it("keeps a folder closed when the core has not expanded it, listing or not", () => {
    const rows = explorerRows({
      rootPath: ROOT,
      listings: { [ROOT]: TOP, [`${ROOT}/src`]: SRC },
      expandedPaths: [],
      changes: null,
    });
    expect(rows.map((row) => row.name)).toEqual(["src", "README.md", ".gitignore"]);
    expect(rows.map((row) => row.expanded)).toEqual([false, false, false]);
  });

  it("shows no child under an expanded folder whose listing has not arrived", () => {
    const rows = explorerRows({
      rootPath: ROOT,
      listings: { [ROOT]: TOP },
      expandedPaths: [`${ROOT}/src`],
      changes: null,
    });
    expect(rows.map((row) => row.name)).toEqual(["src", "README.md", ".gitignore"]);
  });

  it("shows nothing at all before the root's own listing arrives", () => {
    expect(explorerRows({ rootPath: ROOT, listings: {}, expandedPaths: [], changes: null })).toEqual([]);
  });
});

describe("git decorations", () => {
  it("gives a file the letter Git names its status with", () => {
    const decorations = gitDecorations(changes([["src/a.ts", "modified"]]), ROOT);
    expect(decorationFor(decorations, `${ROOT}/src/a.ts`, ROOT, false)).toEqual({
      badge: "M",
      title: "Modified",
      status: "modified",
    });
  });

  it("gives a folder the dot and its riskiest descendant's status", () => {
    const decorations = gitDecorations(
      changes([
        ["src/a.ts", "modified"],
        ["src/nested/b.ts", "conflict"],
        ["src/nested/c.ts", "untracked"],
      ]),
      ROOT,
    );
    expect(decorationFor(decorations, `${ROOT}/src`, ROOT, true)).toEqual({
      badge: "●",
      title: "Contains changed files; highest priority is conflict",
      status: "conflict",
    });
    expect(decorationFor(decorations, `${ROOT}/src/nested`, ROOT, true)?.status).toBe("conflict");
  });

  it("marks nothing under a folder with no changed descendant", () => {
    const decorations = gitDecorations(changes([["src/a.ts", "modified"]]), ROOT);
    expect(decorationFor(decorations, `${ROOT}/src/nested`, ROOT, true)).toBeNull();
    expect(decorationFor(decorations, `${ROOT}/README.md`, ROOT, false)).toBeNull();
    expect(decorationFor(gitDecorations(null, ROOT), `${ROOT}/src/a.ts`, ROOT, false)).toBeNull();
  });

  it("puts the relative path and the status title in the row's tooltip", () => {
    const rows = explorerRows({
      rootPath: ROOT,
      listings: { [ROOT]: TOP },
      expandedPaths: [],
      changes: changes([["src/a.ts", "deleted"]]),
    });
    expect(rows.map((row) => rowTitle(row, ROOT))).toEqual([
      "src · Contains changed files; highest priority is deleted",
      "README.md",
      ".gitignore",
    ]);
  });

  it("presents the root itself as a dot", () => {
    expect(relativeTo(ROOT, ROOT)).toBe(".");
    expect(relativeTo(`${ROOT}/src/a.ts`, ROOT)).toBe("src/a.ts");
    expect(relativeTo("/elsewhere/a.ts", ROOT)).toBe("/elsewhere/a.ts");
  });
});
