// The images one Markdown Live view draws (PRD markdown-html): a document
// names its images, and this reads them under a cap. A file under the checkout
// comes through the same `file_bytes` path an image View uses (so a checkout
// on a connected device works), and its blob URL is kept by source so a
// widget drawn again does not read it again. Web addresses are handed to the
// browser as they are; they count against the image cap but their bytes are
// the browser's. The view disposes the cache with itself, which revokes every
// blob URL it made.

import { blobUrl, requestFileBytesWithTotal, type FileSource } from "../fileBytes";
import type { ImageTarget } from "./liveHtml";

/** What one document can load: images drawn, bytes read, and one file's size. */
export const LIVE_IMAGE_LIMITS = {
  images: 64,
  bytes: 32 * 1024 * 1024,
  file: 8 * 1024 * 1024,
} as const;

export type FileRead = (path: string, source: FileSource, limit: number) => Promise<{ bytes: Uint8Array; total: number }>;

/** The read an image View uses, bounded to `limit` bytes plus one so an oversize file is told by its total. */
export const readImageFile: FileRead = (path, source, limit) => requestFileBytesWithTotal(path, { offset: 0, length: limit + 1 }, source);

/** Reasons a read failed that a later draw may not fail with. */
const TRANSIENT = new Set(["superseded", "relay_busy", "not_connected"]);

export class LiveImages {
  private readonly loaded = new Map<string, Promise<string | null>>();
  private readonly urls: string[] = [];
  private bytes = 0;
  private disposed = false;
  private warned = new Set<string>();
  // hided runs two device reads per client at once and ends the oldest when a
  // third arrives, and other views of the window share those two, so this
  // view holds one read at a time.
  private queue: Promise<unknown> = Promise.resolve();

  constructor(
    private readonly read: FileRead,
    private readonly log: (message: string) => void,
  ) {}

  /** The URL to draw, or null when the image may not or could not be loaded. */
  load(target: ImageTarget, source: FileSource): Promise<string | null> {
    if (target.kind === "refused") return Promise.resolve(null);
    const key = target.kind === "file" ? `file:${source?.device ?? ""}:${target.path}` : `${target.kind}:${target.url}`;
    const known = this.loaded.get(key);
    if (known) return known;
    if (this.loaded.size >= LIVE_IMAGE_LIMITS.images) {
      this.warn("images", `markdown live: more than ${LIVE_IMAGE_LIMITS.images} images in one document, the rest show their alt text`);
      return Promise.resolve(null);
    }
    const pending = target.kind === "file" ? this.enqueue(() => this.fetch(target, source, key)) : this.fetch(target, source, key);
    this.loaded.set(key, pending);
    return pending;
  }

  dispose(): void {
    this.disposed = true;
    for (const url of this.urls.splice(0)) URL.revokeObjectURL(url);
    this.loaded.clear();
  }

  private enqueue<T>(run: () => Promise<T>): Promise<T> {
    const next = this.queue.then(run, run);
    this.queue = next.catch(() => undefined);
    return next;
  }

  private async fetch(target: Exclude<ImageTarget, { kind: "refused" }>, source: FileSource, key: string): Promise<string | null> {
    if (target.kind === "remote") return target.url;
    if (target.kind === "data") return this.charge(target.url.length) ? target.url : null;
    try {
      const { bytes, total } = await this.read(target.path, source, LIVE_IMAGE_LIMITS.file);
      if (total > LIVE_IMAGE_LIMITS.file) {
        this.warn("file", `markdown live: ${target.path} is ${total} bytes, over the ${LIVE_IMAGE_LIMITS.file} byte image cap`);
        return null;
      }
      if (!this.charge(bytes.byteLength)) return null;
      const url = blobUrl(bytes, target.mime);
      if (this.disposed) {
        URL.revokeObjectURL(url);
        return null;
      }
      this.urls.push(url);
      return url;
    } catch (error) {
      // A missing or refused file is the document's own broken link; the
      // widget shows its alt text, and the reason goes to the log.
      const reason = error instanceof Error ? error.message : "read_failed";
      this.warn(target.path, `markdown live: image ${target.path} not read: ${reason}`);
      // A busy or absent connection, or a file that is not there yet, may
      // read next time; only a definite refusal is kept.
      if (TRANSIENT.has(reason) || reason === "read_failed") this.loaded.delete(key);
      return null;
    }
  }

  private charge(size: number): boolean {
    if (this.bytes + size > LIVE_IMAGE_LIMITS.bytes) {
      this.warn("bytes", `markdown live: images in one document passed ${LIVE_IMAGE_LIMITS.bytes} bytes, the rest show their alt text`);
      return false;
    }
    this.bytes += size;
    return true;
  }

  private warn(key: string, message: string): void {
    if (this.warned.has(key)) return;
    this.warned.add(key);
    this.log(message);
  }
}
