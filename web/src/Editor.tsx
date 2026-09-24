import { useEffect, useRef, useState } from "react";
import type { Actions } from "./actions";
import { allBuffers, bufferDecision, bufferFor, claimLegacyBuffer, deleteBuffer, draftStorageHold, flushBuffer, identity, queueBuffer, tabBufferKey, type BufferKey } from "./buffers";
import { CodeMirrorEditor } from "./editor/CodeMirrorEditor";
import { PatchView } from "./editor/PatchView";
import { clearDraft, latestDraft, noteDraft } from "./editor/draft";
import { activeEditorTab, changesFor, editorFor, type EditorDocumentSnapshot, type EditorTabSnapshot } from "./snapshot";
import { downloadFile } from "./fileBytes";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { FileViewer } from "./viewers/FileViewer";
import { useFileSource } from "./viewers/useFileBytes";

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
  const host = useShellStore((s) => s.daemon?.host_id ?? null);
  const scale = useShellStore((s) => s.rest?.ui_state?.editor_text_scale);
  const findRequest = useUiStore((s) => s.editorFindRequest);
  const showing = editorFor(editor);
  const tab = activeEditorTab(editor);
  if (!showing || !tab) return null;
  // A draft is filed under this daemon host, the tab's device, its checkout
  // root and the real path (D-14, S5.5 B9).
  const draftKey = tab.kind === "file" ? tabBufferKey(host, rest, tab) : null;
  return (
    <EditorTabView
      key={tab.id}
      tab={tab}
      draftKey={draftKey}
      document={showing.document}
      scale={typeof scale === "number" ? scale : DEFAULT_SCALE}
      findRequest={findRequest}
      actions={actions}
    />
  );
}

function EditorTabView({
  tab,
  draftKey,
  document,
  scale,
  findRequest,
  actions,
}: {
  tab: EditorTabSnapshot;
  draftKey: BufferKey | null;
  document: EditorDocumentSnapshot | null;
  scale: number;
  findRequest: number;
  actions: Actions;
}) {
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-background" data-editor={tab.id} data-editor-kind={tab.kind}>
      <EditorHeader tab={tab} document={document} actions={actions} />
      {tab.kind === "diff"
        ? <DiffBody tab={tab} scale={scale} />
        : <EditorBody tab={tab} draftKey={draftKey} document={document} scale={scale} findRequest={findRequest} actions={actions} />}
    </div>
  );
}

