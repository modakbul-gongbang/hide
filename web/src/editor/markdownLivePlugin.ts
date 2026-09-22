// The Markdown Live plugin (PRD B6, D-07): it turns the pure plan into
// CodeMirror decorations, so the source stays the document while the drawn
// text carries the styles and hides the markup. Recomputing is whole-document
// per edit, which is why Live is off above the byte limit.

import { syntaxTree } from "@codemirror/language";
import type { EditorState, Extension, Range } from "@codemirror/state";
import { Decoration, EditorView, WidgetType, type DecorationSet, type PluginValue, type ViewUpdate } from "@codemirror/view";
import { ViewPlugin } from "@codemirror/view";
import { markdownLivePlan, MARKDOWN_LIVE_BYTE_LIMIT } from "./markdownLive";

class BulletWidget extends WidgetType {
  toDOM(): HTMLElement {
    const span = document.createElement("span");
    span.className = "cm-md-bullet";
    span.textContent = "•";
    return span;
  }
  eq(): boolean {
    return true;
  }
  ignoreEvent(): boolean {
    return false;
  }
}

class CheckboxWidget extends WidgetType {
  constructor(
    readonly checked: boolean,
    readonly from: number,
    readonly to: number,
  ) {
    super();
  }
  eq(other: CheckboxWidget): boolean {
    return other.checked === this.checked && other.from === this.from;
  }
  toDOM(view: EditorView): HTMLElement {
    const input = document.createElement("input");
    input.type = "checkbox";
    input.checked = this.checked;
    input.className = "cm-md-checkbox";
    input.setAttribute("aria-label", this.checked ? "Completed task" : "Open task");
    input.addEventListener("mousedown", (event) => event.preventDefault());
    input.addEventListener("click", (event) => {
      event.preventDefault();
      view.dispatch({ changes: { from: this.from, to: this.to, insert: this.checked ? "[ ]" : "[x]" } });
    });
    return input;
  }
  ignoreEvent(): boolean {
    return false;
  }
}

function build(state: EditorState): DecorationSet {
  const doc = state.doc;
  if (doc.length > MARKDOWN_LIVE_BYTE_LIMIT) return Decoration.none;
  const plan = markdownLivePlan(syntaxTree(state), doc, state.selection.main);
  const ranges: Range<Decoration>[] = [];
  for (const range of plan) {
    switch (range.kind) {
      case "hide":
        ranges.push(Decoration.replace({}).range(range.from, range.to));
        break;
      case "bullet":
        ranges.push(Decoration.replace({ widget: new BulletWidget() }).range(range.from, range.to));
        break;
      case "checkbox":
        ranges.push(
          Decoration.replace({ widget: new CheckboxWidget(range.checked === true, range.from, range.to) }).range(
            range.from,
            range.to,
          ),
        );
        break;
      case "heading":
        ranges.push(Decoration.line({ class: `cm-md-heading-${range.level ?? 1}` }).range(range.from));
        break;
      case "codeBlock":
        ranges.push(Decoration.line({ class: "cm-md-code" }).range(range.from));
        break;
      case "fenceLine":
        ranges.push(Decoration.line({ class: "cm-md-hidden-line" }).range(range.from));
        break;
      case "quote":
        ranges.push(Decoration.line({ class: "cm-md-quote" }).range(range.from));
        break;
      case "link":
        ranges.push(Decoration.mark({ class: "cm-md-link" }).range(range.from, range.to));
        break;
      case "codeInline":
        ranges.push(Decoration.mark({ class: "cm-md-code-inline" }).range(range.from, range.to));
        break;
    }
  }
  return Decoration.set(ranges, true);
}

/**
 * Live decorations while the document shows. The plugin rebuilds when the
 * document, the selection or the parse changed, so a lazily loaded Markdown
 * language lights the view up as soon as its first tree exists.
 */
class LivePlugin implements PluginValue {
  decorations: DecorationSet;
  constructor(view: EditorView) {
    this.decorations = build(view.state);
  }
  update(update: ViewUpdate) {
    if (
      update.docChanged ||
      update.selectionSet ||
      update.viewportChanged ||
      syntaxTree(update.state) !== syntaxTree(update.startState)
    ) {
      this.decorations = build(update.state);
    }
  }
}

export function markdownLive(): Extension {
  return ViewPlugin.fromClass(LivePlugin, { decorations: (value) => value.decorations });
}
