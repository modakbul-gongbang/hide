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
/** The pane an in-flight request belongs to, so a refusal names its pane. */
const paneByRequest = new Map<string, string>();

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
  const batch = crypto.randomUUID();
  paneByRequest.set(batch, paneId);
  const stages: string[] = [];
  for (const [index, file] of files.entries()) {
    const stage = `${batch}-${index}`;
    stages.push(stage);
    paneByRequest.set(stage, paneId);
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

/** The image files a clipboard or drop carries, as attachment inputs. */
export function imageInputs(files: FileList | null): File[] {
  if (!files) return [];
  return Array.from(files).filter((file) => file.type.startsWith("image/"));
}

export async function readInputs(files: FileList | File[]): Promise<AttachmentInput[]> {
  const list = Array.from(files);
  return Promise.all(
    list.map(async (file) => ({ name: file.name || "attachment.bin", bytes: new Uint8Array(await file.arrayBuffer()) })),
  );
}