function DiffBody({ tab, scale }: { tab: EditorTabSnapshot; scale: number }) {
  const changes = useShellStore((s) => changesFor(s.changes, s.rest?.navigator?.changes_root_path ?? null));
  if (!changes) return <Notice text="Reading the diff…" state="diff-loading" />;
  if (changes.unavailable_reason) return <Notice text={`History is unavailable: ${changes.unavailable_reason}`} state="diff-unavailable" />;
  const committed = tab.diff_committed === true;
  const group = committed ? changes.committed : changes.entries;
  if (!group.some((entry) => entry.path === tab.path)) {
    return <Notice text="This file is no longer in the selected History group. Close this tab or choose another row." state="diff-unavailable" />;
  }
  const diff = changes.selected_path === tab.path && changes.selected_committed === committed && changes.diff?.path === tab.path
    ? changes.diff : null;
  if (!diff) return <Notice text="Reading the diff…" state="diff-loading" />;
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col" data-diff-path={tab.path} data-diff-group={committed ? "committed" : "working"}>
      {diff.notice ? <div className="border-b border-divider px-md py-xs text-caption text-warning" data-diff-notice="true">{diff.notice}</div> : null}
      {diff.text ? <PatchView text={diff.text} scale={scale} /> : <div className="min-h-0 flex-1" data-diff-empty="true" />}
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
  const isMarkdown = tab.kind === "file" && document?.document_kind === "markdown";
  const editable = document?.document_kind === "text" || isMarkdown;
  return (
    <div className="flex shrink-0 items-center gap-sm border-b border-divider px-md py-xs text-caption text-secondary">
      <span className="min-w-0 flex-1 truncate" title={tab.kind === "diff" ? `${tab.diff_committed ? "Committed on branch" : "Uncommitted"}: ${tab.path}` : tab.path} data-editor-path="true">
        {tab.kind === "diff" ? `${tab.diff_committed ? "Branch diff" : "Working diff"} · ` : ""}{tab.path}
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
      {tab.kind === "file" && editable ? (
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
      {tab.kind === "file" ? <button
        type="button"
        className="text-muted hover:text-primary"
        aria-label="Find in document"
        title="Find in document (⌘F)"
        disabled={!editable}
        onClick={() => actions.requestEditorFind()}
      >
        Find
      </button> : null}
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
  draftKey,
  document,
  scale,
  findRequest,
  actions,
}: {
  tab: EditorTabSnapshot;
  draftKey: BufferKey | null;
  document: EditorDocumentSnapshot | null;
  scale: number;
  findRequest: number;
  actions: Actions;
}) {
  const draftId = draftKey ? identity(draftKey) : null;
  const keyRef = useRef(draftKey);
  keyRef.current = draftKey;
  const editable = document?.document_kind === "text" || document?.document_kind === "markdown";
  const latest = useRef(document);
  latest.current = document;
  const checked = useRef(false);
  const autosave = useRef<number | undefined>(undefined);
  const connection = useShellStore((s) => s.connection);
  // A draft store that refused a write holds every other clean document
  // read-only until there is room again (B44); only open tabs count, so a
  // closed tab's old refusal does not hold the editor forever.
  const storageFull = useShellStore((s) => (s.editor?.tabs ?? []).some((row) => s.bufferWarnings.has(row.id)));
  const unstored = useShellStore((s) => s.bufferWarnings.has(tab.id));
  const hold = draftStorageHold({ storageFull, unstored, dirty: document?.dirty ?? false });
  // A saved document needs no stored draft, so its tab-only mark goes with
  // the save, and with it the hold on every other document.
  const clean = document ? !document.dirty : false;
  useEffect(() => {
    if (clean) useShellStore.getState().noteBufferWarning(tab.id, false);
  }, [clean, tab.id]);

  const autosaveDue = () => {
    const current = latest.current;
    return (
      !!current &&
      (current.document_kind === "text" || current.document_kind === "markdown") &&
      !current.readonly_reason &&
      !current.conflict &&
      // A save whose answer was lost blocks the next one until it is read
      // back; a running or waiting save takes the newest draft behind it.
      (!current.save || current.save.state === "saving" || current.save.state === "waiting") &&
      current.dirty
    );
  };

  /** Saves the showing document once editing has been idle for a moment. */
  const scheduleAutosave = () => {
    window.clearTimeout(autosave.current);
    autosave.current = window.setTimeout(() => {
      if (!autosaveDue()) return;
      if (useShellStore.getState().connection !== "live") return;
      useShellStore.getState().noteSaving(tab.id, actions.saveFile() === true);
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
    if (document?.conflict || connection !== "live") {
      window.clearTimeout(autosave.current);
      useShellStore.getState().noteSaving(tab.id, false);
      return;
    }
    if (document?.dirty) scheduleAutosave();
    // `scheduleAutosave` reads the newest document through `latest`.
  }, [document?.conflict, document?.dirty, tab.id, connection]);

  // The save landed when the core reports the document clean.
  useEffect(() => {
    if (document?.dirty === false) useShellStore.getState().noteSaving(tab.id, false);
  }, [document?.dirty, tab.id]);

  // A refused save never reaches the clean state, so the core's failure is
  // what takes the saving mark off the tab; the document stays dirty and the
  // reason is in the diagnostic log (B5).
  // Only a file error settles a save; a device or catalog error that lands
  // while a slow device save runs leaves its mark alone (S5.5 B45).
  const failureAt = useShellStore((s) => {
    const error = s.rest?.status?.last_error;
    return error && error.kind.startsWith("file.") ? error.occurred_at : null;
  });
  useEffect(() => {
    if (failureAt !== null) useShellStore.getState().noteSaving(tab.id, false);
  }, [failureAt, tab.id]);

  // A reconnect may have left an unsaved draft in IndexedDB (B8): the draft
  // is the newest edit, so it is restored over a clean core document and
  // dropped when the core already holds the same contents. A draft is never
  // dropped for its age (S5.5 B12).
  useEffect(() => {
    checked.current = false;
    const key = keyRef.current;
    if (connection !== "live" || !key) return;
    let live = true;
    void flushBuffer(key).then(() => claimLegacyBuffer(key)).then(allBuffers).then((buffers) => {
      if (!live) return;
      const buffer = bufferFor(buffers, key);
      checked.current = true;
      if (!buffer) return;
      const current = latest.current;
      if (!current) return;
      // A read-only or preview-only document takes no draft, so this one
      // cannot be restored into it; it stays a recovery item to export or
      // discard rather than being deleted here (D-14, B11).
      if (current.readonly_reason !== null || current.contents_utf8 === null) {
        useShellStore
          .getState()
          .noteDiagnostic(`an unsaved draft for ${tab.path} is kept for recovery: the document is not editable here`);
        return;
      }
      const decision = bufferDecision(buffer, current);
      if (decision === "restore") {
        noteDraft(tab.id, buffer.contents);
        actions.updateDraft(buffer.contents);
      } else if (decision === "drop") void deleteBuffer(key);
    });
    return () => {
      live = false;
    };
  }, [tab.id, draftId, tab.path, actions, connection]);

  // A document the core reports clean holds the draft on disk, so its copy
  // goes: the core is clean only after a save of the draft landed, or when no
  // draft was ever made (the restore above runs first).
  useEffect(() => {
    const key = keyRef.current;
    if (!key || !checked.current || document?.dirty) return;
    if (document?.document_kind !== "text" && document?.document_kind !== "markdown") return;
    void deleteBuffer(key);
  }, [document?.dirty, document?.document_kind, draftId]);

  if (!document) {
    return <Notice text="Loading…" state="loading" />;
  }
  if (!editable) {
    return <FileViewer document={document} />;
  }
  // A bare browser cannot prove that the daemon is on the viewer's machine,
  // even when its URL is localhost through an SSH tunnel (D-12).
  if (document.contents_utf8 === null) {
    return <PreviewOnly document={document} />;
  }
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {document.readonly_reason ? (
        <div className="border-b border-divider px-md py-xs text-caption text-warning" data-editor-readonly="true">
          {document.readonly_reason}
        </div>
      ) : null}
      {hold === "held" ? (
        <div role="status" className="border-b border-divider px-md py-xs text-caption text-warning" data-editor-draft-hold="held">
          Unsaved drafts cannot be stored right now (their storage is full at 512 MiB or this browser's quota, or unavailable), so this document stays read-only until the unstored draft is saved, exported or discarded. Stored drafts are never removed to make room.
        </div>
      ) : null}
      {hold === "unstored" ? (
        <div role="status" className="flex items-center gap-sm border-b border-divider px-md py-xs text-caption text-warning" data-editor-draft-hold="unstored">
          <span className="min-w-0 flex-1">This draft is not stored: draft storage is full or unavailable, so it lives in this tab only. Save or export it; the next edit is stored again once there is room.</span>
          <ExportDraftButton tabId={tab.id} path={tab.path} />
        </div>
      ) : null}
      {document.conflict ? (
        <ConflictBar tabId={tab.id} draftKey={draftKey} path={tab.path} removed={document.conflict.disk_revision === null} actions={actions} />
      ) : null}
      {document.save && document.save.state !== "saving" ? <SaveStatusBar tabId={tab.id} path={tab.path} save={document.save} actions={actions} /> : null}
      <CodeMirrorEditor
        key={tab.id}
        tabId={tab.id}
        document={document}
        scale={scale}
        wrap={tab.wrap}
        live={document.document_kind === "markdown" && tab.markdown_live}
        findRequest={findRequest}
        held={hold === "held"}
        onDraft={(contents) => {
          noteDraft(tab.id, contents); 
          // A buffer that cannot be stored keeps the edit alive and says so on
          // the tab; the session continues either way (D-14).
          if (draftKey) {
            queueBuffer(draftKey, contents, (stored) => {
              if (stored !== null) useShellStore.getState().noteBufferWarning(tab.id, !stored);
            });
          } else {
            useShellStore.getState().noteBufferWarning(tab.id, true);
          }
          actions.updateDraft(contents);
          scheduleAutosave();
        }}
      />
    </div>
  );
}

function PreviewOnly({ document }: { document: EditorDocumentSnapshot }) {
  const [downloadError, setDownloadError] = useState<string | null>(null);
  const source = useFileSource();
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-sm px-md text-center text-caption text-muted" data-editor-preview-only="true">
      <span>{document.readonly_reason ?? "This file is too large to edit here."}</span>
      <button
        type="button"
        className="rounded-sm bg-elevated px-md py-xs text-primary hover:bg-divider"
        data-editor-download="true"
        onClick={() => {
          setDownloadError(null);
          void downloadFile(document.path, source).catch((error: unknown) => {
            if ((error as { name?: string }).name !== "AbortError") {
              setDownloadError(error instanceof Error ? error.message : "download_failed");
            }
          });
        }}
      >
        Download
      </button>
      {downloadError ? <span className="text-danger" data-editor-download-failed="true">The download failed: {downloadError}</span> : null}
    </div>
  );
}

/**
 * Saves the draft the operator sees to a file of their choosing, for a
 * document whose own save cannot land (B13-B15): the draft leaves through the
 * browser's download, never through the host's filesystem.
 */
function exportDraft(tabId: string, path: string) {
  const contents = latestDraft(tabId) ?? useShellStore.getState().editor?.document?.contents_utf8 ?? "";
  const url = URL.createObjectURL(new Blob([contents], { type: "text/plain;charset=utf-8" }));
  const link = window.document.createElement("a");
  link.href = url;
  link.download = `${path.split("/").pop() || "draft"}.draft`;
  link.click();
  URL.revokeObjectURL(url);
  // The exported text no longer depends on this tab, so a device removal
  // held for it can go ahead (B26, B44).
  useShellStore.getState().noteDraftExported(tabId, contents);
}

function ExportDraftButton({ tabId, path }: { tabId: string; path: string }) {
  return (
    <button type="button" className="text-secondary hover:text-primary" data-export-draft="true" onClick={() => exportDraft(tabId, path)}>
      Export draft
    </button>
  );
}

/**
 * A save that has no result yet: one waiting for the device's helper, or one
 * whose answer was lost, which is never resent and is read back to settle it
 * (B14). Or a save that was refused or not sent, with its reason, Export and
 * Retry at the same place; autosave waits for Retry rather than repeating a
 * refusal (S5.5 B15, B45).
 */
function SaveStatusBar({ tabId, path, save, actions }: { tabId: string; path: string; save: NonNullable<EditorDocumentSnapshot["save"]>; actions: Actions }) {
  // `not_applied` is an unanswered save read back unchanged: it has not
  // reached the file yet but may still, so it is not called unsaved. Retry
  // is safe there: a save that meets the late one lands as a conflict.
  const prefix = save.state === "refused" ? "Not saved: " : save.state === "not_applied" ? "Not saved yet: " : "";
  const settled = prefix !== "";
  return (
    <div role="status" className="flex flex-wrap items-center gap-sm border-b border-divider px-md py-xs text-caption text-warning" data-editor-save-state={save.state}>
      <span className="min-w-0 flex-1 break-words">
        {prefix}
        {save.message ?? "The last save's result is unknown; reading the file back."}
        {settled ? "" : " Your draft is preserved."}
      </span>
      <ExportDraftButton tabId={tabId} path={path} />
      {settled ? (
        <button type="button" className="text-secondary hover:text-primary" data-save-retry="true" onClick={() => actions.saveFile(tabId)}>
          Retry
        </button>
      ) : null}
    </div>
  );
}

function ConflictBar({ tabId, draftKey, path, removed, actions }: { tabId: string; draftKey: BufferKey | null; path: string; removed: boolean; actions: Actions }) {
  return (
    <div className="flex items-center gap-sm border-b border-divider px-md py-xs text-caption text-warning" data-editor-conflict="true">
      <span className="flex-1">
        {removed ? "This file was removed or could not be read back." : "This file changed on disk."} Your draft is preserved.
      </span>
      <ExportDraftButton tabId={tabId} path={path} />
      <button
        type="button"
        className="text-secondary hover:text-primary"
        data-conflict-action="reload"
        onClick={() => {
          clearDraft(tabId);
          if (draftKey) void deleteBuffer(draftKey);
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
