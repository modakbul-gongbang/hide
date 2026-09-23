// A document's leading YAML frontmatter, found by text alone so the rule is
// testable without a browser. Live mode draws the front half in its own pane
// (PRD S3 D-11); source mode and every other document kind keep one buffer.

export type Frontmatter = {
  /** The block, delimiters included, ending in a newline. */
  front: string;
  /** Everything after it; the body keeps the document's remaining lines. */
  body: string;
};

/**
 * The frontmatter at the very top of `contents`, or null when there is none.
 *
 * Only a first line that is exactly `---` opens a block and only a later
 * `---` line closes it, which is the rule YAML frontmatter is written with;
 * an unclosed block is body text, because there is no block to draw.
 */
export function splitFrontmatter(contents: string): Frontmatter | null {
  const lines = contents.split("\n");
  if ((lines[0] ?? "").trimEnd() !== "---") return null;
  for (let index = 1; index < lines.length; index += 1) {
    if ((lines[index] ?? "").trimEnd() !== "---") continue;
    const front = `${lines.slice(0, index + 1).join("\n")}\n`;
    return { front, body: contents.slice(front.length) };
  }
  return null;
}
