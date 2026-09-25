import { useEffect, useRef, useState } from "react";
import type { Actions } from "./actions";
import { allBuffers, bufferDecision, bufferFor, claimLegacyBuffer, deleteBuffer, draftStorageHold, flushBuffer, identity, queueBuffer, tabBufferKey, type BufferKey } from "./buffers";
import { CodeMirrorEditor } from "./editor/CodeMirrorEditor";
import { PatchView } from "./editor/PatchView";
import { clearDraft, closingWithSave, latestDraft, noteDraft } from "./editor/draft";
import { downloadFile } from "./fileBytes";
import { changesFor, editorTabFor, type EditorDocumentSnapshot, type EditorTabSnapshot, type ViewDisplaySnapshot } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { FileViewer } from "./viewers/FileViewer";
import { useFileSource } from "./viewers/useFileBytes";

// The document surface (PRD B3-B7; S7 B4, B5, A9, A10). The core owns the
// documents and the displays that show them; this draws one display - its
// document from the `documents` section, or its diff from `changes.diffs` -
// and dispatches the events that change it. Text and Markdown edit in
// CodeMirror; a document the core classified as binary takes no edits, and a
// readonly one says why in one line.
//
// One document may show in several displays. It is still one buffer: its
// stored draft, autosave, saving mark and save-on-leave are kept once per
// document by `DocumentKeeper`, and every display's edits reach the core as
// that document's drafts.

const DEFAULT_SCALE = 1;

/** How long editing must be idle before the shell saves (PRD S3 D-10). */
export const AUTOSAVE_IDLE_MS = 600;

type ShellState = ReturnType<typeof useShellStore.getState>;

/** Where a document's unsaved draft is stored: this daemon host, the device, its checkout root and the real path (D-14, S5.5 B9). */
function documentBufferKey(state: ShellState, tabId: string): BufferKey | null {
  const tab = editorTabFor(state.editor, tabId);
  return tab && tab.kind === "file" ? tabBufferKey(state.daemon?.host_id, state.rest, tab) : null;
}

function autosaveDue(document: EditorDocumentSnapshot | null | undefined): boolean {
  return (
    !!document &&
    (document.document_kind === "text" || document.document_kind === "markdown") &&
    !document.readonly_reason &&
    !document.conflict &&
    // A save whose answer was lost blocks the next one until it is read
    // back; a running or waiting save takes the newest draft behind it.
    (!document.save || document.save.state === "saving" || document.save.state === "waiting") &&
    document.dirty
  );
}

/** One idle timer per document, whichever display the last edit came from. */
const autosaves = new Map<string, number>();

/** Saves a document once editing it has been idle for a moment. */
function scheduleAutosave(tabId: string, actions: Actions): void {
  window.clearTimeout(autosaves.get(tabId));
  autosaves.set(
    tabId,
    window.setTimeout(() => {
      autosaves.delete(tabId);
      const state = useShellStore.getState();
      if (!autosaveDue(state.documents[tabId]) || state.connection !== "live" || closingWithSave(tabId)) return;
      state.noteSaving(tabId, actions.saveFile(tabId) === true);
    }, AUTOSAVE_IDLE_MS),
  );
}

function cancelAutosave(tabId: string): void {
  window.clearTimeout(autosaves.get(tabId));
  autosaves.delete(tabId);
}

/** One edit, from whichever display of the document it was made in. */
function draftEdited(tabId: string, contents: string, actions: Actions): void {
  noteDraft(tabId, contents);
  // A buffer that cannot be stored keeps the edit alive and says so on the
  // tab; the session continues either way (D-14).
  const key = documentBufferKey(useShellStore.getState(), tabId);
  if (key) {
    queueBuffer(key, contents, (stored) => {
      if (stored !== null) useShellStore.getState().noteBufferWarning(tabId, !stored);
    });
  } else {
    useShellStore.getState().noteBufferWarning(tabId, true);
  }
  actions.updateDraft(tabId, contents);
  scheduleAutosave(tabId, actions);
}

