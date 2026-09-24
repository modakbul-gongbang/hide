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
const LEGACY_STORE = "buffers";
const STORE = "buffers_v2";
/** v2 keys a buffer by (checkout root, real path); v1 keyed by path alone. */
const DATABASE_VERSION = 3;

function openDatabase(): Promise<IDBDatabase | null> {
  return new Promise((resolve) => {
    if (typeof indexedDB === "undefined") {
      resolve(null);
      return;
    }
    let request: IDBOpenDBRequest;
    try {
      request = indexedDB.open(DATABASE, DATABASE_VERSION);
    } catch {
      resolve(null);
      return;
    }
    let settled = false;
    request.onupgradeneeded = () => {
      const database = request.result;
      // v1 has no checkout root. Keep those unsaved edits until a live core
      // snapshot can identify their open tabs; deleting the store here would
      // lose work during the upgrade from the already shipped S3 shell.
      if (!database.objectStoreNames.contains(STORE)) database.createObjectStore(STORE, { keyPath: "id" });
      // Completion builds also wrote root-keyed rows to the old store at DB
      // version 2. Move those rows in the upgrade transaction; without a
      // version bump, onupgradeneeded would never create the new store.
      if (database.objectStoreNames.contains(LEGACY_STORE)) {
        const source = request.transaction!.objectStore(LEGACY_STORE);
        if (source.keyPath === "id") {
          const read = source.getAll();
          read.onsuccess = () => {
            const destination = request.transaction!.objectStore(STORE);
            for (const row of read.result as StoredBuffer[]) {
              destination.get(row.id).onsuccess = (lookup) => {
                const current = (lookup.target as IDBRequest<StoredBuffer | undefined>).result;
                if (!current || current.updated_at < row.updated_at) destination.put(row);
              };
            }
            database.deleteObjectStore(LEGACY_STORE);
          };
        }
      }
    };
    request.onsuccess = () => {
      if (settled) request.result.close();
      else { settled = true; resolve(request.result); }
    };
    request.onerror = () => { if (!settled) { settled = true; resolve(null); } };
    // Another tab with the old version may hold the upgrade indefinitely.
    // Report a tab-only recovery copy now rather than hanging its write.
    request.onblocked = () => { if (!settled) { settled = true; resolve(null); } };
  });
}

async function withStore<T>(mode: IDBTransactionMode, run: (store: IDBObjectStore) => IDBRequest<T>): Promise<{ ok: boolean; result: T | null }> {
  const database = await openDatabase();
  if (!database) return { ok: false, result: null };
  return new Promise((resolve) => {
    let transaction: IDBTransaction;
    let request: IDBRequest<T>;
    try {
      transaction = database.transaction(STORE, mode);
      request = run(transaction.objectStore(STORE));
    } catch {
      database.close();
      resolve({ ok: false, result: null });
      return;
    }
    let result: T | null = null;
    request.onsuccess = () => { result = request.result; };
    transaction.oncomplete = () => { database.close(); resolve({ ok: true, result }); };
    transaction.onabort = () => { database.close(); resolve({ ok: false, result: null }); };
    transaction.onerror = () => { database.close(); resolve({ ok: false, result: null }); };
  });
}

type LegacyBuffer = { path: string; contents: string; updated_at: number };

/** Claim one v1 path only after the core identifies its checkout root. */
export async function claimLegacyBuffer(root: string, path: string): Promise<void> {
  const database = await openDatabase();
  if (!database) return;
  if (!database.objectStoreNames.contains(LEGACY_STORE)) { database.close(); return; }
  await new Promise<void>((resolve) => {
    let transaction: IDBTransaction;
    try { transaction = database.transaction([LEGACY_STORE, STORE], "readwrite"); }
    catch { database.close(); resolve(); return; }
    const old = transaction.objectStore(LEGACY_STORE);
    const current = transaction.objectStore(STORE);
    old.get(path).onsuccess = (event) => {
      const row = (event.target as IDBRequest<LegacyBuffer | undefined>).result;
      if (!row || !root || !path.startsWith(`${root}/`)) return;
      if (Date.now() - row.updated_at > BUFFER_MAX_AGE_MS) { old.delete(path); return; }
      current.get(identity(root, path)).onsuccess = (lookup) => {
        // A v2 edit is newer than a legacy path-only draft.
        if (!(lookup.target as IDBRequest<StoredBuffer | undefined>).result) {
          current.put({ id: identity(root, path), root, path, contents: row.contents, updated_at: row.updated_at });
        }
        old.delete(path);
      };
    };
    const finish = () => { database.close(); resolve(); };
    transaction.oncomplete = finish;
    transaction.onabort = finish;
    transaction.onerror = finish;
  });
}

