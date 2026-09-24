import { describe, expect, it } from "vitest";
import {
  decorationFor,
  explorerGitLine,
  explorerRows,
  firstChildSelection,
  gitDecorations,
  moveSelection,
  parentPath,
  parentSelection,
  relativeTo,
  rowForPath,
  rowTitle,
  selectionAfterRemoval,
} from "./explorer";
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
    diff: null,
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
  it("places a registered subfolder's changed file under its actual Explorer path", () => {
    const scoped = changes([["inside.txt", "modified"]]);
    scoped.root_path = `${ROOT}/registered`;
    scoped.entries[0]!.path = `${ROOT}/registered/inside.txt`;
    const decorations = gitDecorations(scoped, ROOT);
    expect(decorationFor(decorations, `${ROOT}/registered`, ROOT, true)?.status).toBe("modified");
    expect(decorationFor(decorations, `${ROOT}/registered/inside.txt`, ROOT, false)?.status).toBe("modified");
    expect(decorationFor(decorations, `${ROOT}/inside.txt`, ROOT, false)).toBeNull();
    expect(gitDecorations(scoped, "/another-checkout").files.size).toBe(0);
  });

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

describe("tree navigation", () => {
  const rows = explorerRows({
    rootPath: ROOT,
    listings: { [ROOT]: TOP, [`${ROOT}/src`]: SRC },
    expandedPaths: [`${ROOT}/src`],
    changes: null,
  });

  it("moves the cursor one row and clamps at both ends", () => {
    expect(moveSelection(rows, null, 1)).toBe(`${ROOT}/src`);
    expect(moveSelection(rows, `${ROOT}/src`, 1)).toBe(`${ROOT}/src/a.ts`);
    expect(moveSelection(rows, `${ROOT}/src/a.ts`, 1)).toBe(`${ROOT}/src/nested`);
    expect(moveSelection(rows, `${ROOT}/src/a.ts`, -1)).toBe(`${ROOT}/src`);
    expect(moveSelection(rows, `${ROOT}/src`, -1)).toBe(`${ROOT}/src`);
    expect(moveSelection(rows, `${ROOT}/.gitignore`, 1)).toBe(`${ROOT}/.gitignore`);
    expect(moveSelection([], `${ROOT}/src`, 1)).toBeNull();
  });

  it("moves the cursor to the enclosing folder, skipping a deeper sibling", () => {
    expect(parentSelection(rows, `${ROOT}/src/a.ts`)).toBe(`${ROOT}/src`);
    expect(parentSelection(rows, `${ROOT}/src/nested`)).toBe(`${ROOT}/src`);
    expect(parentSelection(rows, `${ROOT}/src`)).toBeNull();
    expect(parentSelection(rows, null)).toBeNull();
  });

  it("names the first child only while the folder is expanded with rows under it", () => {
    expect(firstChildSelection(rows, `${ROOT}/src`)).toBe(`${ROOT}/src/a.ts`);
    expect(firstChildSelection(rows, `${ROOT}/README.md`)).toBeNull();
    expect(firstChildSelection(rows, `${ROOT}/.gitignore`)).toBeNull();
  });

  it("finds the row a path names and reports a path the tree does not show", () => {
    expect(rowForPath(rows, `${ROOT}/src/a.ts`)?.name).toBe("a.ts");
    expect(rowForPath(rows, `${ROOT}/gone`)).toBeNull();
    expect(rowForPath(rows, null)).toBeNull();
  });
});

describe("removal selection", () => {
  const rows = explorerRows({
    rootPath: ROOT,
    listings: { [ROOT]: TOP, [`${ROOT}/src`]: SRC },
    expandedPaths: [`${ROOT}/src`],
    changes: null,
  });

  it("names the folder a path sits in", () => {
    expect(parentPath(`${ROOT}/src/a.ts`)).toBe(`${ROOT}/src`);
    expect(parentPath(`${ROOT}/README.md`)).toBe(ROOT);
  });

  it("selects the next sibling, else the previous sibling, else the parent", () => {
    // `src` is followed by README.md, so removing it selects README.md.
    expect(selectionAfterRemoval(rows, `${ROOT}/src`, ROOT)).toBe(`${ROOT}/README.md`);
    // `.gitignore` is last, so removing it selects its previous sibling.
    expect(selectionAfterRemoval(rows, `${ROOT}/.gitignore`, ROOT)).toBe(`${ROOT}/README.md`);
    // The folder's first child is followed by its sibling.
    expect(selectionAfterRemoval(rows, `${ROOT}/src/a.ts`, ROOT)).toBe(`${ROOT}/src/nested`);
    expect(selectionAfterRemoval(rows, `${ROOT}/missing`, ROOT)).toBe(ROOT);
  });
});

describe("the Explorer's Git status line (S5.5 B20, B22)", () => {
  const base = { root_path: "/repo", entries: [], committed: [], selected_path: null, diff: null } as unknown as ChangesSnapshot;
  it("says a failed read is unavailable or out of date and never draws it as current", () => {
    expect(explorerGitLine(null)).toEqual({ state: "loading", text: "Loading Git status" });
    expect(explorerGitLine({ ...base, unavailable_reason: "git is not installed" })?.text).toBe("Git status unavailable: git is not installed");
    expect(explorerGitLine({ ...base, stale_reason: "git status timed out" })?.state).toBe("stale");
    expect(explorerGitLine(base)).toBeNull();
  });
});