/**
 * The bookkeeping of one file document on screen, mounted once however many
 * displays show it: the autosave window, the saving mark, the stored draft's
 * restore and removal, and the save of a dirty draft the operator leaves
 * behind. It draws nothing.
 */
export function DocumentKeeper({ tabId, actions }: { tabId: string; actions: Actions }) {
  const document = useShellStore((s) => s.documents[tabId] ?? null);
  const path = useShellStore((s) => editorTabFor(s.editor, tabId)?.path ?? null);
  const draftId = useShellStore((s) => {
    const key = documentBufferKey(s, tabId);
    return key ? identity(key) : null;
  });
  const connection = useShellStore((s) => s.connection);
  const latest = useRef(document);
  latest.current = document;
  const checked = useRef(false);
  const hasDocument = document !== null;

  // A saved document needs no stored draft, so its tab-only mark goes with
  // the save, and with it the hold on every other document.
  const clean = document ? !document.dirty : false;
  useEffect(() => {
    if (clean) useShellStore.getState().noteBufferWarning(tabId, false);
  }, [clean, tabId]);

  // Leaving the document ends its idle window: the timer goes, and a dirty
  // draft the operator left behind is saved rather than waiting for a return
  // that may never come (D-10). A document the operator closed carries its
  // draft in the close itself, so it is not saved a second time here.
  useEffect(
    () => () => {
      cancelAutosave(tabId);
      const current = latest.current;
      if (!current?.dirty || current.conflict || closingWithSave(tabId)) return;
      const stillOpen = useShellStore.getState().editor?.tabs.some((row) => row.id === tabId);
      if (stillOpen) actions.saveFile(tabId);
    },
    [tabId, actions],
  );

  // A conflict pauses autosave until the operator chooses; the choice (or the
  // next edit) resumes it, because the conflict clears and the document is
  // still dirty (D-10, B5).
  useEffect(() => {
    if (document?.conflict || connection !== "live") {
      cancelAutosave(tabId);
      useShellStore.getState().noteSaving(tabId, false);
      return;
    }
    if (document?.dirty) scheduleAutosave(tabId, actions);
  }, [document?.conflict, document?.dirty, tabId, connection, actions]);

  // The save landed when the core reports the document clean.
  useEffect(() => {
    if (document?.dirty === false) useShellStore.getState().noteSaving(tabId, false);
  }, [document?.dirty, tabId]);

  // A refused save never reaches the clean state, so the core's failure is
  // what takes the saving mark off the tab; the document stays dirty and the
  // reason is in the diagnostic log (B5). Only a file error settles a save;
  // a device or catalog error that lands while a slow device save runs
  // leaves its mark alone (S5.5 B45).
  const failureAt = useShellStore((s) => {
    const error = s.rest?.status?.last_error;
    return error && error.kind.startsWith("file.") ? error.occurred_at : null;
  });
  useEffect(() => {
    if (failureAt !== null) useShellStore.getState().noteSaving(tabId, false);
  }, [failureAt, tabId]);

  // A reconnect may have left an unsaved draft in IndexedDB (B8): the draft
  // is the newest edit, so it is restored over a clean core document and
  // dropped when the core already holds the same contents. A draft is never
  // dropped for its age (S5.5 B12). It waits for the document itself, which
  // arrives with the display that shows it.
  useEffect(() => {
    checked.current = false;
    const key = documentBufferKey(useShellStore.getState(), tabId);
    if (connection !== "live" || !key || !hasDocument) return;
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
        useShellStore.getState().noteDiagnostic(`an unsaved draft for ${path ?? tabId} is kept for recovery: the document is not editable here`);
        return;
      }
      const decision = bufferDecision(buffer, current);
      if (decision === "restore") {
        noteDraft(tabId, buffer.contents);
        actions.updateDraft(tabId, buffer.contents);
      } else if (decision === "drop") void deleteBuffer(key);
    });
    return () => {
      live = false;
    };
  }, [tabId, draftId, path, actions, connection, hasDocument]);

  // A document the core reports clean holds the draft on disk, so its copy
  // goes: the core is clean only after a save of the draft landed, or when no
  // draft was ever made (the restore above runs first).
  useEffect(() => {
    const key = documentBufferKey(useShellStore.getState(), tabId);
    if (!key || !checked.current || document?.dirty) return;
    if (document?.document_kind !== "text" && document?.document_kind !== "markdown") return;
    void deleteBuffer(key);
  }, [document?.dirty, document?.document_kind, draftId, tabId]);

  return null;
}

