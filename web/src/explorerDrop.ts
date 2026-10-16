// Files and folders dragged into the Explorer from the operator's file
// manager (docs/UI_BEHAVIOR.md, Explorer). The page reads the bytes through the
// File API - it cannot name a path, and a path outside every registered
// checkout never reaches the core (AGENTS.md) - uploads each file to hided's
// staging as an attachment is, then sends one `path_import` naming the staged
// files. Hided turns those ids into the core's own event and the checkout's
// host writes the items, on this machine or on a device alike. The caps here
// are the ones hided and the host enforce, checked before a byte leaves the
// page; crossing one is a reported failure and nothing is added.

import { DROP_MAX_BYTES, DROP_MAX_FILE_BYTES, DROP_MAX_ITEMS, dropFailureText, type DropFailure } from "./explorerDropText";
import { rememberDrop, safeName, sendPathImport, stageFile } from "./attachments";
import { useUiStore } from "./ui";

/** One file or folder of a drop, below the folder it lands in. */
export type DroppedItem = { path: string; directory: boolean; file: File | null };

/** What a drop event carried, taken while the event is still running: the entries die when the handler returns. */
export type CapturedDrop = { entries: FileSystemEntry[]; files: File[] };

/** Whether the drag carries files from outside the page. */
export function carriesOsFiles(data: Pick<DataTransfer, "types"> | null): boolean {
  return data !== null && Array.from(data.types).includes("Files");
}

/** Takes the entries and files out of a drop event; call it synchronously from the handler. */
export function captureDrop(data: DataTransfer): CapturedDrop {
  const entries: FileSystemEntry[] = [];
  for (const item of Array.from(data.items)) {
    if (item.kind !== "file") continue;
    const entry = item.webkitGetAsEntry();
    if (entry) entries.push(entry);
  }
  return { entries, files: entries.length === 0 ? Array.from(data.files) : [] };
}

const entryFile = (entry: FileSystemFileEntry) => new Promise<File>((resolve, reject) => entry.file(resolve, reject));

async function childrenOf(entry: FileSystemDirectoryEntry): Promise<FileSystemEntry[]> {
  const reader = entry.createReader();
  const all: FileSystemEntry[] = [];
  // A reader hands out its entries in batches and ends with an empty one.
  for (;;) {
    const batch = await new Promise<FileSystemEntry[]>((resolve, reject) => reader.readEntries(resolve, reject));
    if (batch.length === 0) return all;
    all.push(...batch);
    if (all.length > DROP_MAX_ITEMS) return all;
  }
}

/**
 * The items a drop holds, folders walked to their files. The walk stops as
 * soon as the item cap is crossed, so a dropped disk is refused without being
 * read; file bytes are not touched here.
 */
export async function listDropped(captured: CapturedDrop): Promise<DroppedItem[] | DropFailure> {
  const items: DroppedItem[] = [];
  const walk = async (entry: FileSystemEntry, path: string): Promise<boolean> => {
    if (items.length >= DROP_MAX_ITEMS) return false;
    if (entry.isDirectory) {
      items.push({ path, directory: true, file: null });
      for (const child of await childrenOf(entry as FileSystemDirectoryEntry)) {
        if (!(await walk(child, `${path}/${child.name}`))) return false;
      }
      return true;
    }
    items.push({ path, directory: false, file: await entryFile(entry as FileSystemFileEntry) });
    return true;
  };
  try {
    for (const entry of captured.entries) {
      if (!(await walk(entry, entry.name))) return "too_many";
    }
    for (const file of captured.files) {
      if (items.length >= DROP_MAX_ITEMS) return "too_many";
      items.push({ path: file.name, directory: false, file });
    }
  } catch {
    return "unreadable";
  }
  return items.length === 0 ? "unreadable" : (dropFailure(items) ?? items);
}

/** The cap a drop crosses, read from the item count and the file sizes alone, or null. */
export function dropFailure(items: readonly DroppedItem[]): DropFailure | null {
  if (items.length > DROP_MAX_ITEMS) return "too_many";
  let total = 0;
  for (const item of items) {
    const size = item.file?.size ?? 0;
    if (size > DROP_MAX_FILE_BYTES) return "too_large";
    total += size;
  }
  return total > DROP_MAX_BYTES ? "batch_too_large" : null;
}

/**
 * Reads, uploads and commits a drop into `destination`. A failure at any step
 * before the commit leaves nothing behind: the bytes are staged by hided only,
 * and the host writes the items whole or not at all.
 */
export async function submitDrop(target: { root: string; device_id?: string }, destination: string, captured: CapturedDrop): Promise<void> {
  const ui = useUiStore.getState();
  const fail = (message: string) => useUiStore.getState().setExplorerNotice({ path: destination, message });
  ui.setExplorerNotice(null);
  const listed = await listDropped(captured);
  if (typeof listed === "string") return fail(dropFailureText(listed));
  const batch = crypto.randomUUID();
  const staged: { path: string; directory: boolean; stage?: string }[] = [];
  const uploads: { stage: string; name: string; bytes: Uint8Array }[] = [];
  try {
    for (const [index, item] of listed.entries()) {
      if (item.directory || !item.file) {
        staged.push({ path: item.path, directory: true });
        continue;
      }
      const stage = `${batch}-${index}`;
      uploads.push({ stage, name: safeName(item.file.name), bytes: new Uint8Array(await item.file.arrayBuffer()) });
      staged.push({ path: item.path, directory: false, stage });
    }
  } catch {
    return fail(dropFailureText("unreadable"));
  }
  rememberDrop(batch, destination);
  for (const upload of uploads) {
    if (!stageFile(upload.stage, { name: upload.name, bytes: upload.bytes })) return fail(dropFailureText("failed"));
  }
  if (!sendPathImport({ request_id: batch, ...target, destination, items: staged })) fail(dropFailureText("failed"));
}
