// The CodeMirror 6 host (PRD B4, D-07). The core owns the document; this view
// renders it, reports an edit as a `file_draft`, and replaces its own text
// only when the core's contents are the ones this view last sent (an echo) or
// a real external change. A stale echo of an earlier keystroke is dropped
// rather than applied, which is the rule the Swift editor uses.
//
// Markdown Live draws a document's leading frontmatter in its own pane above
// the body (D-11), so that document is held by two views that partition it:
// the pane scrolls inside its fixed height while the body keeps the rest. The
// split and the merge are not user events and never enter either history, so
// undo stays inside the half it happened in.

import { Compartment, EditorState, type Extension, type StateEffect } from "@codemirror/state";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { bracketMatching, indentOnInput } from "@codemirror/language";
import { openSearchPanel, search, searchKeymap } from "@codemirror/search";
import {
  EditorView,
  drawSelection,
  dropCursor,
  highlightActiveLine,
  highlightActiveLineGutter,
  highlightSpecialChars,
  keymap,
  lineNumbers,
  type ViewUpdate,
} from "@codemirror/view";
import { useEffect, useRef } from "react";
import type { EditorDocumentSnapshot } from "../snapshot";
import { splitFrontmatter } from "./frontmatter";
import { languageLoader } from "./languages";
import { markdownLive } from "./markdownLivePlugin";
import { baseTheme, liveTheme, scaleTheme } from "./theme";

const languageConf = new Compartment();
const readonlyConf = new Compartment();
const wrapConf = new Compartment();
const scaleConf = new Compartment();
const liveConf = new Compartment();

/** How many lines of frontmatter the pane shows before it scrolls (D-11). */
const FRONTMATTER_LINES = 8;
/** The editor's line height, from `theme.ts`. */
const LINE_HEIGHT = 1.5;

function readonlyExtensions(readonly: boolean): Extension {
  return readonly ? [EditorState.readOnly.of(true), EditorView.editable.of(false)] : [];
}

