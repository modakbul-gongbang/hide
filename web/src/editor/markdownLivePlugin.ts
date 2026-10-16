// The Markdown Live plugin (PRD B6, D-07): it turns the pure plan into
// CodeMirror decorations, so the source stays the document while the drawn
// text carries the styles and hides the markup. Recomputing is whole-document
// per edit, which is why Live is off above the byte limit.
//
// The decorations live in a state field, not a view plugin, because an HTML
// block is drawn as a block widget and CodeMirror takes block decorations
// only from state.

import { syntaxTree } from "@codemirror/language";
import { StateField, type EditorState, type Extension, type Range } from "@codemirror/state";
import { Decoration, EditorView, ViewPlugin, WidgetType, type DecorationSet } from "@codemirror/view";
import type { FileSource } from "../fileBytes";
import { translate } from "../i18n/client";
import { useShellStore } from "../store";
import { imageTarget, resolveInCheckout, sanitizeHtml, type ImageBase } from "./liveHtml";
import { LiveImages, readImageFile } from "./liveImages";
import { markdownLivePlan, MARKDOWN_LIVE_BYTE_LIMIT } from "./markdownLive";

/**
 * What a Live text can reach outside itself: where its relative paths start
 * (null for a text that is not a file, such as an issue body) and what a
 * modifier-click on a link does.
 */
export type LiveFiles = {
  base: () => (ImageBase & { source: FileSource }) | null;
  openUrl: (url: string) => void;
  openPath: (path: string) => void;
};

const OUTSIDE: LiveFiles = {
  base: () => null,
  openUrl: (url) => {
    window.open(url, "_blank", "noopener,noreferrer");
  },
  openPath: () => {},
};

/** What a widget draws with: the view's image cache and the files it may name. */
type Drawing = { images: LiveImages; files: LiveFiles };

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
    input.setAttribute("aria-label", translate(this.checked ? "documents.completedTask" : "documents.openTask"));
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

