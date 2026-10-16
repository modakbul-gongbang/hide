import { describe, expect, it } from "vitest";
import { carriesOsFiles, dropFailure, listDropped, type CapturedDrop, type DroppedItem } from "./explorerDrop";
import { DROP_MAX_BYTES, DROP_MAX_FILE_BYTES, DROP_MAX_ITEMS } from "./explorerDropText";

const file = (name: string, size: number): DroppedItem => ({ path: name, directory: false, file: { size } as File });

describe("the caps of a drop from the file manager", () => {
  it("passes a drop within every cap", () => {
    expect(dropFailure([file("a.txt", 1), { path: "dir", directory: true, file: null }, file("dir/b.txt", DROP_MAX_FILE_BYTES)])).toBeNull();
  });

  it("refuses by item count, by one file and by the whole drop, from sizes alone", () => {
    expect(dropFailure(Array.from({ length: DROP_MAX_ITEMS + 1 }, (_, n) => file(`f${n}`, 0)))).toBe("too_many");
    expect(dropFailure([file("big.bin", DROP_MAX_FILE_BYTES + 1)])).toBe("too_large");
    expect(dropFailure([file("a", DROP_MAX_FILE_BYTES), file("b", DROP_MAX_FILE_BYTES), file("c", 1)])).toBe("batch_too_large");
    expect(DROP_MAX_BYTES).toBe(2 * DROP_MAX_FILE_BYTES);
  });
});

describe("what a drag carries", () => {
  it("is the file manager's only when the types name Files", () => {
    expect(carriesOsFiles({ types: ["Files"] })).toBe(true);
    expect(carriesOsFiles({ types: ["text/plain"] })).toBe(false);
    expect(carriesOsFiles(null)).toBe(false);
  });
});

/** A directory entry whose children arrive in batches, as a real reader hands them out. */
function directory(name: string, children: FileSystemEntry[]): FileSystemDirectoryEntry {
  return {
    isDirectory: true,
    isFile: false,
    name,
    createReader: () => {
      const rest = [...children];
      return { readEntries: (done: (entries: FileSystemEntry[]) => void) => done(rest.splice(0, 2)) };
    },
  } as unknown as FileSystemDirectoryEntry;
}

function fileEntry(name: string, size: number): FileSystemFileEntry {
  return { isDirectory: false, isFile: true, name, file: (done: (file: File) => void) => done({ name, size } as File) } as unknown as FileSystemFileEntry;
}

describe("walking a dropped folder", () => {
  it("lists every folder and file below it with its path under the destination", async () => {
    const captured: CapturedDrop = {
      entries: [directory("photos", [fileEntry("one.png", 3), directory("raw", [fileEntry("two.png", 4)]), fileEntry("three.png", 5)]), fileEntry("note.txt", 1)],
      files: [],
    };
    const listed = await listDropped(captured);
    expect(Array.isArray(listed) && listed.map((item) => `${item.directory ? "d" : "f"}:${item.path}`)).toEqual([
      "d:photos",
      "f:photos/one.png",
      "d:photos/raw",
      "f:photos/raw/two.png",
      "f:photos/three.png",
      "f:note.txt",
    ]);
  });

  it("stops reading a folder once it holds more than the cap and reports it", async () => {
    const many = Array.from({ length: DROP_MAX_ITEMS + 5 }, (_, n) => fileEntry(`f${n}`, 0));
    expect(await listDropped({ entries: [directory("many", many)], files: [] })).toBe("too_many");
  });

  it("reports an unreadable entry and an empty drop", async () => {
    const broken = { isDirectory: false, isFile: true, name: "x", file: (_: unknown, fail: (error: Error) => void) => fail(new Error("gone")) } as unknown as FileSystemFileEntry;
    expect(await listDropped({ entries: [broken], files: [] })).toBe("unreadable");
    expect(await listDropped({ entries: [], files: [] })).toBe("unreadable");
  });
});
