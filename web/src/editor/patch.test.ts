import { describe, expect, it } from "vitest";
import { patchLines } from "./patch";

describe("unified patch rows", () => {
  it("assigns old and new numbers only to hunk contents across multiple hunks", () => {
    const rows = patchLines([
      "diff --git a/a.txt b/a.txt",
      "--- a/a.txt",
      "+++ b/a.txt",
      "@@ -3,2 +3,3 @@",
      " before",
      "-old",
      "+new",
      "+extra",
      "\\ No newline at end of file",
      "@@ -10 +11 @@",
      " tail",
      "",
    ].join("\n"));
    expect(rows.map(({ kind, oldLine, newLine }) => [kind, oldLine, newLine])).toEqual([
      ["header", null, null],
      ["header", null, null],
      ["header", null, null],
      ["hunk", null, null],
      ["context", 3, 3],
      ["removed", 4, null],
      ["added", null, 4],
      ["added", null, 5],
      ["meta", null, null],
      ["hunk", null, null],
      ["context", 10, 11],
    ]);
  });

  it("keeps binary summaries and successful empty patches free of invented lines", () => {
    expect(patchLines("Binary files a/icon.png and b/icon.png differ\n")[0]).toMatchObject({ kind: "header", oldLine: null, newLine: null });
    expect(patchLines("")).toEqual([]);
  });
});