/**
 * One open display (S7 B4): its header and its document or diff. `placeKey`
 * names the display within its Workspace, so its selection and scroll come
 * back with it.
 */
export function DisplayEditor({ display, placeKey, actions }: { display: ViewDisplaySnapshot; placeKey: string; actions: Actions }) {
  const tab = useShellStore((s) => editorTabFor(s.editor, display.tab_id));
  const document = useShellStore((s) => (display.tab_id ? (s.documents[display.tab_id] ?? null) : null));
  const scaleValue = useShellStore((s) => s.rest?.ui_state?.editor_text_scale);
  const scale = typeof scaleValue === "number" ? scaleValue : DEFAULT_SCALE;
  if (!tab) return <Notice text="Loading…" state="loading" />;
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-background" data-editor={tab.id} data-editor-kind={display.kind} data-editor-display={display.id}>
      <EditorHeader display={display} tab={tab} document={document} actions={actions} />
      {display.kind === "diff" ? (
        <DiffBody display={display} scale={scale} />
      ) : (
        <FileBody display={display} tab={tab} document={document} placeKey={placeKey} scale={scale} actions={actions} />
      )}
    </div>
  );
}

/** A diff display's own entry of `changes.diffs`, found by its path and group (S7 A5, A10). */
function DiffBody({ display, scale }: { display: ViewDisplaySnapshot; scale: number }) {
  const changes = useShellStore((s) => changesFor(s.changes, s.rest?.navigator?.changes_root_path ?? null));
  if (!changes) return <Notice text="Reading the diff…" state="diff-loading" />;
  if (changes.unavailable_reason) return <Notice text={`History is unavailable: ${changes.unavailable_reason}`} state="diff-unavailable" />;
  const committed = display.committed === true;
  const group = committed ? changes.committed : changes.entries;
  if (!group.some((entry) => entry.path === display.path)) {
    return <Notice text="This file is no longer in the selected History group. Close this view or choose another row." state="diff-unavailable" />;
  }
  const diff = (changes.diffs ?? []).find((row) => row.path === display.path && row.committed === committed) ?? null;
  if (!diff) return <Notice text="Reading the diff…" state="diff-loading" />;
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col" data-diff-path={display.path} data-diff-group={committed ? "committed" : "working"}>
      {diff.notice ? <div className="border-b border-divider px-md py-xs text-caption text-warning" data-diff-notice="true">{diff.notice}</div> : null}
      {diff.text ? <PatchView text={diff.text} scale={scale} /> : <div className="min-h-0 flex-1" data-diff-empty="true" />}
    </div>
  );
}

