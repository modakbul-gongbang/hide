// Unsaved editor drafts across a disconnect (PRD S3 B8, S5.5 B9-B12, B44).
// The core owns the document, but a daemon restart loses the in-memory draft;
// the shell keeps the newest draft per document in IndexedDB, and a reconnect
// reconciles it against the core: a draft for an open document wins over a
// clean core document, and a draft whose document is not open stays as a
// recovery item until the operator opens, exports or discards it. Nothing is
// discarded on its own: a draft exists only while it differs from what was
// saved, and a draft of another host or device is not this screen's to judge.
// When IndexedDB is unavailable the store degrades to no-op, which is what a
// test runtime without it gets, and each write says it did not land.

import { checkoutById, deviceOfCheckout, type EditorTabSnapshot, type SnapshotRest } from "./snapshot";

/** Which document a draft belongs to: the daemon host, the device, the checkout root and the real path. */
export type BufferKey = { host: string; device: string; root: string; path: string };

export type StoredBuffer = {
  /** The store's key: identity(key). */
  id: string;
  /**
   * The daemon host and device the draft was written for; null for a draft
   * stored before drafts named them, whose origin is unverified (B11).
   */
  host: string | null;
  device: string | null;
  /** The checkout root; "" for a draft from before drafts named a root. */
  root: string;
  path: string;
  contents: string;
  updated_at: number;
};

const DATABASE = "hide-shell";
/** v1 kept path-only drafts; v2 and v3 keyed them by (root, path). */
const LEGACY_STORES = ["buffers", "buffers_v2"];
const STORE = "drafts_v4";
/** v4 keys a draft by (host, device, root, path). */
const DATABASE_VERSION = 4;
/** Every stored draft together, the browser's quota permitting (D-15, B44). */
export const MAX_STORED_BYTES = 512 * 1024 * 1024;

/**
 * Moves every older draft into the v4 store as an unverified recovery item,
 * inside the upgrade transaction: it commits whole or not at all, so an
 * interrupted upgrade keeps the old stores and runs again on the next open
 * (B12). No draft is attributed to a device here; only an explicit open on
 * this host's own checkout claims one (`claimLegacyBuffer`).
 */
function migrate(database: IDBDatabase, transaction: IDBTransaction) {
  const destination = database.objectStoreNames.contains(STORE)
    ? transaction.objectStore(STORE)
    : database.createObjectStore(STORE, { keyPath: "id" });
  // Every legacy row is read before anything is written, and each draft id
  // is written once with its newest row: queued reads all run before any
  // write, so comparing row by row would let an older row win (B12).
  const legacy = LEGACY_STORES.filter((name) => database.objectStoreNames.contains(name));
  const newest = new Map<string, StoredBuffer>();
  let left = legacy.length;
  for (const name of legacy) {
    transaction.objectStore(name).getAll().onsuccess = (event) => {
      const rows = (event.target as IDBRequest<Array<{ root?: string; path: string; contents: string; updated_at: number }>>).result;
      for (const row of rows) {
        const record = legacyRecord(row.root ?? "", row.path, row.contents, row.updated_at);
        const seen = newest.get(record.id);
        if (!seen || seen.updated_at < record.updated_at) newest.set(record.id, record);
      }
      database.deleteObjectStore(name);
      left -= 1;
      if (left > 0) return;
      for (const record of newest.values()) {
        destination.get(record.id).onsuccess = (lookup) => {
          const current = (lookup.target as IDBRequest<StoredBuffer | undefined>).result;
          if (!current || current.updated_at < record.updated_at) destination.put(record);
        };
      }
    };
  }
}

function legacyRecord(root: string, path: string, contents: string, updated_at: number): StoredBuffer {
  return { id: legacyIdentity(root, path), host: null, device: null, root, path, contents, updated_at };
}

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
    request.onupgradeneeded = () => migrate(request.result, request.transaction!);
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
    // A listener, not `onsuccess`: `run` may own that handler (`putWithinCap`).
    request.addEventListener("success", () => { result = request.result; });
    transaction.oncomplete = () => { database.close(); resolve({ ok: true, result }); };
    transaction.onabort = () => { database.close(); resolve({ ok: false, result: null }); };
    transaction.onerror = () => { database.close(); resolve({ ok: false, result: null }); };
  });
}