export function CodeMirrorEditor({
  tabId,
  document,
  scale,
  wrap,
  live,
  findRequest,
  held = false,
  onDraft,
}: {
  tabId: string;
  document: EditorDocumentSnapshot;
  scale: number;
  wrap: boolean;
  live: boolean;
  findRequest: number;
  /** Read-only while unsaved drafts cannot be stored (S5.5 B44). */
  held?: boolean;
  onDraft: (contents: string) => void;
}) {
  const bodyHost = useRef<HTMLDivElement>(null);
  const frontHost = useRef<HTMLDivElement>(null);
  const bodyView = useRef<EditorView | null>(null);
  const frontView = useRef<EditorView | null>(null);
  // The last contents this view sent the core; the core's echo of anything
  // else is a stale keystroke and is not written back over the buffer.
  const pending = useRef<string | null>(null);
  // The body's caret across a pane toggle, so showing or hiding the
  // frontmatter does not move the operator's place in the document.
  const caret = useRef<{ tabId: string; anchor: number; head: number } | null>(null);
  const onDraftRef = useRef(onDraft);
  onDraftRef.current = onDraft;

  const contents = document.contents_utf8 ?? "";
  const contentsRef = useRef(contents);
  contentsRef.current = contents;
  const readonly = held || document.readonly_reason !== null || document.document_kind !== "text" && document.document_kind !== "markdown";
  // The pane belongs to Live mode's markdown only; source mode and every
  // other kind keep one buffer (D-11).
  const split = document.document_kind === "markdown" && live && splitFrontmatter(contents) !== null;

  const combined = () =>
    (frontView.current?.state.doc.toString() ?? "") + (bodyView.current?.state.doc.toString() ?? "");

  // One view per tab, and one pair while the frontmatter pane shows: switching
  // tabs remounts (the parent keys by tab id), so the history and the
  // selection belong to the document, not the shell.
  useEffect(() => {
    if (!bodyHost.current) return undefined;
    const parts = split ? splitFrontmatter(contentsRef.current) : null;
    // Both halves report one draft: the changed half comes from this update,
    // the untouched one from its own view, and the block stays first (D-11).
    const listeners = (half: "front" | "body") =>
      EditorView.updateListener.of((update: ViewUpdate) => {
        if (!update.docChanged) return;
        // An undo or a redo is the operator's edit too, and the core must hear
        // it or the next save writes the text they just undid.
        if (
          !update.transactions.some(
            (tr) =>
              tr.isUserEvent("input") ||
              tr.isUserEvent("delete") ||
              tr.isUserEvent("move") ||
              tr.isUserEvent("undo") ||
              tr.isUserEvent("redo"),
          )
        )
          return;
        const front = half === "front" ? update.state.doc.toString() : (frontView.current?.state.doc.toString() ?? "");
        const body = half === "body" ? update.state.doc.toString() : (bodyView.current?.state.doc.toString() ?? "");
        pending.current = front + body;
        onDraftRef.current(front + body);
      });
    const shared = [
      highlightSpecialChars(),
      history(),
      drawSelection(),
      dropCursor(),
      indentOnInput(),
      bracketMatching(),
      highlightActiveLine(),
      baseTheme,
      readonlyConf.of(readonlyExtensions(readonly)),
      wrapConf.of(wrap ? EditorView.lineWrapping : []),
      scaleConf.of(scaleTheme(scale)),
    ];
    const bodyEditor = new EditorView({
      state: EditorState.create({
        doc: parts ? parts.body : contentsRef.current,
        extensions: [
          ...shared,
          lineNumbers(),
          highlightActiveLineGutter(),
          search({ top: true }),
          keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap, indentWithTab]),
          languageConf.of([]),
          liveConf.of(live ? [markdownLive(), liveTheme] : []),
          listeners("body"),
        ],
      }),
      parent: bodyHost.current,
    });
    bodyView.current = bodyEditor;
    let frontEditor: EditorView | null = null;
    if (parts && frontHost.current) {
      frontEditor = new EditorView({
        state: EditorState.create({
          doc: parts.front,
          extensions: [
            ...shared,
            keymap.of([...defaultKeymap, ...historyKeymap, indentWithTab]),
            listeners("front"),
          ],
        }),
        parent: frontHost.current,
      });
      frontView.current = frontEditor;
    }
    const remembered = caret.current;
    if (remembered && remembered.tabId === tabId) {
      const limit = bodyEditor.state.doc.length;
      bodyEditor.dispatch({ selection: { anchor: Math.min(remembered.anchor, limit), head: Math.min(remembered.head, limit) } });
    }
    return () => {
      const selection = bodyEditor.state.selection.main;
      caret.current = { tabId, anchor: selection.anchor, head: selection.head };
      bodyView.current = null;
      frontView.current = null;
      bodyEditor.destroy();
      frontEditor?.destroy();
    };
    // The views are created once per tab and pane state; the effects below
    // reconfigure them.
  }, [tabId, split]);

  // The core's contents, when they are not what these views hold: an echo of
  // the last draft is a no-op, a stale echo is dropped, and anything else is
  // a real change (a reload) that replaces both halves.
  useEffect(() => {
    const editor = bodyView.current;
    if (!editor) return;
    const incoming = document.contents_utf8 ?? "";
    if (incoming === combined()) {
      pending.current = null;
      return;
    }
    if (pending.current !== null && incoming !== pending.current) return;
    pending.current = null;
    const parts = split ? splitFrontmatter(incoming) : null;
    const front = frontView.current;
    if (parts && front) {
      front.dispatch({ changes: { from: 0, to: front.state.doc.length, insert: parts.front } });
      editor.dispatch({ changes: { from: 0, to: editor.state.doc.length, insert: parts.body } });
    } else {
      editor.dispatch({ changes: { from: 0, to: editor.state.doc.length, insert: incoming } });
    }
  }, [document.contents_utf8, split]);

  // The language pack loads only for a document that asks for one; the
  // dispatch is ignored when the tab was closed while the import was in flight.
  useEffect(() => {
    const editor = bodyView.current;
    if (!editor) return;
    let cancelled = false;
    const loader = languageLoader(document.language, document.path);
    const apply = (extension: Extension | null) => {
      if (cancelled || bodyView.current !== editor) return;
      editor.dispatch({ effects: languageConf.reconfigure(extension ?? []) });
    };
    if (!loader) apply(null);
    else void loader().then(apply).catch(() => apply(null));
    return () => {
      cancelled = true;
    };
  }, [document.language, document.path, split]);

  // A pane setting reaches both halves, because the frontmatter is the same
  // document and the operator's scale, wrap and readonly choice is one choice.
  const both = (effect: StateEffect<unknown>) => {
    bodyView.current?.dispatch({ effects: effect });
    frontView.current?.dispatch({ effects: effect });
  };

  useEffect(() => {
    both(readonlyConf.reconfigure(readonlyExtensions(readonly)));
  }, [readonly]);

  useEffect(() => {
    both(wrapConf.reconfigure(wrap ? EditorView.lineWrapping : []));
  }, [wrap]);

  useEffect(() => {
    both(scaleConf.reconfigure(scaleTheme(scale)));
  }, [scale]);

  useEffect(() => {
    bodyView.current?.dispatch({ effects: liveConf.reconfigure(live ? [markdownLive(), liveTheme] : []) });
  }, [live]);

  // A request is an increment: the counter survives a tab switch, so only a
  // new ⌘F opens the panel, and a freshly mounted document does not inherit
  // the last one's find input.
  const seenFind = useRef(findRequest);
  useEffect(() => {
    if (findRequest <= seenFind.current) return;
    seenFind.current = findRequest;
    if (bodyView.current) openSearchPanel(bodyView.current);
  }, [findRequest]);

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden" data-editor-codemirror="true">
      {split ? (
        <div
          className="shrink-0 overflow-hidden border-b border-divider bg-elevated"
          data-editor-frontmatter="true"
          style={{ height: `calc(var(--text-editor-document) * ${scale} * ${FRONTMATTER_LINES * LINE_HEIGHT})` }}
        >
          <div ref={frontHost} className="h-full" data-editor-frontmatter-editor="true" />
        </div>
      ) : null}
      <div ref={bodyHost} className="min-h-0 flex-1 overflow-hidden" data-editor-body="true" />
    </div>
  );
}