function EditorHeader({
  display,
  tab,
  document,
  actions,
}: {
  display: ViewDisplaySnapshot;
  tab: EditorTabSnapshot;
  document: EditorDocumentSnapshot | null;
  actions: Actions;
}) {
  const file = display.kind === "file";
  const isMarkdown = file && document?.document_kind === "markdown";
  const editable = document?.document_kind === "text" || isMarkdown;
  const group = display.committed ? "Committed on branch" : "Uncommitted";
  return (
    <div className="flex shrink-0 items-center gap-sm border-b border-divider px-md py-xs text-caption text-secondary">
      <span className="min-w-0 flex-1 truncate" title={file ? display.path : `${group}: ${display.path}`} data-editor-path="true">
        {file ? "" : `${display.committed ? "Branch diff" : "Working diff"} · `}{display.path}
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
          onClick={() => actions.setFileView(tab.id, !tab.markdown_live, tab.wrap)}
        >
          {tab.markdown_live ? "Live" : "Source"}
        </button>
      ) : null}
      {file && editable ? (
        <button
          type="button"
          className={tab.wrap ? "text-primary" : "text-muted hover:text-primary"}
          data-editor-wrap={tab.wrap ? "true" : "false"}
          title="Wrap lines"
          onClick={() => actions.setFileView(tab.id, tab.markdown_live, !tab.wrap)}
        >
          Wrap
        </button>
      ) : null}
      {file ? (
        <button
          type="button"
          className="text-muted hover:text-primary"
          aria-label="Find in document"
          title="Find in document (⌘F)"
          disabled={!editable}
          onClick={() => actions.requestEditorFind(display.id)}
        >
          Find
        </button>
      ) : null}
      {display.preview ? (
        <button
          type="button"
          className="text-muted hover:text-primary"
          title="Keep open (⌘⇧K)"
          data-editor-preview="true"
          onClick={() => actions.keepViewOpen(display.id)}
        >
          preview
        </button>
      ) : null}
    </div>
  );
}

function FileBody({
  display,
  tab,
  document,
  placeKey,
  scale,
  actions,
}: {
  display: ViewDisplaySnapshot;
  tab: EditorTabSnapshot;
  document: EditorDocumentSnapshot | null;
  placeKey: string;
  scale: number;
  actions: Actions;
}) {
  const findRequest = useUiStore((s) => s.editorFindRequest);
  const findTarget = useUiStore((s) => s.editorFindDisplay === display.id);
  // A draft store that refused a write holds every other clean document
  // read-only until there is room again (B44); only open documents count, so
  // a closed one's old refusal does not hold the editor forever.
  const storageFull = useShellStore((s) => (s.editor?.tabs ?? []).some((row) => s.bufferWarnings.has(row.id)));
  const unstored = useShellStore((s) => s.bufferWarnings.has(tab.id));
  const hold = draftStorageHold({ storageFull, unstored, dirty: document?.dirty ?? false });
  if (!document) {
    return <Notice text="Loading…" state="loading" />;
  }
  if (document.document_kind !== "text" && document.document_kind !== "markdown") {
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
        <ConflictBar tabId={tab.id} path={tab.path} removed={document.conflict.disk_revision === null} actions={actions} />
      ) : null}
      {document.save && document.save.state !== "saving" ? <SaveStatusBar tabId={tab.id} path={tab.path} save={document.save} actions={actions} /> : null}
      <CodeMirrorEditor
        key={placeKey}
        tabId={tab.id}
        placeKey={placeKey}
        document={document}
        scale={scale}
        wrap={tab.wrap}
        live={document.document_kind === "markdown" && tab.markdown_live}
        findRequest={findRequest}
        findTarget={findTarget}
        held={hold === "held"}
        onDraft={(contents) => draftEdited(tab.id, contents, actions)}
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
  const contents = latestDraft(tabId) ?? useShellStore.getState().documents[tabId]?.contents_utf8 ?? "";
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

function ConflictBar({ tabId, path, removed, actions }: { tabId: string; path: string; removed: boolean; actions: Actions }) {
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
          const key = documentBufferKey(useShellStore.getState(), tabId);
          if (key) void deleteBuffer(key);
          actions.resolveConflict(tabId, "reload");
        }}
      >
        Reload disk version
      </button>
      <button
        type="button"
        className="text-secondary hover:text-primary"
        data-conflict-action="keep_editing"
        onClick={() => actions.resolveConflict(tabId, "keep_editing")}
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
