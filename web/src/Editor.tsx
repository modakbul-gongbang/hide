import { useEffect, useRef } from "react";
import type { Actions } from "./actions";
import { allBuffers, bufferDecision, deleteBuffer, putBuffer } from "./buffers";
import { CodeMirrorEditor } from "./editor/CodeMirrorEditor";
import { clearDraft, noteDraft } from "./editor/draft";
import { activeEditorTab, editorFor, type EditorDocumentSnapshot, type EditorTabSnapshot } from "./snapshot";
import { downloadFile, isLocalHost } from "./fileBytes";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { FileViewer } from "./viewers/FileViewer";

// The document surface (PRD B3-B7). The core owns the open tabs and the
// document; this draws the one that is showing and dispatches the events that
// change it. Text and Markdown edit in CodeMirror; a document the core
// classified as binary takes no edits, and a readonly one says why in one
// line. The image, PDF and video viewers arrive with the file-bytes frame.

const DEFAULT_SCALE = 1;

export function EditorSurface({ actions }: { actions: Actions }) {
  const editor = useShellStore((s) => s.editor);
  const scale = useShellStore((s) => s.rest?.ui_state?.editor_text_scale);
  const findRequest = useUiStore((s) => s.editorFindRequest);
  const showing = editorFor(editor);
  const tab = activeEditorTab(editor);
  if (!showing || !tab) return null;
  return (
    <EditorTabView
      tab={tab}
      document={showing.document}
      scale={typeof scale === "number" ? scale : DEFAULT_SCALE}
      findRequest={findRequest}
      actions={actions}
    />
  );
}