/** Drop v1 drafts whose document is no longer open, after claiming open paths. */
export async function discardLegacyBuffers(openPaths: Set<string>): Promise<string[]> {
  const database = await openDatabase();
  if (!database) return [];
  if (!database.objectStoreNames.contains(LEGACY_STORE)) { database.close(); return []; }
  return new Promise((resolve) => {
    const discarded: string[] = [];
    let transaction: IDBTransaction;
    try { transaction = database.transaction(LEGACY_STORE, "readwrite"); }
    catch { database.close(); resolve([]); return; }
    const store = transaction.objectStore(LEGACY_STORE);
    store.getAll().onsuccess = (event) => {
      const rows = (event.target as IDBRequest<LegacyBuffer[]>).result;
      for (const row of rows) {
        if (openPaths.has(row.path) && Date.now() - row.updated_at <= BUFFER_MAX_AGE_MS) continue;
        store.delete(row.path);
        discarded.push(row.path);
      }
    };
    transaction.oncomplete = () => { database.close(); resolve(discarded); };
    transaction.onabort = () => { database.close(); resolve([]); };
    transaction.onerror = () => { database.close(); resolve([]); };
  });
}

/**
 * Stores one buffer. `false` means the write did not land - IndexedDB is
 * unavailable or the quota is full - so the caller keeps editing and says the
 * buffer lives in this tab only (D-14).
 */
/** One active write and one latest pending operation per identity. */
type QueuedOperation =
  | { kind: "put"; record: StoredBuffer; settled?: (ok: boolean | null) => void }
  | { kind: "delete"; settled?: (ok: boolean | null) => void };
type BufferQueue = { active: boolean; activeBytes: number; pending: QueuedOperation | null; timer?: ReturnType<typeof setTimeout>; idleWaiters: Array<() => void> };
const queues = new Map<string, BufferQueue>();
const MAX_BUFFER_QUEUES = 32;
const MAX_BUFFER_QUEUE_BYTES = 128 * 1024 * 1024;
const BUFFER_WRITE_DELAY_MS = 150;
const operationBytes = (operation: QueuedOperation | null) => operation?.kind === "put" ? operation.record.contents.length * 2 : 0;

async function flush(key: string, queue: BufferQueue): Promise<void> {
  if (queue.active) return;
  if (queue.timer !== undefined) clearTimeout(queue.timer);
  queue.timer = undefined;
  const operation = queue.pending;
  if (!operation) {
    queues.delete(key);
    queue.idleWaiters.splice(0).forEach((wake) => wake());
    return;
  }
  queue.pending = null;
  queue.active = true;
  queue.activeBytes = operationBytes(operation);
  let committed = false;
  try {
    const outcome = operation.kind === "put"
      ? await withStore("readwrite", (store) => store.put(operation.record))
      : await withStore("readwrite", (store) => store.delete(key));
    committed = outcome.ok;
  } catch {
    committed = false;
  }
  queue.active = false;
  queue.activeBytes = 0;
  // A newer edit or delete owns the badge. The old result cannot overwrite it.
  operation.settled?.(queue.pending ? null : committed);
  if (queue.pending) void flush(key, queue);
  else {
    queues.delete(key);
    queue.idleWaiters.splice(0).forEach((wake) => wake());
  }
}

