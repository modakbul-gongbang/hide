import { describe, expect, it } from "vitest";
import { bufferDecision, sweepBuffers, type StoredBuffer } from "./buffers";

function buffer(path: string, contents: string): StoredBuffer {
  return { path, contents, updated_at: 0 };
}

describe("buffer reconciliation", () => {
  it("restores the buffer over a core copy that differs", () => {
    expect(bufferDecision(buffer("/a.ts", "edited"), { contents_utf8: "disk", dirty: false })).toBe("restore");
    expect(bufferDecision(buffer("/a.ts", "edited"), { contents_utf8: "older", dirty: true })).toBe("restore");
  });

  it("drops the buffer when the core already holds the same contents", () => {
    expect(bufferDecision(buffer("/a.ts", "same"), { contents_utf8: "same", dirty: true })).toBe("drop");
    expect(bufferDecision(buffer("/a.ts", ""), { contents_utf8: null, dirty: false })).toBe("drop");
  });

  it("keeps a buffer for a document the editor is not showing", () => {
    expect(bufferDecision(buffer("/a.ts", "x"), null)).toBe("keep");
  });

  it("sweeps only the buffers whose document is gone", () => {
    const buffers = [buffer("/a.ts", "1"), buffer("/b.ts", "2")];
    expect(sweepBuffers(buffers, new Set(["/a.ts"])).map((row) => row.path)).toEqual(["/b.ts"]);
  });

  it("is a no-op without IndexedDB", async () => {
    const { allBuffers, deleteBuffer, putBuffer } = await import("./buffers");
    await putBuffer("/a", "x");
    await deleteBuffer("/a");
    await expect(allBuffers()).resolves.toEqual([]);
  });
});
