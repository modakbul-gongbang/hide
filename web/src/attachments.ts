// Terminal attachments (PRD B14, B15, D-02). The browser reads a dropped
// file's bytes or a pasted clipboard image, but it cannot name a path, so the
// bytes go to hided, which stages them and runs the Swift shell's
// `terminal_attachment` flow. The caps are hided's and the core's; a refusal
// is one line over the pane, and hided's reason codes become that line here.

import { useShellStore } from "./store";
import type { DispatchFn } from "./ws";

/** One binary upload frame carries at most this many bytes. */
export const CHUNK_BYTES = 4 * 1024 * 1024;

export type AttachmentInput = { name: string; bytes: Uint8Array };

type Sender = { dispatch: DispatchFn; sendBinary: (bytes: Uint8Array) => void };

let sender: Sender | null = null;
/** The pane a request belongs to, so a refusal names its pane. A refusal
 * arrives after the commit, so entries are kept until one does; the map is
 * capped so a long session cannot grow it without bound. */
const paneByRequest = new Map<string, string>();
const MAX_TRACKED_REQUESTS = 64;

function rememberPane(requestId: string, paneId: string): void {
  paneByRequest.set(requestId, paneId);
  while (paneByRequest.size > MAX_TRACKED_REQUESTS) {
    const oldest = paneByRequest.keys().next().value;
    if (oldest === undefined) break;
    paneByRequest.delete(oldest);
  }
}

export function configureAttachments(value: Sender): void {
  sender = value;
}

/** The one-line text a refusal reason becomes. */
export function refusalText(reason: string): string {
  switch (reason) {
    case "too_large":
      return "A file exceeds the 20 MiB attachment limit.";
    case "batch_too_large":
      return "The selection exceeds the 40 MiB attachment limit.";
    case "too_many_files":
      return "Choose between 1 and 8 regular files.";
    case "staging_full":
      return "Too many attachments are staged right now; try again in a moment.";
    case "invalid_request_id":
      return "That file name cannot be attached.";
    case "size_mismatch":
      return "The file changed while it was being attached.";
    case "unknown_stage":
    case "stage_incomplete":
      return "The attachment upload did not finish.";
    case "stage_failed":
      return "The file could not be staged for the terminal.";
    case "forward_failed":
      return "The attachment could not be delivered to the terminal.";
    case "no_pane":
      return "There is no terminal pane to attach to.";
    default:
      return "The attachment was refused.";
  }
}

export function receiveAttachmentRefusal(payload: { request_id: string; reason: string }): void {
  const paneId = paneByRequest.get(payload.request_id);
  for (const key of [...paneByRequest.keys()]) {
    if (key === payload.request_id || key.startsWith(`${payload.request_id}-`)) paneByRequest.delete(key);
  }
  if (paneId) useShellStore.getState().setAttachmentRefusal({ pane_id: paneId, reason: payload.reason });
}

/** One upload frame: a 4-byte big-endian header length, the header JSON, bytes. */
export function frameChunk(requestId: string, offset: number, bytes: Uint8Array, eof: boolean): Uint8Array {
  const header = new TextEncoder().encode(JSON.stringify({ request_id: requestId, offset, eof }));
  const out = new Uint8Array(4 + header.length + bytes.length);
  new DataView(out.buffer).setUint32(0, header.length, false);
  out.set(header, 4);
  out.set(bytes, 4 + header.length);
  return out;
}

function sendChunks(requestId: string, bytes: Uint8Array): void {
  const active = sender;
  if (!active) return;
  let offset = 0;
  do {
    const next = Math.min(offset + CHUNK_BYTES, bytes.length);
    active.sendBinary(frameChunk(requestId, offset, bytes.subarray(offset, next), next >= bytes.length));
    offset = next;
  } while (offset < bytes.length);
}

/**
 * Uploads a batch and commits it: one `terminal_attachment` reaches the core
 * with the staged paths (or, for a clipboard image, none, because the core
 * reads the staged image from its own path).
 */
