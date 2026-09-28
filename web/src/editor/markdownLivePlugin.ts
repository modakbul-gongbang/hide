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
      view.dispatch({
        changes: { from: this.from, to: this.to, insert: this.checked ? "[ ]" : "[x]" },
        // The toggle is an operator edit like a keystroke, so the draft event
        // reaches the core and the tab turns dirty.
        userEvent: "input",
      });
    });
    return input;
  }
  ignoreEvent(): boolean {
    return false;
  }
}

/** No selection anywhere in the document, so no mark is revealed. */
const NOWHERE = { from: -1, to: -1 };

function build(state: EditorState, reveal: boolean): DecorationSet {
  const doc = state.doc;
  if (doc.length > MARKDOWN_LIVE_BYTE_LIMIT) return Decoration.none;
  const plan = markdownLivePlan(syntaxTree(state), doc, reveal ? state.selection.main : NOWHERE);
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
  constructor(
    view: EditorView,
    private readonly reveal: boolean,
  ) {
    this.decorations = build(view.state, reveal);
  }
  update(update: ViewUpdate) {
    if (
      update.docChanged ||
      update.selectionSet ||
      update.viewportChanged ||
      syntaxTree(update.state) !== syntaxTree(update.startState)
    ) {
      this.decorations = build(update.state, this.reveal);
    }
  }
}

/**
 * `reveal` shows the marks of the lines the selection touches, as an editor
 * does; a read-only text (an issue's body) has no caret to edit at, so it
 * never reveals them.
 */
export function markdownLive({ reveal = true }: { reveal?: boolean } = {}): Extension {
  return ViewPlugin.define((view) => new LivePlugin(view, reveal), { decorations: (value) => value.decorations });
}