/** One image: shown once its address is decided and its bytes are in, and as its alt text in the smallest muted form when it is refused or fails. */
function drawImage(img: HTMLImageElement, src: string, drawing: Drawing, view: EditorView): void {
  const base = drawing.files.base();
  const target = imageTarget(src, base);
  const alt = img.getAttribute("alt") ?? "";
  const name = src.split(/[?#]/)[0]?.split("/").pop() ?? "";
  const fallback = document.createElement("span");
  fallback.className = "cm-md-alt";
  fallback.dataset.state = "loading";
  fallback.textContent = alt || name;
  const fail = () => {
    fallback.dataset.state = "failed";
    view.requestMeasure();
  };
  img.replaceWith(fallback);
  if (target.kind === "refused") {
    fail();
    return;
  }
  img.addEventListener(
    "load",
    () => {
      fallback.replaceWith(img);
      view.requestMeasure();
    },
    { once: true },
  );
  img.addEventListener("error", fail, { once: true });
  img.referrerPolicy = "no-referrer";
  img.decoding = "async";
  void drawing.images.load(target, base?.source ?? null).then((url) => {
    if (url === null) fail();
    else img.src = url;
  });
}

/** A link inside drawn HTML: a plain click only places the caret, a modifier-click opens it. */
function openLink(anchor: HTMLAnchorElement, files: LiveFiles): void {
  const href = anchor.getAttribute("href") ?? "";
  if (href === "" || href.startsWith("#")) return;
  if (/^(?:https?|mailto):/i.test(href)) {
    files.openUrl(href);
    return;
  }
  const base = files.base();
  const path = base ? resolveInCheckout(href, base) : null;
  if (path === null) {
    useShellStore.getState().noteDiagnostic(`markdown live: link ${href} is not inside the checkout`);
    return;
  }
  files.openPath(path);
}

/** Sanitized HTML, drawn as a block (a whole HTML block) or inline (one `<img>` tag). */
class HtmlWidget extends WidgetType {
  constructor(
    readonly html: string,
    readonly block: boolean,
    readonly key: string,
    private readonly drawing: Drawing,
  ) {
    super();
  }
  eq(other: HtmlWidget): boolean {
    return other.html === this.html && other.block === this.block && other.key === this.key;
  }
  toDOM(view: EditorView): HTMLElement {
    const root = document.createElement(this.block ? "div" : "span");
    root.className = this.block ? "cm-md-html cm-md-html-block" : "cm-md-html";
    const clean = sanitizeHtml(this.html);
    if (!clean) {
      root.textContent = this.html;
      return root;
    }
    root.append(clean.fragment);
    for (const { element, src } of clean.images) drawImage(element, src, this.drawing, view);
    const anchorOf = (event: Event) => (event.target instanceof Element ? event.target.closest("a") : null);
    // The modifier-click opens the link before the editor sees the press, so
    // the caret stays and the source does not open; every other press is the
    // editor's, which places the caret and shows the source.
    root.addEventListener("mousedown", (event: MouseEvent) => {
      const anchor = anchorOf(event);
      if (!anchor || !(event.metaKey || event.ctrlKey)) return;
      event.preventDefault();
      event.stopPropagation();
      openLink(anchor, this.drawing.files);
    });
    // A link never navigates the page itself.
    root.addEventListener("click", (event) => {
      if (anchorOf(event)) event.preventDefault();
    });
    return root;
  }
  ignoreEvent(): boolean {
    return false;
  }
}

/** A Markdown image `![alt](src "title")`. */
class ImageWidget extends WidgetType {
  constructor(
    readonly src: string,
    readonly alt: string,
    readonly title: string | undefined,
    readonly key: string,
    private readonly drawing: Drawing,
  ) {
    super();
  }
  eq(other: ImageWidget): boolean {
    return other.src === this.src && other.alt === this.alt && other.title === this.title && other.key === this.key;
  }
  toDOM(view: EditorView): HTMLElement {
    const root = document.createElement("span");
    root.className = "cm-md-html";
    const img = document.createElement("img");
    img.alt = this.alt;
    if (this.title) img.title = this.title;
    root.append(img);
    drawImage(img, this.src, this.drawing, view);
    return root;
  }
  ignoreEvent(): boolean {
    return false;
  }
}

/** No selection anywhere in the document, so no mark is revealed. */
const NOWHERE = { from: -1, to: -1 };

function build(state: EditorState, reveal: boolean, drawing: Drawing): DecorationSet {
  const doc = state.doc;
  if (doc.length > MARKDOWN_LIVE_BYTE_LIMIT) return Decoration.none;
  const plan = markdownLivePlan(syntaxTree(state), doc, reveal ? state.selection.main : NOWHERE);
  const ranges: Range<Decoration>[] = [];
  const base = drawing.files.base();
  // The widgets of a different document or checkout are not the same drawing.
  const key = base ? `${base.docPath}|${base.root}|${base.source?.device ?? ""}` : "";
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
      case "html":
        ranges.push(
          Decoration.replace({ widget: new HtmlWidget(range.html ?? "", range.block === true, key, drawing), block: range.block === true }).range(
            range.from,
            range.to,
          ),
        );
        break;
      case "image":
        ranges.push(
          Decoration.replace({ widget: new ImageWidget(range.src ?? "", range.alt ?? "", range.title, key, drawing) }).range(range.from, range.to),
        );
        break;
    }
  }
  return Decoration.set(ranges, true);
}

/**
 * `reveal` shows the marks of the lines the selection touches, as an editor
 * does; a read-only text (an issue's body) has no caret to edit at, so it
 * never reveals them. `files` says where a document's relative images and
 * links start; a text with none loads web images only.
 *
 * The field rebuilds when the document, the selection or the parse changed, so
 * a lazily loaded Markdown language lights the view up as soon as its first
 * tree exists. A widget whose source and base are unchanged keeps its DOM, so
 * an edit outside an HTML block neither redraws it nor reads its images again.
 */
export function markdownLive({ reveal = true, files = OUTSIDE }: { reveal?: boolean; files?: LiveFiles } = {}): Extension {
  const drawing: Drawing = { images: new LiveImages(readImageFile, (message) => useShellStore.getState().noteDiagnostic(message)), files };
  const field = StateField.define<DecorationSet>({
    create: (state) => build(state, reveal, drawing),
    update(value, tr) {
      if (tr.docChanged || tr.selection || syntaxTree(tr.state) !== syntaxTree(tr.startState)) return build(tr.state, reveal, drawing);
      return value;
    },
    provide: (f) => EditorView.decorations.from(f),
  });
  // The image cache lives as long as the view that draws with it.
  const owner = ViewPlugin.define(() => ({ destroy: () => drawing.images.dispose() }));
  return [field, owner];
}