/**
 * Claims an unverified draft for a document this host just opened on its own
 * machine, when one was stored for the same checkout root and path (or, from
 * before drafts named a root, the same path). A draft already stored for the
 * document is newer and wins. A device's document never claims one: a draft
 * with no device may have been written for this machine only (B11).
 */
export async function claimLegacyBuffer(key: BufferKey): Promise<void> {
  if (key.device !== "local") return;
  const database = await openDatabase();
  if (!database) return;
  await new Promise<void>((resolve) => {
    let transaction: IDBTransaction;
    try { transaction = database.transaction(STORE, "readwrite"); }
    catch { database.close(); resolve(); return; }
    const store = transaction.objectStore(STORE);
    const target = identity(key);
    store.get(target).onsuccess = (lookup) => {
      if ((lookup.target as IDBRequest<StoredBuffer | undefined>).result) return;
      // Both legacy spellings are read first; only the newest is claimed and
      // only its row is removed, so the other stays a recovery item (B12).
      const ids = [legacyIdentity(key.root, key.path), legacyIdentity("", key.path)];
      const found: StoredBuffer[] = [];
      let answered = 0;
      for (const legacyId of ids) {
        store.get(legacyId).onsuccess = (event) => {
          const row = (event.target as IDBRequest<StoredBuffer | undefined>).result;
          if (row) found.push(row);
          answered += 1;
          if (answered < ids.length || found.length === 0) return;
          const claimed = found.reduce((a, b) => (b.updated_at > a.updated_at ? b : a));
          store.put({ ...claimed, id: target, host: key.host, device: key.device, root: key.root, path: key.path });
          store.delete(claimed.id);
        };
      }
    };
    const finish = () => { database.close(); resolve(); };
    transaction.oncomplete = finish;
    transaction.onabort = finish;
    transaction.onerror = finish;
  });
}

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

/**
 * Writes one draft unless every stored draft together would pass
 * `MAX_STORED_BYTES`. The total is read in the write's own transaction, so
 * the cap holds however many tabs write at once.
 */
function putWithinCap(record: StoredBuffer): Promise<{ ok: boolean; result: StoredBuffer[] | null }> {
  return withStore<StoredBuffer[]>("readwrite", (store) => {
    const all = store.getAll() as IDBRequest<StoredBuffer[]>;
    all.onsuccess = () => {
      const others = all.result.filter((row) => row.id !== record.id).reduce((sum, row) => sum + row.contents.length * 2, 0);
      if (others + record.contents.length * 2 > MAX_STORED_BYTES) all.transaction?.abort();
      else store.put(record);
    };
    return all;
  });
}

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
      ? await putWithinCap(operation.record)
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

function recordFor(key: BufferKey, contents: string): StoredBuffer {
  return { id: identity(key), host: key.host, device: key.device, root: key.root, path: key.path, contents, updated_at: Date.now() };
}

/**
 * Stores one draft, coalescing keystrokes. `false` means the write did not
 * land - IndexedDB is unavailable, the drafts together would pass
 * `MAX_STORED_BYTES`, or the browser's quota is full - so the caller keeps
 * editing and says the draft lives in this tab only (D-14, B44).
 */
export function queueBuffer(key: BufferKey, contents: string, settled: (ok: boolean | null) => void): void {
  const id = identity(key);
  if (!enqueue(id, { kind: "put", record: recordFor(key, contents), settled })) settled(false);
}

export function deleteBuffer(key: BufferKey): Promise<void> {
  return deleteBufferId(identity(key));
}

/** Removes one stored draft by its id: the operator's explicit discard of a recovery item. */
export function deleteBufferId(id: string): Promise<void> {
  return new Promise((resolve) => {
    if (!enqueue(id, { kind: "delete", settled: () => resolve() })) {
      // The cap protects pending drafts, not cleanup. No queue for this key
      // exists here, so a direct committed delete cannot overtake its write.
      void withStore("readwrite", (store) => store.delete(id)).then(() => resolve(), () => resolve());
    }
  });
}

/** A reconnect reads only after this tab's queued recovery copy has landed. */
export function flushBuffer(key: BufferKey): Promise<void> {
  const id = identity(key);
  const queue = queues.get(id);
  if (!queue) return Promise.resolve();
  return new Promise((resolve) => {
    queue.idleWaiters.push(resolve);
    if (!queue.active) void flush(id, queue);
  });
}

/** The stored draft of one document once every write queued for it has landed. */
export async function settledBuffer(key: BufferKey): Promise<StoredBuffer | null> {
  await flushBuffer(key);
  return bufferFor(await allBuffers(), key);
}

