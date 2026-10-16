import { describe, expect, it, vi } from "vitest";
import { LIVE_IMAGE_LIMITS, LiveImages, type FileRead } from "./liveImages";

vi.stubGlobal("URL", Object.assign(URL, { createObjectURL: vi.fn((blob: Blob) => `blob:${blob.size}`), revokeObjectURL: vi.fn() }));

const file = (path: string) => ({ kind: "file" as const, path, mime: "image/png" });

function images(read: FileRead) {
  const log: string[] = [];
  return { cache: new LiveImages(read, (message) => log.push(message)), log };
}

describe("LiveImages", () => {
  it("reads a file once however many times it is drawn", async () => {
    const read = vi.fn<FileRead>(async () => ({ bytes: new Uint8Array(10), total: 10 }));
    const { cache } = images(read);
    const first = await cache.load(file("/r/a.png"), null);
    const second = await cache.load(file("/r/a.png"), null);
    expect(first).toBe("blob:10");
    expect(second).toBe(first);
    expect(read).toHaveBeenCalledTimes(1);
  });

  it("answers null and logs when a file does not read", async () => {
    const { cache, log } = images(async () => {
      throw new Error("outside_checkout");
    });
    expect(await cache.load(file("/r/gone.png"), null)).toBeNull();
    expect(log).toEqual(["markdown live: image /r/gone.png not read: outside_checkout"]);
  });

  it("refuses a file over the per-image cap, on either side of it", async () => {
    const at = images(async () => ({ bytes: new Uint8Array(4), total: LIVE_IMAGE_LIMITS.file }));
    expect(await at.cache.load(file("/r/at.png"), null)).not.toBeNull();
    const over = images(async () => ({ bytes: new Uint8Array(4), total: LIVE_IMAGE_LIMITS.file + 1 }));
    expect(await over.cache.load(file("/r/over.png"), null)).toBeNull();
    expect(over.log).toHaveLength(1);
  });

  it("stops loading images past the document's byte cap and logs the crossing once", async () => {
    const { cache, log } = images(async () => ({ bytes: new Uint8Array(LIVE_IMAGE_LIMITS.file), total: LIVE_IMAGE_LIMITS.file }));
    const results = [];
    for (let i = 0; i < 6; i += 1) results.push(await cache.load(file(`/r/${i}.png`), null));
    expect(results.filter((url) => url !== null)).toHaveLength(LIVE_IMAGE_LIMITS.bytes / LIVE_IMAGE_LIMITS.file);
    expect(log.filter((line) => line.includes("passed"))).toHaveLength(1);
  });

  it("stops at the image count cap and passes web addresses through", async () => {
    const { cache, log } = images(async () => ({ bytes: new Uint8Array(1), total: 1 }));
    for (let i = 0; i < LIVE_IMAGE_LIMITS.images; i += 1) await cache.load({ kind: "remote", url: `https://x/${i}.png` }, null);
    expect(await cache.load({ kind: "remote", url: "https://x/one-more.png" }, null)).toBeNull();
    expect(await cache.load({ kind: "remote", url: "https://x/0.png" }, null)).toBe("https://x/0.png");
    expect(log).toHaveLength(1);
  });

  it("revokes what it made when the view ends, and a late read makes nothing", async () => {
    let release: () => void = () => {};
    const gate = new Promise<void>((resolve) => (release = resolve));
    const { cache } = images(async () => {
      await gate;
      return { bytes: new Uint8Array(1), total: 1 };
    });
    const late = cache.load(file("/r/late.png"), null);
    cache.dispose();
    release();
    expect(await late).toBeNull();
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:1");
  });

  it("revokes every URL it made when it is disposed", async () => {
    const { cache } = images(async () => ({ bytes: new Uint8Array(7), total: 7 }));
    await cache.load(file("/r/a.png"), null);
    vi.mocked(URL.revokeObjectURL).mockClear();
    cache.dispose();
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:7");
  });

  it("holds one file read at a time", async () => {
    let running = 0;
    let most = 0;
    const { cache } = images(async () => {
      running += 1;
      most = Math.max(most, running);
      await new Promise((resolve) => setTimeout(resolve, 5));
      running -= 1;
      return { bytes: new Uint8Array(1), total: 1 };
    });
    await Promise.all(["a", "b", "c", "d"].map((name) => cache.load(file(`/r/${name}.png`), null)));
    expect(most).toBe(1);
  });

  it("reads again after a read that a busy connection ended, and keeps a definite refusal", async () => {
    let reason = "superseded";
    const read = vi.fn<FileRead>(async () => {
      throw new Error(reason);
    });
    const { cache } = images(read);
    expect(await cache.load(file("/r/a.png"), null)).toBeNull();
    expect(await cache.load(file("/r/a.png"), null)).toBeNull();
    expect(read).toHaveBeenCalledTimes(2);
    reason = "outside_checkout";
    await cache.load(file("/r/b.png"), null);
    await cache.load(file("/r/b.png"), null);
    expect(read).toHaveBeenCalledTimes(3);
  });
});
