import { describe, expect, it } from "vitest";
import { bufferDecision, bufferFor, closeWithSaveOutcome, draftStorageHold, storedDraftOnClose, identity, recoveryBuffers, tabBufferKey, type BufferKey, type StoredBuffer } from "./buffers";
import { draftPlace } from "./DraftRecovery";
import type { SnapshotRest } from "./snapshot";

const HOST = "host-a";
const KEY: BufferKey = { host: HOST, device: "local", root: "/repo", path: "/repo/a.ts" };

function buffer(key: BufferKey, contents: string, updatedAt = 0): StoredBuffer {
  return { id: identity(key), host: key.host, device: key.device, root: key.root, path: key.path, contents, updated_at: updatedAt };
}

function legacy(root: string, path: string): StoredBuffer {
  return { id: ["", "", root, path].join("\u0000"), host: null, device: null, root, path, contents: "old", updated_at: 1 };
}

function rest(focusedDevice = "local"): SnapshotRest {
  const checkout = (workspace: string, id: string, path: string) => ({ id, workspace_id: workspace, label: id, path, tabs: [], strip: [] });
  return {
    navigator: {
      focused_device_id: focusedDevice,
      focused_workspace_id: "w-local",
      focused_checkout_id: "c-local",
      devices: [{ id: "studio", label: "Studio", kind: "remote" }],
      workspaces: [{ id: "w-local", device_id: "local", checkouts: [checkout("w-local", "c-local", "/repo")] }],
    },
    status: {
      remote: [
        {
          target_id: "studio",
          state: "connected",
          session: {
            workspaces: [{ id: "remote:studio:project:p", device_id: "studio", checkouts: [checkout("remote:studio:project:p", "remote:studio:checkout:w1", "/repo")] }],
            focused_workspace_id: "remote:studio:project:p",
            focused_checkout_id: "remote:studio:checkout:w1",
          },
        },
      ],
    },
  } as unknown as SnapshotRest;
}

describe("draft reconciliation", () => {
  it("restores the draft over a core copy that differs", () => {
    expect(bufferDecision(buffer(KEY, "edited"), { contents_utf8: "disk", dirty: false })).toBe("restore");
    expect(bufferDecision(buffer(KEY, "edited"), { contents_utf8: "older", dirty: true })).toBe("restore");
  });

  it("drops the draft only when the core holds the same contents saved", () => {
    expect(bufferDecision(buffer(KEY, "same"), { contents_utf8: "same", dirty: false })).toBe("drop");
    expect(bufferDecision(buffer(KEY, ""), { contents_utf8: null, dirty: false })).toBe("drop");
  });

  it("keeps the draft the core holds only in memory, as after a refused save", () => {
    expect(bufferDecision(buffer(KEY, "same"), { contents_utf8: "same", dirty: true })).toBe("keep");
  });

  it("keeps a draft for a document the editor is not showing", () => {
    expect(bufferDecision(buffer(KEY, "x"), null)).toBe("keep");
  });
});

describe("a full draft store (B44)", () => {
  it("keeps the unstored tab editable and holds every other clean document", () => {
    expect(draftStorageHold({ storageFull: true, unstored: true, dirty: true })).toBe("unstored");
    expect(draftStorageHold({ storageFull: true, unstored: false, dirty: false })).toBe("held");
    expect(draftStorageHold({ storageFull: true, unstored: false, dirty: true })).toBeNull();
    expect(draftStorageHold({ storageFull: false, unstored: false, dirty: false })).toBeNull();
  });
});

describe("draft identity", () => {
  it("keeps the same path on two devices and two hosts apart (B2, B9)", () => {
    const studio = { ...KEY, device: "studio" };
    const otherHost = { ...KEY, host: "host-b" };
    const drafts = [buffer(KEY, "local"), buffer(studio, "studio"), buffer(otherHost, "other host")];
    expect(bufferFor(drafts, KEY)?.contents).toBe("local");
    expect(bufferFor(drafts, studio)?.contents).toBe("studio");
    expect(bufferFor(drafts, otherHost)?.contents).toBe("other host");
    expect(new Set(drafts.map((row) => row.id)).size).toBe(3);
  });

  it("files a tab's draft under this host and the device of its checkout", () => {
    expect(tabBufferKey(HOST, rest(), { checkout_id: "remote:studio:checkout:w1", path: "/repo/a.ts" })).toEqual({ ...KEY, device: "studio" });
    expect(tabBufferKey(HOST, rest(), { checkout_id: "c-local", path: "/repo/a.ts" })).toEqual(KEY);
    expect(tabBufferKey(null, rest(), { checkout_id: "c-local", path: "/repo/a.ts" })).toBeNull();
    expect(tabBufferKey(HOST, rest(), { checkout_id: "gone", path: "/repo/a.ts" })).toBeNull();
  });
});

