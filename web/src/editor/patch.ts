export type PatchLine = {
  text: string;
  kind: "header" | "hunk" | "context" | "added" | "removed" | "meta";
  oldLine: number | null;
  newLine: number | null;
};

/** Parse the unified patch the core already supplies; do not synthesize files. */
export function patchLines(text: string): PatchLine[] {
  if (!text) return [];
  const lines = text.endsWith("\n") ? text.slice(0, -1).split("\n") : text.split("\n");
  const rows: PatchLine[] = [];
  let oldLine = 0;
  let newLine = 0;
  let inHunk = false;
  for (const line of lines) {
    if (line.startsWith("diff --git ")) inHunk = false;
    const hunk = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(line);
    if (hunk) {
      oldLine = Number(hunk[1]);
      newLine = Number(hunk[2]);
      inHunk = true;
      rows.push({ text: line, kind: "hunk", oldLine: null, newLine: null });
    } else if (!inHunk) {
      rows.push({ text: line, kind: "header", oldLine: null, newLine: null });
    } else if (line.startsWith("+")) {
      rows.push({ text: line, kind: "added", oldLine: null, newLine: newLine++ });
    } else if (line.startsWith("-")) {
      rows.push({ text: line, kind: "removed", oldLine: oldLine++, newLine: null });
    } else if (line.startsWith(" ")) {
      rows.push({ text: line, kind: "context", oldLine: oldLine++, newLine: newLine++ });
    } else {
      rows.push({ text: line, kind: "meta", oldLine: null, newLine: null });
    }
  }
  return rows;
}
