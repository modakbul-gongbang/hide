// An agent's message on the phone drawn as Markdown: parsed once per message
// with the grammar the editor's Markdown view uses (@lezer/markdown with GFM)
// and drawn as plain elements, never an editor, so a page of messages costs a
// few milliseconds. Nothing on the page navigates: a link is its text and an
// image its description, as the terminal rows keep OSC 8 links as text. Code
// wraps at the phone's width like everything else, without highlighting.

import type { SyntaxNode } from "@lezer/common";
import { GFM, parser } from "@lezer/markdown";
import type { ReactNode } from "react";

const markdown = parser.configure(GFM);

/** The syntax the drawing stands in for: `**`, `#`, `-`, `>`, backticks, brackets, pipes, a fence's language. */
const MARKS = new Set(["EmphasisMark", "CodeMark", "HeaderMark", "ListMark", "QuoteMark", "LinkMark", "StrikethroughMark", "TableDelimiter", "CodeInfo"]);

/** What a link or an image carries besides its text. */
const TARGETS = new Set(["URL", "LinkTitle", "LinkLabel"]);

const HEADINGS = /^(ATXHeading[1-6]|SetextHeading[12])$/;

export function renderMarkdown(text: string): ReactNode {
  return blocks(markdown.parse(text).topNode, text);
}

function childrenOf(node: SyntaxNode): SyntaxNode[] {
  const children: SyntaxNode[] = [];
  for (let child = node.firstChild; child; child = child.nextSibling) children.push(child);
  return children;
}

/** A container's blocks; the whitespace between them is layout, not text. */
function blocks(node: SyntaxNode, text: string): ReactNode[] {
  return childrenOf(node).map((child, index) => block(child, text, index));
}

function block(node: SyntaxNode, text: string, key: number): ReactNode {
  const name = node.name;
  if (MARKS.has(name)) return null;
  if (HEADINGS.test(name)) {
    return (
      <p key={key} className="whitespace-pre-line font-semibold">
        {trimmed(inline(node, text))}
      </p>
    );
  }
  switch (name) {
    case "BulletList":
      return (
        <ul key={key} className="list-disc space-y-xs pl-lg">
          {blocks(node, text)}
        </ul>
      );
    case "OrderedList": {
      const mark = node.getChild("ListItem")?.getChild("ListMark");
      const start = mark ? Number.parseInt(text.slice(mark.from, mark.to), 10) : 1;
      return (
        <ol key={key} start={Number.isFinite(start) ? start : 1} className="list-decimal space-y-xs pl-lg">
          {blocks(node, text)}
        </ol>
      );
    }
    case "ListItem":
      return (
        <li key={key} className="space-y-xs">
          {blocks(node, text)}
        </li>
      );
    case "Blockquote":
      return (
        <blockquote key={key} className="space-y-xs border-l-2 border-border pl-md text-muted-foreground">
          {blocks(node, text)}
        </blockquote>
      );
    case "FencedCode":
    case "CodeBlock":
      return (
        <pre key={key} className="m-none whitespace-pre-wrap wrap-anywhere rounded-md bg-secondary px-md py-sm font-mono">
          {codeOf(node, text)}
        </pre>
      );
    case "HorizontalRule":
      return <hr key={key} className="border-border" />;
    case "Table":
      return (
        <table key={key} className="w-full border-collapse">
          <tbody>{childrenOf(node).map((row, index) => tableRow(row, text, index))}</tbody>
        </table>
      );
    default:
      // A paragraph, a task, and anything the grammar knows that has no drawing of its own (raw HTML, a link reference).
      return (
        <p key={key} className="whitespace-pre-line">
          {inline(node, text)}
        </p>
      );
  }
}

function tableRow(row: SyntaxNode, text: string, key: number): ReactNode {
  if (row.name !== "TableHeader" && row.name !== "TableRow") return null;
  const Cell = row.name === "TableHeader" ? "th" : "td";
  return (
    <tr key={key}>
      {row.getChildren("TableCell").map((cell, index) => (
        <Cell key={index} className={`border border-border px-sm py-xs text-left align-top wrap-anywhere ${Cell === "th" ? "font-semibold" : ""}`}>
          {inline(cell, text)}
        </Cell>
      ))}
    </tr>
  );
}

/** The code between a fence's marks, or an indented block's lines. */
function codeOf(node: SyntaxNode, text: string): string {
  const lines = node.getChildren("CodeText");
  const first = lines[0];
  const last = lines.at(-1);
  if (!first || !last) return "";
  const code = text.slice(first.from, last.to);
  return node.name === "CodeBlock" ? code.replace(/^ {1,4}/gm, "") : code;
}

/** A run of inline text: what lies between the child nodes is the text itself. */
function inline(node: SyntaxNode, text: string, skip: ReadonlySet<string> = MARKS): ReactNode[] {
  const out: ReactNode[] = [];
  let at = node.from;
  for (const child of childrenOf(node)) {
    if (child.from > at) out.push(text.slice(at, child.from));
    if (!skip.has(child.name) && !MARKS.has(child.name)) out.push(span(child, text, out.length));
    at = child.to;
  }
  if (node.to > at) out.push(text.slice(at, node.to));
  return out;
}

const LINK_PARTS = new Set([...MARKS, ...TARGETS]);

function span(node: SyntaxNode, text: string, key: number): ReactNode {
  switch (node.name) {
    case "Emphasis":
      return <em key={key}>{inline(node, text)}</em>;
    case "StrongEmphasis":
      return (
        <strong key={key} className="font-semibold">
          {inline(node, text)}
        </strong>
      );
    case "Strikethrough":
      return <s key={key}>{inline(node, text)}</s>;
    case "InlineCode":
      return (
        <code key={key} className="rounded-xs bg-secondary px-xxs font-mono">
          {inline(node, text)}
        </code>
      );
    case "Link":
    case "Image":
      return <span key={key}>{inline(node, text, LINK_PARTS)}</span>;
    case "Autolink":
      return <span key={key}>{inline(node, text)}</span>;
    case "HardBreak":
      return <br key={key} />;
    case "Escape":
      return text.slice(node.from + 1, node.to);
    default:
      // A bare URL, an HTML tag, an entity, a task's box: as written.
      return text.slice(node.from, node.to);
  }
}

/** A heading without the space after its `#` or the line before its underline. */
function trimmed(parts: ReactNode[]): ReactNode[] {
  const first = parts[0];
  if (typeof first === "string") parts[0] = first.trimStart();
  const end = parts.length - 1;
  const last = parts[end];
  if (typeof last === "string") parts[end] = last.trimEnd();
  return parts;
}
