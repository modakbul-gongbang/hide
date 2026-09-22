// The CodeMirror 6 host (PRD B4, D-07). The core owns the document; this view
// renders it, reports an edit as a `file_draft`, and replaces its own text
// only when the core's contents are the ones this view last sent (an echo) or
// a real external change. A stale echo of an earlier keystroke is dropped
// rather than applied, which is the rule the Swift editor uses.

import { Compartment, EditorState, type Extension } from "@codemirror/state";
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
} from "@codemirror/view";
import { useEffect, useRef } from "react";
import type { EditorDocumentSnapshot } from "../snapshot";
import { languageLoader } from "./languages";
import { baseTheme, scaleTheme } from "./theme";

const languageConf = new Compartment();
const readonlyConf = new Compartment();
const wrapConf = new Compartment();
const scaleConf = new Compartment();

export function CodeMirrorEditor({
  tabId,
  document,
  scale,
  wrap,
  findRequest,
  onDraft,
}: {
  tabId: string;
  document: EditorDocumentSnapshot;
  scale: number;
  wrap: boolean;
  findRequest: number;
  onDraft: (contents: string) => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  // The last contents this view sent the core; the core's echo of anything
  // else is a stale keystroke and is not written back over the buffer.
  const pending = useRef<string | null>(null);
  const onDraftRef = useRef(onDraft);
  onDraftRef.current = onDraft;

  const readonly = document.readonly_reason !== null || document.document_kind !== "text" && document.document_kind !== "markdown";

  // One view per tab: switching tabs remounts (the parent keys by tab id), so
  // the history and the selection belong to the document, not the shell.
  useEffect(() => {
    if (!host.current) return undefined;
    const state = EditorState.create({
      doc: document.contents_utf8 ?? "",
      extensions: [
        lineNumbers(),
        highlightActiveLineGutter(),
        highlightSpecialChars(),
        history(),
        drawSelection(),
        dropCursor(),
        indentOnInput(),
        bracketMatching(),
        highlightActiveLine(),
        search({ top: true }),
        keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap, indentWithTab]),
        baseTheme,
        languageConf.of([]),
        readonlyConf.of(readonly ? [EditorState.readOnly.of(true), EditorView.editable.of(false)] : []),
        wrapConf.of(wrap ? EditorView.lineWrapping : []),
        scaleConf.of(scaleTheme(scale)),
        EditorView.updateListener.of((update) => {
          if (!update.docChanged) return;
          if (!update.transactions.some((tr) => tr.isUserEvent("input") || tr.isUserEvent("delete") || tr.isUserEvent("move"))) return;
          const contents = update.state.doc.toString();
          pending.current = contents;
          onDraftRef.current(contents);
        }),
      ],
    });
    const editor = new EditorView({ state, parent: host.current });
    view.current = editor;
    return () => {
      view.current = null;
      editor.destroy();
    };
    // The view is created once per tab; the effects below reconfigure it.
  }, [tabId]);

  // The core's contents, when they are not what this view holds: an echo of
  // the last draft is a no-op, a stale echo is dropped, and anything else is
  // a real change (a reload) that replaces the buffer.
  useEffect(() => {
    const editor = view.current;
    if (!editor) return;
    const incoming = document.contents_utf8 ?? "";
    const current = editor.state.doc.toString();
    if (incoming === current) {
      pending.current = null;
      return;
    }
    if (pending.current !== null && incoming !== pending.current) return;
    pending.current = null;
    editor.dispatch({ changes: { from: 0, to: editor.state.doc.length, insert: incoming } });
  }, [document.contents_utf8]);

  // The language pack loads only for a document that asks for one; the
  // dispatch is ignored when the tab was closed while the import was in flight.
  useEffect(() => {
    const editor = view.current;
    if (!editor) return;
    let cancelled = false;
    const loader = languageLoader(document.language, document.path);
    const apply = (extension: Extension | null) => {
      if (cancelled || view.current !== editor) return;
      editor.dispatch({ effects: languageConf.reconfigure(extension ?? []) });
    };
    if (!loader) apply(null);
    else void loader().then(apply).catch(() => apply(null));
    return () => {
      cancelled = true;
    };
  }, [document.language, document.path]);

  useEffect(() => {
    view.current?.dispatch({ effects: readonlyConf.reconfigure(readonly ? [EditorState.readOnly.of(true), EditorView.editable.of(false)] : []) });
  }, [readonly]);

  useEffect(() => {
    view.current?.dispatch({ effects: wrapConf.reconfigure(wrap ? EditorView.lineWrapping : []) });
  }, [wrap]);

  useEffect(() => {
    view.current?.dispatch({ effects: scaleConf.reconfigure(scaleTheme(scale)) });
  }, [scale]);

  useEffect(() => {
    if (findRequest > 0 && view.current) openSearchPanel(view.current);
  }, [findRequest]);

  return <div ref={host} className="min-h-0 flex-1 overflow-hidden" data-editor-codemirror="true" />;
}
