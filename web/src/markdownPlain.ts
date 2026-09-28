// A Markdown text as the words it shows, for a line-clamped preview that has
// no room for the editor's Live view (PRD overview-lenses-issues B8): the
// same parser the Markdown document uses drops the marks, and each line is
// trimmed with the blank ones gone.

import { GFM, parser } from "@lezer/markdown";

const markdown = parser.configure(GFM);

/** The syntax that is punctuation, not words. */
const MARKS = new Set(["HeaderMark", "EmphasisMark", "ListMark", "QuoteMark", "CodeMark", "CodeInfo", "LinkMark", "URL", "TaskMarker", "StrikethroughMark"]);

export function markdownPlainText(text: string): string {
  let plain = "";
  let at = 0;
  markdown.parse(text).iterate({
    enter: (node) => {
      if (!MARKS.has(node.name)) return;
      plain += text.slice(at, node.from);
      at = node.to;
      return false;
    },
  });
  plain += text.slice(at);
  return plain
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .join("\n");
}