/** Retarget an unsaved draft in one transaction after its old writes settle. */
export async function moveBuffer(from: BufferKey, to: BufferKey): Promise<"moved" | "missing" | "failed"> {
  await flushBuffer(from);
  const database = await openDatabase();
  if (!database) return "failed";
  return new Promise((resolve) => {
    let transaction: IDBTransaction;
    try { transaction = database.transaction(STORE, "readwrite"); }
    catch { database.close(); resolve("failed"); return; }
    const store = transaction.objectStore(STORE);
    const oldKey = identity(from);
    const newKey = identity(to);
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
          store.put({ ...old, id: newKey, host: to.host, device: to.device, root: to.root, path: to.path });
        }
        store.delete(oldKey);
      };
    };
    transaction.oncomplete = () => { database.close(); resolve(found ? "moved" : "missing"); };
    transaction.onabort = () => { database.close(); resolve("failed"); };
    transaction.onerror = () => { database.close(); resolve("failed"); };
  });
}

/**
 * What a full draft store means for one editor tab (PRD S5.5 B44). A tab
 * whose draft could not be stored keeps editing, since its only copy is in
 * this tab and saving or exporting it is how the operator gets out; every
 * other clean document is held read-only, so no new edit is made that could
 * not be kept. Nothing stored is evicted to make room, on any device.
 */
/**
 * A stored draft of a tab closed without a save (S5.5 B10-B12, B44). It goes
 * only when it is exactly the text the core holds for that file; anything
 * else, including a background tab whose draft this page never loaded or a
 * document that turned read-only, stays as a recovery item to open, export
 * or discard.
 */
export function storedDraftOnClose(stored: StoredBuffer | null, document: { contents_utf8: string | null; dirty: boolean } | null): "none" | "delete" | "keep" {
  if (!stored) return "none";
  return document && !document.dirty && document.contents_utf8 === stored.contents ? "delete" : "keep";
}

/**
 * What closing one display of a document carries (PRD S7 B5, contract 4.1):
 * the document's unsaved text as the save the close waits on whenever there
 * is any, since the core and not this page's frame decides whether the
 * display is the document's last; nothing for a document with no unsaved
 * work; or `unloaded` for unsaved work this page cannot reproduce (a
 * background document it never loaded), which only the operator's explicit
 * Don't save may drop. The newest keystroke decides, not the last frame's
 * dirty flag, and a document that takes no edits has no text to carry.
 */
export function documentCloseCarries(input: {
  draft: string | null;
  document: { contents_utf8: string | null; dirty: boolean; readonly_reason: string | null; save: unknown } | null;
  dirty: boolean;
}): { kind: "save"; contents: string } | { kind: "clean" } | { kind: "unloaded" } {
  const { draft, document } = input;
  const editable = document ? document.readonly_reason === null && document.contents_utf8 !== null : true;
  const unsaved = input.dirty || (document?.dirty ?? false) || (document?.save ?? null) !== null;
  if (editable && draft !== null) return { kind: "save", contents: draft };
  if (editable && unsaved && document?.contents_utf8 != null) return { kind: "save", contents: document.contents_utf8 };
  return unsaved ? { kind: "unloaded" } : { kind: "clean" };
}

/** A close that carried a save, while this page waits to learn what came of it. */
export type CloseWatch = {
  tabId: string;
  /** The Workspace the close was taken on, by its key, and the display it named there. */
  workspace: string;
  displayId: string;
  hostId: string | null | undefined;
  device: string;
  errorAt: number | null;
  /** The text the close carried. */
  contents: string;
  /** Whether a frame showed the document's save in flight after the close was sent. */
  sawSave: boolean;
};

/** What one frame says about a watched close. */
export type CloseWatchFrame = {
  connection: string;
  hostId: string | null | undefined;
  tabIds: string[];
  deviceIds: string[];
  /** The Workspace in front, by its key, and every display it shows. */
  workspace: string | null;
  displayIds: string[];
  error: { kind: string; occurred_at: number } | null;
  /** The newest text this page holds for the document, or null. */
  draft: string | null;
  /** The document's save as the frame shows it; `hidden` while no area shows the document. */
  save: "in_flight" | "settled" | "hidden";
};

