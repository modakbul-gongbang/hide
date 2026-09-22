// Unsaved editor buffers across a disconnect (PRD B8, D-07). The core owns the
// document, but a daemon restart loses the in-memory draft; the shell keeps the
// newest buffer per path in IndexedDB, and a reconnect reconciles it against
// the core: a buffer for an open document wins over a clean core document, and
// a buffer whose document is gone is discarded with a diagnostic. When
// IndexedDB is unavailable the store degrades to no-op, which is what a test
// runtime without it gets.

export type StoredBuffer = { path: string; contents: string; updated_at: number };

const DATABASE = "hide-shell";
const STORE = "buffers";

function openDatabase(): Promise<IDBDatabase | null> {
  return new Promise((resolve) => {
    if (typeof indexedDB === "undefined") {
      resolve(null);
      return;
    }
    const request = indexedDB.open(DATABASE, 1);
    request.onupgradeneeded = () => {
      const database = request.result;
      if (!database.objectStoreNames.contains(STORE)) database.createObjectStore(STORE, { keyPath: "path" });
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

export async function putBuffer(path: string, contents: string): Promise<void> {
  await withStore("readwrite", (store) => store.put({ path, contents, updated_at: Date.now() }));
}

export async function deleteBuffer(path: string): Promise<void> {
  await withStore("readwrite", (store) => store.delete(path));
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

/** Buffers whose document the core no longer holds: these are discarded. */
export function sweepBuffers(buffers: StoredBuffer[], openPaths: Set<string>): StoredBuffer[] {
  return buffers.filter((buffer) => !openPaths.has(buffer.path));
}
