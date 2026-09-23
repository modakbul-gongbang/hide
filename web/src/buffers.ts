// Unsaved editor buffers across a disconnect (PRD B8, D-07). The core owns the
// document, but a daemon restart loses the in-memory draft; the shell keeps the
// newest buffer per path in IndexedDB, and a reconnect reconciles it against
// the core: a buffer for an open document wins over a clean core document, and
// a buffer whose document is gone is discarded with a diagnostic. When
// IndexedDB is unavailable the store degrades to no-op, which is what a test
// runtime without it gets.

export type StoredBuffer = {
  /** The store's key: identity(root, path). */
  id: string;
  /** The checkout root this path belongs to; the buffer's other half (D-14). */
  root: string;
  path: string;
  contents: string;
  updated_at: number;
};

/** Buffers older than this are discarded on the next start (D-14). */
export const BUFFER_MAX_AGE_MS = 14 * 24 * 60 * 60 * 1000;

const DATABASE = "hide-shell";
const STORE = "buffers";
/** v2 keys a buffer by (checkout root, real path); v1 keyed by path alone. */
const DATABASE_VERSION = 2;

function openDatabase(): Promise<IDBDatabase | null> {
  return new Promise((resolve) => {
    if (typeof indexedDB === "undefined") {
      resolve(null);
      return;
    }
    const request = indexedDB.open(DATABASE, DATABASE_VERSION);
    request.onupgradeneeded = () => {
      const database = request.result;
      // Buffers are ephemeral; a version bump rebuilds the store rather than
      // migrating an old key shape.
      if (database.objectStoreNames.contains(STORE)) database.deleteObjectStore(STORE);
      database.createObjectStore(STORE, { keyPath: "id" });
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => resolve(null);
  });
}

async function withStore<T>(mode: IDBTransactionMode, run: (store: IDBObjectStore) => IDBRequest<T>): Promise<T | null> {
  const database = await openDatabase();
  if (!database) return null;
  return new Promise((resolve) => {
    const transaction = database.transaction(STORE, mode);
    const request = run(transaction.objectStore(STORE));
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => resolve(null);
    transaction.oncomplete = () => database.close();
  });
}

/**
 * Stores one buffer. `false` means the write did not land - IndexedDB is
 * unavailable or the quota is full - so the caller keeps editing and says the
 * buffer lives in this tab only (D-14).
 */
export async function putBuffer(root: string, path: string, contents: string): Promise<boolean> {
  const record = { id: identity(root, path), root, path, contents, updated_at: Date.now() };
  const written = await withStore("readwrite", (store) =>
    store.put(record as unknown as StoredBuffer),
  );
  return written !== null;
}

export async function deleteBuffer(root: string, path: string): Promise<void> {
  await withStore("readwrite", (store) => store.delete(identity(root, path)));
}

export async function allBuffers(): Promise<StoredBuffer[]> {
  const rows = await withStore<StoredBuffer[]>("readonly", (store) => store.getAll() as IDBRequest<StoredBuffer[]>);
  return rows ?? [];
}

/** What a stored buffer means against the document the core reports now. */
export function bufferDecision(buffer: StoredBuffer, document: { contents_utf8: string | null; dirty: boolean } | null): "restore" | "keep" | "drop" {
  if (!document) return "keep";
  if (buffer.contents === (document.contents_utf8 ?? "")) return "drop";
  // The core's copy is the disk version (clean) or an older draft; the
  // operator's newest buffer is the one to show either way.
  return "restore";
}

/** The buffer for one (checkout root, real path) identity, or null. */
export function bufferFor(buffers: StoredBuffer[], root: string, path: string): StoredBuffer | null {
  return buffers.find((buffer) => buffer.root === root && buffer.path === path) ?? null;
}

/** Buffers whose document the core no longer holds: these are discarded. */
export function sweepBuffers(buffers: StoredBuffer[], openIdentities: Set<string>): StoredBuffer[] {
  return buffers.filter((buffer) => !openIdentities.has(identity(buffer.root, buffer.path)));
}

/** The one key a (checkout root, real path) pair is compared by. */
export function identity(root: string, path: string): string {
  return `${root}\u0000${path}`;
}

/** Buffers older than `maxAgeMs`; the next start discards them (D-14). */
export function staleBuffers(buffers: StoredBuffer[], now: number, maxAgeMs = BUFFER_MAX_AGE_MS): StoredBuffer[] {
  return buffers.filter((buffer) => now - buffer.updated_at > maxAgeMs);
}