function EditorTabView({
  tab,
  document,
  scale,
  findRequest,
  actions,
}: {
  tab: EditorTabSnapshot;
  document: EditorDocumentSnapshot | null;
  scale: number;
  findRequest: number;
  actions: Actions;
}) {
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-background" data-editor={tab.id} data-editor-kind={tab.kind}>
      <EditorHeader tab={tab} document={document} actions={actions} />
      <EditorBody tab={tab} document={document} scale={scale} findRequest={findRequest} actions={actions} />
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
  const isMarkdown = document?.document_kind === "markdown";
  const editable = document?.document_kind === "text" || isMarkdown;
  return (
    <div className="flex shrink-0 items-center gap-sm border-b border-divider px-md py-xs text-caption text-secondary">
      <span className="min-w-0 flex-1 truncate" title={tab.path} data-editor-path="true">
        {tab.path}
      </span>
      {document?.dirty ? (
        <span className="text-warning" data-editor-dirty="true">
          Unsaved
        </span>
      ) : null}
      {isMarkdown && editable ? (
        <button
          type="button"
          className={tab.markdown_live ? "text-primary" : "text-muted hover:text-primary"}
          data-markdown-mode={tab.markdown_live ? "live" : "source"}
          title={tab.markdown_live ? "Edit with formatting shown in place" : "Edit Markdown source"}
          onClick={() => actions.setFileView(!tab.markdown_live, tab.wrap)}
        >
          {tab.markdown_live ? "Live" : "Source"}
        </button>
      ) : null}
      {editable ? (
        <button
          type="button"
          className={tab.wrap ? "text-primary" : "text-muted hover:text-primary"}
          data-editor-wrap={tab.wrap ? "true" : "false"}
          title="Wrap lines"
          onClick={() => actions.setFileView(tab.markdown_live, !tab.wrap)}
        >
          Wrap
        </button>
      ) : null}
      <button
        type="button"
        className="text-muted hover:text-primary"
        aria-label="Find in document"
        title="Find in document (⌘F)"
        disabled={!editable}
        onClick={() => actions.requestEditorFind()}
      >
        Find
      </button>
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

function EditorBody({
  tab,
  document,
  scale,
  findRequest,
  actions,
}: {
  tab: EditorTabSnapshot;
  document: EditorDocumentSnapshot | null;
  scale: number;
  findRequest: number;
  actions: Actions;
}) {
  const editable = document?.document_kind === "text" || document?.document_kind === "markdown";
  const latest = useRef(document);
  latest.current = document;
  const checked = useRef(false);

  // A reconnect may have left an unsaved buffer in IndexedDB (B8): the buffer
  // is the newest edit, so it is restored over a clean core document and
  // dropped when the core already holds the same contents.
  useEffect(() => {
    checked.current = false;
    let live = true;
    void allBuffers().then((buffers) => {
      if (!live) return;
      const buffer = buffers.find((row) => row.path === tab.path);
      checked.current = true;
      if (!buffer) return;
      const current = latest.current;
      if (!current) return;
      if (bufferDecision(buffer, current) === "restore") actions.updateDraft(buffer.contents);
      else void deleteBuffer(tab.path);
    });
    return () => {
      live = false;
    };
  }, [tab.id, tab.path, actions]);

  // A document the core reports clean has nothing unsaved, so its buffer goes.
  useEffect(() => {
    if (!checked.current || document?.dirty) return;
    if (document?.document_kind !== "text" && document?.document_kind !== "markdown") return;
    void deleteBuffer(tab.path);
  }, [document?.dirty, document?.document_kind, tab.path]);

  if (!document) {
    return <Notice text="Loading…" state="loading" />;
  }
  if (!editable) {
    return <FileViewer document={document} />;
  }
  // A text document the core would not read (past the editable cap) offers the
  // host OS handler instead of an editor (D-12); a remote daemon downloads.
  if (document.contents_utf8 === null) {
    return <PreviewOnly document={document} actions={actions} />;
  }
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {document.readonly_reason ? (
        <div className="border-b border-divider px-md py-xs text-caption text-warning" data-editor-readonly="true">
          {document.readonly_reason}
        </div>
      ) : null}
      {document.conflict ? <ConflictBar tabId={tab.id} actions={actions} /> : null}
      <CodeMirrorEditor
        key={tab.id}
        tabId={tab.id}
        document={document}
        scale={scale}
        wrap={tab.wrap}
        live={document.document_kind === "markdown" && tab.markdown_live}
        findRequest={findRequest}
        onDraft={(contents) => {
          noteDraft(tab.id, contents);
          void putBuffer(tab.path, contents);
          actions.updateDraft(contents);
        }}
      />
    </div>
  );
}

function PreviewOnly({ document, actions }: { document: EditorDocumentSnapshot; actions: Actions }) {
  const external = useShellStore((s) => s.externalOpen);
  const local = isLocalHost();
  const failed = external?.ok === false && external.path === document.path;
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-sm px-md text-center text-caption text-muted" data-editor-preview-only="true">
      <span>{document.readonly_reason ?? "This file is too large to edit here."}</span>
      <button
        type="button"
        className="rounded-sm bg-elevated px-md py-xs text-primary hover:bg-divider"
        data-editor-open-external="true"
        onClick={() => {
          if (local) actions.openExternal(document.path);
          else void downloadFile(document.path);
        }}
      >
        {local ? "Open in default app" : "Download"}
      </button>
      {failed ? (
        <span className="text-danger" data-editor-open-failed="true">
          {`The default app could not open it: ${external.reason ?? "failed"}`}
        </span>
      ) : null}
    </div>
  );
}

function ConflictBar({ tabId, actions }: { tabId: string; actions: Actions }) {
  return (
    <div className="flex items-center gap-sm border-b border-divider px-md py-xs text-caption text-warning" data-editor-conflict="true">
      <span className="flex-1">This file changed on disk. Your draft is preserved.</span>
      <button
        type="button"
        className="text-secondary hover:text-primary"
        data-conflict-action="reload"
        onClick={() => {
          clearDraft(tabId);
          actions.resolveConflict("reload");
        }}
      >
        Reload disk version
      </button>
      <button
        type="button"
        className="text-secondary hover:text-primary"
        data-conflict-action="keep_editing"
        onClick={() => actions.resolveConflict("keep_editing")}
      >
        Keep editing
      </button>
    </div>
  );
}

function Notice({ text, state }: { text: string; state: string }) {
  return (
    <div className="flex flex-1 items-center justify-center px-md text-center text-caption text-muted" data-editor-state={state}>
      {text}
    </div>
  );
}