/**
 * What a close that carried a save learns from a frame. The core removes the
 * tab only after that save landed, so a tab gone from the same daemon over a
 * live connection, with its device still registered, is a landed close and
 * its draft goes. A dropped connection, another daemon or a removed device
 * can also take the tab away without the save landing, so the draft is then
 * kept for recovery. While the tab stays, the watch ends as `keep` once
 * nothing more can come of the close - a failure or refusal reported after
 * it, the operator editing the document again, another Workspace in front
 * (a close that arrived after the front moved is only logged), its display
 * gone without the document (the core took it as not the last), or its
 * save settled - so the document's autosave and save on leaving come back
 * (interface-2).
 */
export function closeWithSaveOutcome(watch: CloseWatch, next: CloseWatchFrame): { outcome: "wait" | "landed" | "keep"; sawSave: boolean } {
  const keep = { outcome: "keep" as const, sawSave: watch.sawSave };
  if (next.connection !== "live" || next.hostId !== watch.hostId) return keep;
  if (!next.tabIds.includes(watch.tabId)) {
    if (watch.device !== "local" && !next.deviceIds.includes(watch.device)) return keep;
    return { outcome: "landed", sawSave: watch.sawSave };
  }
  // Another document's failure read this way only keeps a draft, never loses one.
  const failed =
    next.error !== null &&
    next.error.occurred_at !== watch.errorAt &&
    (next.error.kind.startsWith("file.save_") || next.error.kind.startsWith("view_layout."));
  if (failed) return keep;
  if (next.draft !== null && next.draft !== watch.contents) return keep;
  if (next.workspace !== watch.workspace || !next.displayIds.includes(watch.displayId)) return keep;
  if (next.save === "in_flight") return { outcome: "wait", sawSave: true };
  if (next.save === "settled" && watch.sawSave) return keep;
  return { outcome: "wait", sawSave: watch.sawSave };
}

export function draftStorageHold(input: { storageFull: boolean; unstored: boolean; dirty: boolean }): "unstored" | "held" | null {
  if (input.unstored) return "unstored";
  if (input.storageFull && !input.dirty) return "held";
  return null;
}

export async function allBuffers(): Promise<StoredBuffer[]> {
  const rows = await withStore<StoredBuffer[]>("readonly", (store) => store.getAll() as IDBRequest<StoredBuffer[]>);
  return rows.result ?? [];
}

/** What a stored draft means against the document the core reports now. */
export function bufferDecision(buffer: StoredBuffer, document: { contents_utf8: string | null; dirty: boolean } | null): "restore" | "keep" | "drop" {
  if (!document) return "keep";
  // The same contents in a dirty core is the core holding this draft in
  // memory, not on disk: a refused or pending save. The stored copy is the
  // only one a restart keeps, so it stays until the core reports clean.
  if (buffer.contents === (document.contents_utf8 ?? "")) return document.dirty ? "keep" : "drop";
  // The core's copy is the disk version (clean) or an older draft; the
  // operator's newest draft is the one to show either way.
  return "restore";
}

/** The draft stored for one document, or null. */
export function bufferFor(buffers: StoredBuffer[], key: BufferKey): StoredBuffer | null {
  const id = identity(key);
  return buffers.find((buffer) => buffer.id === id) ?? null;
}

/**
 * Drafts no open tab stands for: kept as recovery items, never discarded on
 * their own (B10-B12). A draft of another host or device, or one whose
 * origin is unverified, is among them.
 */
export function recoveryBuffers(buffers: StoredBuffer[], openIdentities: Set<string>): StoredBuffer[] {
  return buffers.filter((buffer) => !openIdentities.has(buffer.id)).sort((a, b) => b.updated_at - a.updated_at);
}

/** The one key a document's draft is stored and compared by. */
export function identity(key: BufferKey): string {
  return [key.host, key.device, key.root, key.path].join("\u0000");
}

function legacyIdentity(root: string, path: string): string {
  return ["", "", root, path].join("\u0000");
}

/**
 * The draft key of an open file tab: this daemon host, the device of the
 * tab's checkout, its root and path. Null until the daemon has named itself
 * or while the checkout is unknown, when no draft can be filed safely.
 */
export function tabBufferKey(host: string | null | undefined, rest: SnapshotRest | null, tab: Pick<EditorTabSnapshot, "checkout_id" | "path">): BufferKey | null {
  const root = checkoutById(rest, tab.checkout_id)?.path;
  if (!host || !root) return null;
  return { host, device: deviceOfCheckout(rest, tab.checkout_id), root, path: tab.path };
}