describe("draft recovery (B10-B12)", () => {
  it("keeps every draft no open tab stands for, whatever its age or origin", () => {
    const ancient = buffer({ ...KEY, path: "/repo/old.ts" }, "x", 1);
    const studio = buffer({ ...KEY, device: "studio" }, "y", 2);
    const unverified = legacy("/repo", "/repo/a.ts");
    const open = new Set([identity(KEY)]);
    const kept = recoveryBuffers([buffer(KEY, "open"), ancient, studio, unverified], open);
    expect(kept.map((row) => row.id).sort()).toEqual([ancient.id, studio.id, unverified.id].sort());
  });

  it("opens a draft only in its own checkout on its own device and host", () => {
    expect(draftPlace(buffer(KEY, "x"), HOST, rest())).toEqual({ kind: "front" });
    expect(draftPlace(buffer({ ...KEY, device: "studio" }, "x"), HOST, rest())).toMatchObject({ kind: "device", device: "studio" });
    expect(draftPlace(buffer({ ...KEY, device: "studio" }, "x"), HOST, rest("studio"))).toEqual({ kind: "front" });
    expect(draftPlace(buffer({ ...KEY, host: "host-b" }, "x"), HOST, rest())).toMatchObject({ kind: "none" });
    expect(draftPlace(buffer({ ...KEY, root: "/elsewhere", path: "/elsewhere/a.ts" }, "x"), HOST, rest())).toMatchObject({ kind: "none" });
  });

  it("offers an unverified draft only to this machine's own checkout at its root", () => {
    expect(draftPlace(legacy("/repo", "/repo/a.ts"), HOST, rest())).toEqual({ kind: "front" });
    expect(draftPlace(legacy("/repo", "/repo/a.ts"), HOST, rest("studio"))).toMatchObject({ kind: "device", device: "local" });
    expect(draftPlace(legacy("", "/repo/a.ts"), HOST, rest())).toMatchObject({ kind: "none" });
  });

  it("is a no-op without IndexedDB, and says a write did not land", async () => {
    const { allBuffers, deleteBuffer, queueBuffer } = await import("./buffers");
    const landed = await new Promise<boolean | null>((resolve) => queueBuffer(KEY, "x", resolve));
    expect(landed).toBe(false);
    await deleteBuffer(KEY);
    await expect(allBuffers()).resolves.toEqual([]);
  });
});

describe("a stored draft when its tab closes (S5.5 B10-B12, B44)", () => {
  it("goes only when it is exactly the clean text the core holds", () => {
    const stored = buffer(KEY, "draft");
    expect(storedDraftOnClose(null, null)).toBe("none");
    expect(storedDraftOnClose(stored, { contents_utf8: "draft", dirty: false })).toBe("delete");
    // A background tab this page never showed, a changed file, a read-only document.
    expect(storedDraftOnClose(stored, null)).toBe("keep");
    expect(storedDraftOnClose(stored, { contents_utf8: "disk", dirty: false })).toBe("keep");
    expect(storedDraftOnClose(stored, { contents_utf8: null, dirty: false })).toBe("keep");
  });

  it("goes after a close with a save only when that close landed", () => {
    const watch = { tabId: "t", hostId: "host-a", device: "mac" };
    const next = { connection: "live", hostId: "host-a", tabIds: [] as string[], deviceIds: ["mac"] };
    expect(closeWithSaveOutcome(watch, { ...next, tabIds: ["t"] })).toBe("wait");
    expect(closeWithSaveOutcome(watch, next)).toBe("landed");
    expect(closeWithSaveOutcome(watch, { ...next, deviceIds: [] })).toBe("keep");
    expect(closeWithSaveOutcome(watch, { ...next, connection: "reconnecting" })).toBe("keep");
    expect(closeWithSaveOutcome(watch, { ...next, hostId: "host-b" })).toBe("keep");
    expect(closeWithSaveOutcome({ ...watch, device: "local" }, { ...next, deviceIds: [] })).toBe("landed");
  });
});
