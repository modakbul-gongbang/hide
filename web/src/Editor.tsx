import { useEffect, useRef } from "react";
import type { Actions } from "./actions";
import { allBuffers, bufferDecision, bufferFor, deleteBuffer, putBuffer } from "./buffers";
import { CodeMirrorEditor } from "./editor/CodeMirrorEditor";
import { clearDraft, noteDraft } from "./editor/draft";
import { activeEditorTab, checkoutById, editorFor, type EditorDocumentSnapshot, type EditorTabSnapshot } from "./snapshot";
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

/** How long editing must be idle before the shell saves (PRD S3 D-10). */
export const AUTOSAVE_IDLE_MS = 600;

export function EditorSurface({ actions }: { actions: Actions }) {
  const editor = useShellStore((s) => s.editor);
  const rest = useShellStore((s) => s.rest);
  const scale = useShellStore((s) => s.rest?.ui_state?.editor_text_scale);
  const findRequest = useUiStore((s) => s.editorFindRequest);
  const showing = editorFor(editor);
  const tab = activeEditorTab(editor);
  if (!showing || !tab) return null;
  // A buffer's identity is its checkout root plus the real path (D-14).
  const root = checkoutById(rest, tab.checkout_id)?.path ?? "";
  return (
    <EditorTabView
      key={tab.id}
      tab={tab}
      root={root}
      document={showing.document}
      scale={typeof scale === "number" ? scale : DEFAULT_SCALE}
      findRequest={findRequest}
      actions={actions}
    />
  );
}

function EditorTabView({
  tab,
  root,
  document,
  scale,
  findRequest,
  actions,
}: {
  tab: EditorTabSnapshot;
  root: string;
  document: EditorDocumentSnapshot | null;
  scale: number;
  findRequest: number;
  actions: Actions;
}) {
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-background" data-editor={tab.id} data-editor-kind={tab.kind}>
      <EditorHeader tab={tab} document={document} actions={actions} />
      <EditorBody tab={tab} root={root} document={document} scale={scale} findRequest={findRequest} actions={actions} />
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
  root,
  document,
  scale,
  findRequest,
  actions,
}: {
  tab: EditorTabSnapshot;
  root: string;
  document: EditorDocumentSnapshot | null;
  scale: number;
  findRequest: number;
  actions: Actions;
}) {
  const editable = document?.document_kind === "text" || document?.document_kind === "markdown";
  const latest = useRef(document);
  latest.current = document;
  const checked = useRef(false);
  const autosave = useRef<number | undefined>(undefined);

  const autosaveDue = () => {
    const current = latest.current;
    return (
      !!current &&
      (current.document_kind === "text" || current.document_kind === "markdown") &&
      !current.readonly_reason &&
      !current.conflict &&
      current.dirty
    );
  };

  /** Saves the showing document once editing has been idle for a moment. */
  const scheduleAutosave = () => {
    window.clearTimeout(autosave.current);
    autosave.current = window.setTimeout(() => {
      if (!autosaveDue()) return;
      useShellStore.getState().noteSaving(tab.id, true);
      actions.saveFile();
    }, AUTOSAVE_IDLE_MS);
  };

  // Leaving the tab ends its idle window: the timer goes, and a dirty draft
  // that the operator left behind is saved rather than waiting for a return
  // that may never come (D-10). A tab the operator closed is already gone from
  // the core's tab list, so its close-save is the one that carries it.
  useEffect(
    () => () => {
      window.clearTimeout(autosave.current);
      const current = latest.current;
      if (!current?.dirty || current.conflict) return;
      const stillOpen = useShellStore.getState().editor?.tabs.some((row) => row.id === tab.id);
      if (stillOpen) actions.saveFile(tab.id);
    },
    [tab.id, actions],
  );

  // A conflict pauses autosave until the operator chooses; the choice (or the
  // next edit) resumes it, because the conflict clears and the document is
  // still dirty (D-10, B5).
  useEffect(() => {
    if (document?.conflict) {
      window.clearTimeout(autosave.current);
      useShellStore.getState().noteSaving(tab.id, false);
      return;
    }
    if (document?.dirty) scheduleAutosave();
    // `scheduleAutosave` reads the newest document through `latest`.
  }, [document?.conflict, document?.dirty, tab.id]);

  // The save landed when the core reports the document clean.
  useEffect(() => {
    if (document?.dirty === false) useShellStore.getState().noteSaving(tab.id, false);
  }, [document?.dirty, tab.id]);

  // A refused save never reaches the clean state, so the core's failure is
  // what takes the saving mark off the tab; the document stays dirty and the
  // reason is in the diagnostic log (B5).
  const failureAt = useShellStore((s) => s.rest?.status?.last_error?.occurred_at ?? null);
  useEffect(() => {
    if (failureAt !== null) useShellStore.getState().noteSaving(tab.id, false);
  }, [failureAt, tab.id]);

  // A reconnect may have left an unsaved buffer in IndexedDB (B8): the buffer
  // is the newest edit, so it is restored over a clean core document and
  // dropped when the core already holds the same contents.
  useEffect(() => {
    checked.current = false;
    let live = true;
    void allBuffers().then((buffers) => {
      if (!live) return;
      const buffer = bufferFor(buffers, root, tab.path);
      checked.current = true;
      if (!buffer) return;
      const current = latest.current;
      if (!current) return;
      if (bufferDecision(buffer, current) === "restore") {
        noteDraft(tab.id, buffer.contents);
        actions.updateDraft(buffer.contents);
      }
      else void deleteBuffer(root, tab.path);
    });
    return () => {
      live = false;
    };
  }, [tab.id, root, tab.path, actions]);

  // A document the core reports clean has nothing unsaved, so its buffer goes.
  useEffect(() => {
    if (!checked.current || document?.dirty) return;
    if (document?.document_kind !== "text" && document?.document_kind !== "markdown") return;
    void deleteBuffer(root, tab.path);
  }, [document?.dirty, document?.document_kind, root, tab.path]);

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
      {document.conflict ? <ConflictBar tabId={tab.id} root={root} path={tab.path} actions={actions} /> : null}
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
          // A buffer that cannot be stored keeps the edit alive and says so on
          // the tab; the session continues either way (D-14).
          void putBuffer(root, tab.path, contents).then((stored) =>
            useShellStore.getState().noteBufferWarning(tab.id, !stored),
          );
          actions.updateDraft(contents);
          scheduleAutosave();
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
          {external.reason === "not_openable"
            ? "Hide never opens a file that could run or install itself"
            : `The default app could not open it: ${external.reason ?? "failed"}`}
        </span>
      ) : null}
    </div>
  );
}

function ConflictBar({ tabId, root, path, actions }: { tabId: string; root: string; path: string; actions: Actions }) {
  return (
    <div className="flex items-center gap-sm border-b border-divider px-md py-xs text-caption text-warning" data-editor-conflict="true">
      <span className="flex-1">This file changed on disk. Your draft is preserved.</span>
      <button
        type="button"
        className="text-secondary hover:text-primary"
        data-conflict-action="reload"
        onClick={() => {
          clearDraft(tabId);
          void deleteBuffer(root, path);
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
