import { describe, expect, it } from "vitest";
import { bufferDecision, bufferFor, identity, staleBuffers, sweepBuffers, type StoredBuffer } from "./buffers";

const ROOT = "/repo";

function buffer(path: string, contents: string, updatedAt = 0): StoredBuffer {
  return { id: identity(ROOT, path), root: ROOT, path, contents, updated_at: updatedAt };
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
    const open = new Set([identity(ROOT, "/a.ts")]);
    expect(sweepBuffers(buffers, open).map((row) => row.path)).toEqual(["/b.ts"]);
  });

  it("keys a buffer by its checkout root and real path", () => {
    const buffers = [buffer("/a.ts", "1")];
    expect(bufferFor(buffers, ROOT, "/a.ts")?.contents).toBe("1");
    expect(bufferFor(buffers, "/other", "/a.ts")).toBeNull();
  });

  it("discards a buffer nobody claimed for two weeks", () => {
    const now = Date.now();
    const fresh = buffer("/a.ts", "1", now - 1000);
    const stale = buffer("/b.ts", "2", now - 15 * 24 * 60 * 60 * 1000);
    expect(staleBuffers([fresh, stale], now).map((row) => row.path)).toEqual(["/b.ts"]);
  });

  it("is a no-op without IndexedDB", async () => {
    const { allBuffers, deleteBuffer, putBuffer } = await import("./buffers");
    await putBuffer("/repo", "/a", "x");
    await deleteBuffer("/repo", "/a");
    await expect(allBuffers()).resolves.toEqual([]);
  });
});