export function submitAttachments(paneId: string, files: AttachmentInput[], clipboard: boolean, bracketedPaste: boolean): void {
  if (!sender || files.length === 0) return;
  // The caps the daemon enforces, checked before a byte leaves the page: a
  // 2 GiB drop must not be read into memory to be refused.
  const reason = preflightRefusal(files);
  if (reason) {
    useShellStore.getState().setAttachmentRefusal({ pane_id: paneId, reason });
    return;
  }
  const batch = crypto.randomUUID();
  rememberPane(batch, paneId);
  const stages: string[] = [];
  for (const [index, file] of files.entries()) {
    // A clipboard image stages at the one path the core reads it from, so its
    // stage id is the batch id the commit and the readiness report carry.
    const stage = clipboard ? batch : `${batch}-${index}`;
    stages.push(stage);
    rememberPane(stage, paneId);
    sender.dispatch({
      schema_version: 2,
      kind: "attachment_stage",
      payload: { request_id: stage, name: file.name, size: file.bytes.byteLength, clipboard },
    });
    sendChunks(stage, file.bytes);
  }
  sender.dispatch({
    schema_version: 2,
    kind: "attachment_commit",
    payload: {
      request_id: batch,
      pane_id: paneId,
      bracketed_paste: bracketedPaste,
      clipboard,
      stages,
    },
  });
}

/**
 * Reads and stages dropped or pasted files, refusing over-cap ones from
 * `File.size` before any byte is read: a 2 GiB drop must not become an
 * allocation to learn it is over the cap.
 */
export async function submitFiles(paneId: string, files: File[], clipboard: boolean, bracketedPaste: boolean): Promise<void> {
  // A clipboard paste carries many representations of one image; the first
  // file is the image itself, and the batch cap allows exactly one.
  const list = Array.from(files);
  const selected = clipboard ? list.slice(0, 1) : list;
  const reason = filesRefusal(selected);
  if (reason) {
    useShellStore.getState().setAttachmentRefusal({ pane_id: paneId, reason });
    return;
  }
  submitAttachments(paneId, await readInputs(selected), clipboard, bracketedPaste);
}

/** The caps a batch of files fails, read from their sizes alone. */
export function filesRefusal(files: File[]): string | null {
  if (files.length === 0 || files.length > MAX_FILES) return "too_many_files";
  if (files.some((file) => file.size > MAX_FILE_BYTES)) return "too_large";
  const total = files.reduce((sum, file) => sum + file.size, 0);
  if (total > MAX_BATCH_BYTES) return "batch_too_large";
  return null;
}

/** The cap a batch fails, or null when it is within every one of them. */
export function preflightRefusal(files: AttachmentInput[]): string | null {
  if (files.length === 0) return "too_many_files";
  if (files.length > MAX_FILES) return "too_many_files";
  if (files.some((file) => file.bytes.byteLength > MAX_FILE_BYTES)) return "too_large";
  const total = files.reduce((sum, file) => sum + file.bytes.byteLength, 0);
  if (total > MAX_BATCH_BYTES) return "batch_too_large";
  return null;
}

/** The caps the daemon enforces, so the page refuses the same shapes first. */
export const MAX_FILE_BYTES = 20 * 1024 * 1024;
export const MAX_BATCH_BYTES = 40 * 1024 * 1024;
export const MAX_FILES = 8;

/** A name the daemon accepts: one path component, at most 96 bytes. A Finder
 * name outside that shape is staged under a neutral one, not refused. */
export function safeName(name: string): string {
  const byteLength = new TextEncoder().encode(name).length;
  if (name === "" || byteLength > 96) return "attachment.bin";
  if (name === "." || name === ".." || name.includes("/") || name.includes("\\")) return "attachment.bin";
  // `char::is_control` covers both C0 and the C1 range.
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f-\u009f]/.test(name)) return "attachment.bin";
  return name;
}

export async function readInputs(files: FileList | File[]): Promise<AttachmentInput[]> {
  const list = Array.from(files);
  return Promise.all(
    list.map(async (file) => ({ name: safeName(file.name), bytes: new Uint8Array(await file.arrayBuffer()) })),
  );
}
