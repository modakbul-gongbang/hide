// The editor's chrome and syntax colors, taken from the design tokens the
// generated CSS already carries. No hex value is written here: every color is
// a `var(--<token>)`, so a token change moves the editor with the shell
// (`scripts/check-web-tokens.mjs` refuses a literal).

import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { Compartment } from "@codemirror/state";
import { EditorView, ViewPlugin } from "@codemirror/view";
import { tags as t } from "@lezer/highlight";

// CodeMirror draws a few things of its own (bracket and selection matches, fold
// markers) in a light or a dark variant; which one follows the page theme, so
// every open editor is told when the theme changes and a new one starts on it.
const appearance = new Compartment();
const openEditors = new Set<EditorView>();
const tracked = ViewPlugin.define((view) => {
  openEditors.add(view);
  return { destroy: () => openEditors.delete(view) };
});

function pageIsDark(): boolean {
  return !document.documentElement.classList.contains("light");
}

/** Re-theme every open editor after the page theme changed; content, selection and history stay. */
export function applyEditorTheme() {
  const effects = appearance.reconfigure(EditorView.darkTheme.of(pageIsDark()));
  for (const view of openEditors) view.dispatch({ effects });
}

/** The base chrome: transparent over the shell background, with a caret and
 * selection that read against it, and a search panel in the shell's palette. */
export const baseTheme = () => [
  appearance.of(EditorView.darkTheme.of(pageIsDark())),
  tracked,
  EditorView.theme(
    {
      "&": {
        color: "var(--foreground)",
        backgroundColor: "var(--background)",
        height: "100%",
      },
      ".cm-content": {
        caretColor: "var(--primary)",
        fontFamily: "var(--font-mono)",
      },
      ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--primary)" },
      "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
        { backgroundColor: "var(--secondary)" },
      ".cm-activeLine": { backgroundColor: "var(--card)" },
      ".cm-gutters": {
        backgroundColor: "var(--background)",
        color: "var(--muted-foreground)",
        border: "none",
      },
      ".cm-activeLineGutter": { backgroundColor: "var(--card)", color: "var(--subtle-foreground)" },
      ".cm-panels": { backgroundColor: "var(--card)", color: "var(--foreground)" },
      ".cm-panels.cm-panels-top": { borderBottom: "var(--size-hairline) solid var(--border)" },
      ".cm-searchMatch": { backgroundColor: "var(--secondary)", outline: "var(--size-hairline) solid var(--border)" },
      ".cm-searchMatch.cm-searchMatch-selected": { backgroundColor: "var(--popover)" },
      ".cm-button": {
        backgroundImage: "none",
        backgroundColor: "var(--secondary)",
        color: "var(--foreground)",
        border: "var(--size-hairline) solid var(--border)",
      },
      ".cm-textfield": {
        backgroundColor: "var(--background)",
        color: "var(--foreground)",
        border: "var(--size-hairline) solid var(--border)",
      },
      ".cm-tooltip": { backgroundColor: "var(--popover)", border: "var(--size-hairline) solid var(--border)" },
    },
  ),
  syntaxHighlighting(
    HighlightStyle.define([
      { tag: [t.comment, t.lineComment, t.blockComment], color: "var(--muted-foreground)" },
      { tag: [t.keyword, t.modifier, t.controlKeyword, t.operatorKeyword], color: "var(--file-purple)" },
      { tag: [t.string, t.special(t.string)], color: "var(--file-green)" },
      { tag: [t.number, t.bool, t.null], color: "var(--file-orange)" },
      { tag: [t.function(t.variableName), t.function(t.propertyName)], color: "var(--file-yellow)" },
      { tag: [t.typeName, t.className, t.namespace], color: "var(--file-blue)" },
      { tag: [t.propertyName, t.attributeName], color: "var(--file-blue)" },
      { tag: [t.variableName, t.definition(t.variableName)], color: "var(--foreground)" },
      { tag: [t.tagName], color: "var(--file-orange)" },
      { tag: [t.heading], color: "var(--file-blue)", fontWeight: "bold" },
      { tag: [t.emphasis], fontStyle: "italic" },
      { tag: [t.strong], fontWeight: "bold" },
      { tag: [t.link, t.url], color: "var(--file-blue)", textDecoration: "underline" },
      { tag: [t.invalid], color: "var(--destructive)" },
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
  ".cm-md-code": { fontFamily: "var(--font-mono)", backgroundColor: "var(--card)" },
  ".cm-md-hidden-line": { display: "none" },
  ".cm-md-quote": {
    borderLeft: "var(--size-editor-quote-rule) solid var(--border)",
    paddingLeft: "var(--spacing-sm)",
    color: "var(--subtle-foreground)",
  },
  ".cm-md-link": { color: "var(--file-blue)", textDecoration: "underline" },
  ".cm-md-code-inline": { fontFamily: "var(--font-mono)", backgroundColor: "var(--card)" },
  ".cm-md-bullet": { color: "var(--subtle-foreground)" },
  ".cm-md-checkbox": { verticalAlign: "middle" },
});
