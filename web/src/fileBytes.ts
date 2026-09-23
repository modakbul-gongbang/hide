// The file-bytes reader (PRD B6, B7, D-07): one request per path, answered by
// one or more binary frames on the token-gated socket. A viewer asks for the
// bytes of an image, a PDF or a video and gets a promise; the frames arrive
// through `ws.ts`, which knows nothing about viewers. A request that is
// abandoned (the tab closed) resolves to nothing.

import type { DispatchFn } from "./ws";

export type BytesHeader = {
  type: "file_bytes";
  request_id: string;
  path: string;
  offset: number;
  total: number;
  eof: boolean;
};

type Pending = {
  path: string;
  total: number;
  chunks: Uint8Array[];
  received: number;
  resolve: (bytes: Uint8Array) => void;
  reject: (error: Error) => void;
};

let dispatchFn: DispatchFn | null = null;
const pending = new Map<string, Pending>();
let nextRequest = 1;

export function configureFileBytes(dispatch: DispatchFn): void {
  dispatchFn = dispatch;
}

/** A header for a binary frame: 4-byte big-endian length, JSON, then bytes. */
function parseFrame(data: ArrayBuffer): { header: BytesHeader; bytes: Uint8Array } | null {
  if (data.byteLength < 4) return null;
  const view = new DataView(data);
  const headerLength = view.getUint32(0, false);
  if (4 + headerLength > data.byteLength) return null;
  try {
    const header = JSON.parse(new TextDecoder().decode(new Uint8Array(data, 4, headerLength))) as BytesHeader;
    return { header, bytes: new Uint8Array(data, 4 + headerLength) };
  } catch {
    return null;
  }
}

export function receiveBytes(data: ArrayBuffer): void {
  const parsed = parseFrame(data);
  if (!parsed) return;
  const { header, bytes } = parsed;
  const entry = pending.get(header.request_id);
  if (!entry) return;
  entry.chunks.push(bytes);
  entry.received += bytes.byteLength;
  entry.total = header.total;
  if (header.eof) {
    pending.delete(header.request_id);
    const joined = new Uint8Array(entry.received);
    let at = 0;
    for (const chunk of entry.chunks) {
      joined.set(chunk, at);
      at += chunk.byteLength;
    }
    entry.resolve(joined);
  }
}

export function receiveBytesError(payload: { request_id: string; reason: string }): void {
  const entry = pending.get(payload.request_id);
  if (!entry) return;
  pending.delete(payload.request_id);
  entry.reject(new Error(payload.reason));
}

/**
 * A boundary refusal names the path, not a request id, so every read of that
 * path is settled with the daemon's reason: the viewer shows one line instead
 * of waiting forever.
 */
export function receiveBytesRefusal(path: string, reason: string): void {
  for (const [requestId, entry] of pending) {
    if (entry.path !== path) continue;
    pending.delete(requestId);
    entry.reject(new Error(reason));
  }
}

/** A socket that is gone cannot answer; every in-flight read fails at once. */
export function clearPending(reason: string): void {
  for (const [requestId, entry] of pending) {
    pending.delete(requestId);
    entry.reject(new Error(reason));
  }
}

/**
 * Reads a file under a registered checkout root. `length` absent reads to the
 * end; a range read names an offset and a length. The promise rejects with the
 * daemon's reason code (`outside_checkout`, `too_large`, `read_failed`, ...).
 */
export function requestFileBytes(path: string, range?: { offset?: number; length?: number }): Promise<Uint8Array> {
  return new Promise((resolve, reject) => {
    if (!dispatchFn) {
      reject(new Error("not_connected"));
      return;
    }
    const request_id = `bytes-${nextRequest}`;
    nextRequest += 1;
    pending.set(request_id, { path, total: 0, chunks: [], received: 0, resolve, reject });
    dispatchFn({
      schema_version: 2,
      kind: "file_bytes",
      payload: {
        request_id,
        path,
        offset: range?.offset ?? 0,
        length: range?.length ?? null,
      },
    });
  });
}

/** A blob URL for bytes, and the one place a viewer revokes it. */
export function blobUrl(bytes: Uint8Array, type: string): string {
  return URL.createObjectURL(new Blob([bytes as BlobPart], { type }));
}