function enqueue(key: string, operation: QueuedOperation): boolean {
  let queue = queues.get(key);
  if (!queue) {
    if (queues.size >= MAX_BUFFER_QUEUES) return false;
    queue = { active: false, activeBytes: 0, pending: null, idleWaiters: [] };
    queues.set(key, queue);
  }
  const bytesWithoutOld = [...queues.values()].reduce((sum, item) => sum + item.activeBytes + operationBytes(item.pending), 0) - operationBytes(queue.pending);
  if (bytesWithoutOld + operationBytes(operation) > MAX_BUFFER_QUEUE_BYTES) {
    if (!queue.active && !queue.pending) queues.delete(key);
    return false;
  }
  queue.pending?.settled?.(null);
  queue.pending = operation;
  if (!queue.active && queue.timer === undefined) {
    queue.timer = setTimeout(() => { void flush(key, queue!); }, BUFFER_WRITE_DELAY_MS);
  }
  return true;
}

/** Coalesce keystrokes without growing one transaction or promise per edit. */
export function queueBuffer(root: string, path: string, contents: string, settled: (ok: boolean | null) => void): void {
  const key = identity(root, path);
  const record = { id: key, root, path, contents, updated_at: Date.now() };
  if (!enqueue(key, { kind: "put", record, settled })) settled(false);
}

/** Used for a path move: the old identity is removed only after this commits. */
export function putBuffer(root: string, path: string, contents: string): Promise<boolean> {
  return new Promise((resolve) => {
    const key = identity(root, path);
    const record = { id: key, root, path, contents, updated_at: Date.now() };
    if (!enqueue(key, { kind: "put", record, settled: (ok) => resolve(ok === true) })) resolve(false);
  });
}

export function deleteBuffer(root: string, path: string): Promise<void> {
  return new Promise((resolve) => {
    const key = identity(root, path);
    if (!enqueue(key, { kind: "delete", settled: () => resolve() })) {
      // The cap protects pending drafts, not cleanup. No queue for this key
      // exists here, so a direct committed delete cannot overtake its write.
      void withStore("readwrite", (store) => store.delete(key)).then(() => resolve(), () => resolve());
    }
  });
}

/** A reconnect reads only after this tab's queued recovery copy has landed. */
export function flushBuffer(root: string, path: string): Promise<void> {
  const key = identity(root, path);
  const queue = queues.get(key);
  if (!queue) return Promise.resolve();
  return new Promise((resolve) => {
    queue.idleWaiters.push(resolve);
    if (!queue.active) void flush(key, queue);
  });
}

/** Retarget an unsaved draft in one transaction after its old writes settle. */
export async function moveBuffer(oldRoot: string, oldPath: string, root: string, path: string): Promise<"moved" | "missing" | "failed"> {
  await flushBuffer(oldRoot, oldPath);
  const database = await openDatabase();
  if (!database) return "failed";
  return new Promise((resolve) => {
    let transaction: IDBTransaction;
    try { transaction = database.transaction(STORE, "readwrite"); }
    catch { database.close(); resolve("failed"); return; }
    const store = transaction.objectStore(STORE);
    const oldKey = identity(oldRoot, oldPath);
    const newKey = identity(root, path);
    let found = false;
    store.get(oldKey).onsuccess = (event) => {
      const old = (event.target as IDBRequest<StoredBuffer | undefined>).result;
      if (!old) return;
      found = true;
      store.get(newKey).onsuccess = (lookup) => {
        const current = (lookup.target as IDBRequest<StoredBuffer | undefined>).result;
        // A newer edit at the destination wins. A still-queued destination
        // edit will commit after this transaction and also wins.
        if (!current || current.updated_at < old.updated_at) {
          store.put({ ...old, id: newKey, root, path });
        }
        store.delete(oldKey);
      };
    };
    transaction.oncomplete = () => { database.close(); resolve(found ? "moved" : "missing"); };
    transaction.onabort = () => { database.close(); resolve("failed"); };
    transaction.onerror = () => { database.close(); resolve("failed"); };
  });
}

export async function allBuffers(): Promise<StoredBuffer[]> {
  const rows = await withStore<StoredBuffer[]>("readonly", (store) => store.getAll() as IDBRequest<StoredBuffer[]>);
  return rows.result ?? [];
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
