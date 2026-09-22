import type { Actions } from "./actions";
import { activeEditorTab, editorFor, type EditorDocumentSnapshot, type EditorTabSnapshot } from "./snapshot";
import { useShellStore } from "./store";

// The document surface (PRD B3, B4, B6, B7). The core owns the open tabs and
// the document; this draws the one that is showing and dispatches the events
// that change it. A document the core classified as binary takes no edits, and
// a readonly one says why in one line. The editor body itself is CodeMirror
// (slice 4); this shell is what every viewer kind hangs off.

export function EditorSurface({ actions }: { actions: Actions }) {
  const editor = useShellStore((s) => s.editor);
  const showing = editorFor(editor);
  const tab = activeEditorTab(editor);
  if (!showing || !tab) return null;
  return <EditorTabView tab={tab} document={showing.document} actions={actions} />;
}

function EditorTabView({
  tab,
  document,
  actions,
}: {
  tab: EditorTabSnapshot;
  document: EditorDocumentSnapshot | null;
  actions: Actions;
}) {
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-background" data-editor={tab.id} data-editor-kind={tab.kind}>
      <EditorHeader tab={tab} document={document} actions={actions} />
      <EditorBody document={document} />
    </div>
  );
}

function EditorHeader({
  tab,
  document,
  actions,
}: {
  tab: EditorTabSnapshot;
  document: EditorDocumentSnapshot | null;
  actions: Actions;
}) {
  return (
    <div className="flex shrink-0 items-center gap-sm border-b border-divider px-md py-xs text-caption text-secondary">
      <span className="min-w-0 flex-1 truncate" title={tab.path} data-editor-path="true">
        {tab.path}
      </span>
      {document?.language ? <span className="text-muted">{document.language}</span> : null}
      {tab.preview ? (
        <button
          type="button"
          className="text-muted hover:text-primary"
          title="Keep open (⌘⇧K)"
          data-editor-preview="true"
          onClick={() => actions.keepOpenFile()}
        >
          preview
        </button>
      ) : null}
    </div>
  );
}

function EditorBody({ document }: { document: EditorDocumentSnapshot | null }) {
  if (!document) {
    return <Notice text="Loading…" data-editor="loading" />;
  }
  if (document.document_kind === "binary") {
    return <Notice text="This file is binary and cannot be edited here." data-editor="binary" />;
  }
  if (document.document_kind === "image") {
    return <Notice text="Image viewer arrives with the file bytes frame." data-editor="image" />;
  }
  if (document.document_kind === "pdf") {
    return <Notice text="PDF viewer arrives with the file bytes frame." data-editor="pdf" />;
  }
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {document.readonly_reason ? (
        <div className="border-b border-divider px-md py-xs text-caption text-muted" data-editor-readonly="true">
          {document.readonly_reason}
        </div>
      ) : null}
      <pre
        className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap px-md py-sm font-mono text-body text-primary"
        data-editor-text="true"
      >
        {document.contents_utf8 ?? ""}
      </pre>
    </div>
  );
}

function Notice({ text, ...data }: { text: string } & Record<`data-${string}`, string>) {
  return (
    <div className="flex flex-1 items-center justify-center px-md text-center text-caption text-muted" {...data}>
      {text}
    </div>
  );
}
