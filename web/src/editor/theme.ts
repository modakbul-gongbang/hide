// The editor's chrome and syntax colors, taken from the design tokens the
// generated CSS already carries. No hex value is written here: every color is
// a `var(--color-*)`, so a token change moves the editor with the shell
// (`scripts/check-web-tokens.mjs` refuses a literal).

import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { EditorView } from "@codemirror/view";
import { tags as t } from "@lezer/highlight";

/** The base chrome: transparent over the shell background, with a caret and
 * selection that read against it, and a search panel in the shell's palette. */
export const baseTheme = [
  EditorView.theme(
    {
      "&": {
        color: "var(--color-primary)",
        backgroundColor: "var(--color-background)",
        height: "100%",
      },
      ".cm-content": {
        caretColor: "var(--color-accent)",
        fontFamily: "var(--font-mono)",
      },
      ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--color-accent)" },
      "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
        { backgroundColor: "var(--color-elevated)" },
      ".cm-activeLine": { backgroundColor: "var(--color-panel)" },
      ".cm-gutters": {
        backgroundColor: "var(--color-background)",
        color: "var(--color-muted)",
        border: "none",
      },
      ".cm-activeLineGutter": { backgroundColor: "var(--color-panel)", color: "var(--color-secondary)" },
      ".cm-panels": { backgroundColor: "var(--color-panel)", color: "var(--color-primary)" },
      ".cm-panels.cm-panels-top": { borderBottom: "1px solid var(--color-divider)" },
      ".cm-searchMatch": { backgroundColor: "var(--color-elevated)", outline: "1px solid var(--color-divider)" },
      ".cm-searchMatch.cm-searchMatch-selected": { backgroundColor: "var(--color-balloon)" },
      ".cm-button": {
        backgroundImage: "none",
        backgroundColor: "var(--color-elevated)",
        color: "var(--color-primary)",
        border: "1px solid var(--color-divider)",
      },
      ".cm-textfield": {
        backgroundColor: "var(--color-background)",
        color: "var(--color-primary)",
        border: "1px solid var(--color-divider)",
      },
      ".cm-tooltip": { backgroundColor: "var(--color-balloon)", border: "1px solid var(--color-divider)" },
    },
    { dark: true },
  ),
  syntaxHighlighting(
    HighlightStyle.define([
      { tag: [t.comment, t.lineComment, t.blockComment], color: "var(--color-muted)" },
      { tag: [t.keyword, t.modifier, t.controlKeyword, t.operatorKeyword], color: "var(--color-file-purple)" },
      { tag: [t.string, t.special(t.string)], color: "var(--color-file-green)" },
      { tag: [t.number, t.bool, t.null], color: "var(--color-file-orange)" },
      { tag: [t.function(t.variableName), t.function(t.propertyName)], color: "var(--color-file-yellow)" },
      { tag: [t.typeName, t.className, t.namespace], color: "var(--color-file-blue)" },
      { tag: [t.propertyName, t.attributeName], color: "var(--color-file-blue)" },
      { tag: [t.variableName, t.definition(t.variableName)], color: "var(--color-primary)" },
      { tag: [t.tagName], color: "var(--color-file-orange)" },
      { tag: [t.heading], color: "var(--color-file-blue)", fontWeight: "bold" },
      { tag: [t.emphasis], fontStyle: "italic" },
      { tag: [t.strong], fontWeight: "bold" },
      { tag: [t.link, t.url], color: "var(--color-file-blue)", textDecoration: "underline" },
      { tag: [t.invalid], color: "var(--color-danger)" },
    ]),
  ),
];

/** The text scale the core holds for the editor (`ui_state.editor_text_scale`). */
export function scaleTheme(scale: number) {
  return EditorView.theme({
    ".cm-content": {
      fontSize: `calc(var(--text-editor-document) * ${scale})`,
      lineHeight: "1.5",
    },
    ".cm-gutters": { fontSize: `calc(var(--text-editor-base) * ${scale})` },
  });
}

/**
 * Markdown Live's drawn styles (PRD B6): the block and inline classes the plan
 * emits. Heading sizes are the editor heading tokens, so Live reads at the same
 * sizes as the Swift view; a hidden fence line collapses to no height.
 */
export const liveTheme = EditorView.theme({
  ".cm-md-heading-1": { fontSize: "var(--text-editor-heading-1)", fontWeight: "600", lineHeight: "1.3" },
  ".cm-md-heading-2": { fontSize: "var(--text-editor-heading-2)", fontWeight: "600", lineHeight: "1.3" },
  ".cm-md-heading-3": { fontSize: "var(--text-editor-heading-3)", fontWeight: "600", lineHeight: "1.3" },
  ".cm-md-heading-4": { fontSize: "var(--text-editor-heading-4)", fontWeight: "600", lineHeight: "1.4" },
  ".cm-md-heading-5": { fontSize: "var(--text-editor-heading-5)", fontWeight: "600", lineHeight: "1.4" },
  ".cm-md-heading-6": { fontSize: "var(--text-editor-heading-6)", fontWeight: "600", lineHeight: "1.4" },
  ".cm-md-code": { fontFamily: "var(--font-mono)", backgroundColor: "var(--color-panel)" },
  ".cm-md-hidden-line": { display: "none" },
  ".cm-md-quote": {
    borderLeft: "var(--size-editor-quote-rule) solid var(--color-divider)",
    paddingLeft: "var(--spacing-sm)",
    color: "var(--color-secondary)",
  },
  ".cm-md-link": { color: "var(--color-file-blue)", textDecoration: "underline" },
  ".cm-md-code-inline": { fontFamily: "var(--font-mono)", backgroundColor: "var(--color-panel)" },
  ".cm-md-bullet": { color: "var(--color-secondary)" },
  ".cm-md-checkbox": { verticalAlign: "middle" },
});
