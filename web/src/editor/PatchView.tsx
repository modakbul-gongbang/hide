import { EditorState } from "@codemirror/state";
import { Decoration, EditorView, gutter, GutterMarker } from "@codemirror/view";
import { useEffect, useRef } from "react";
import { patchLines } from "./patch";
import { baseTheme, scaleTheme } from "./theme";

class NumberMarker extends GutterMarker {
  constructor(readonly value: number) { super(); }
  toDOM(): HTMLElement {
    const span = document.createElement("span");
    span.textContent = String(this.value);
    return span;
  }
}

const patchTheme = EditorView.theme({
  "&": { overflow: "hidden" },
  ".cm-scroller": { overflow: "auto", fontFamily: "var(--font-mono)" },
  ".cm-content": { minWidth: "max-content", paddingTop: "0", whiteSpace: "pre" },
  ".cm-line": { width: "max-content", minWidth: "100%" },
  ".cm-gutter": { width: "var(--size-editor-diff-line-number-col)", textAlign: "right" },
  ".cm-gutterElement": { paddingRight: "var(--spacing-xs)" },
  ".cm-patch-added": { backgroundColor: "color-mix(in srgb, var(--color-diff-added) 15%, transparent)" },
  ".cm-patch-removed": { backgroundColor: "color-mix(in srgb, var(--color-diff-removed) 15%, transparent)" },
  ".cm-patch-hunk": { backgroundColor: "var(--color-panel)", color: "var(--color-file-blue)" },
  ".cm-patch-header": { color: "var(--color-muted)" },
});

export function PatchView({ text, scale }: { text: string; scale: number }) {
  const host = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!host.current || !text) return;
    const rows = patchLines(text);
    const doc = text.endsWith("\n") ? text.slice(0, -1) : text;
    let from = 0;
    const decorations = Decoration.set(rows.flatMap((row) => {
      const position = from;
      from += row.text.length + 1;
      return row.kind === "context" || row.kind === "meta"
        ? []
        : [Decoration.line({ attributes: { class: `cm-patch-${row.kind}` } }).range(position)];
    }));
    const state = EditorState.create({
      doc,
      extensions: [
        ...baseTheme,
        patchTheme,
        scaleTheme(scale),
        EditorState.readOnly.of(true),
        EditorView.editable.of(false),
        gutter({ class: "cm-patch-old", lineMarker: (_view, line) => {
          const number = rows[_view.state.doc.lineAt(line.from).number - 1]?.oldLine;
          return number === null || number === undefined ? null : new NumberMarker(number);
        } }),
        gutter({ class: "cm-patch-new", lineMarker: (_view, line) => {
          const number = rows[_view.state.doc.lineAt(line.from).number - 1]?.newLine;
          return number === null || number === undefined ? null : new NumberMarker(number);
        } }),
        EditorView.decorations.of(decorations),
      ],
    });
    const view = new EditorView({ state, parent: host.current });
    return () => view.destroy();
  }, [text, scale]);
  return <div ref={host} className="min-h-0 min-w-0 flex-1" data-patch-view="true" />;
}
