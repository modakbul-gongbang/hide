import { describe, expect, it, vi } from "vitest";
import { clearPending, configureFileBytes, downloadFile, receiveBytes, receiveBytesError, receiveBytesRefusal, requestFileBytes } from "./fileBytes";

/** The frame hided sends: 4-byte big-endian header length, header JSON, bytes. */
function frame(header: Record<string, unknown>, bytes: number[] | Uint8Array): ArrayBuffer {
  const json = new TextEncoder().encode(JSON.stringify(header));
  const out = new Uint8Array(4 + json.length + bytes.length);
  new DataView(out.buffer).setUint32(0, json.length, false);
  out.set(json, 4);
  out.set(bytes, 4 + json.length);
  return out.buffer;
}

/** Captures the request id of the last `file_bytes` event dispatched. */
function captureDispatch(): { events: Record<string, unknown>[] } {
  const events: Record<string, unknown>[] = [];
  configureFileBytes((event) => {
    events.push(event.payload);
  });
  return { events };
}

describe("file bytes", () => {
  it("resolves a request once the eof frame arrives, joining its chunks", async () => {
    const { events } = captureDispatch();
    const promise = requestFileBytes("/repo/shot.png");
    const requestId = events[0]!.request_id as string;
    expect(events[0]).toMatchObject({ path: "/repo/shot.png", offset: 0, length: null });

    receiveBytes(frame({ type: "file_bytes", request_id: requestId, offset: 0, total: 5, eof: false }, [1, 2]));
    receiveBytes(frame({ type: "file_bytes", request_id: requestId, offset: 2, total: 5, eof: true }, [3, 4, 5]));
    await expect(promise).resolves.toEqual(new Uint8Array([1, 2, 3, 4, 5]));
  });

  it("carries a range request's offset and length", async () => {
    const { events } = captureDispatch();
    const pending = requestFileBytes("/repo/clip.mp4", { offset: 16, length: 8 });
    expect(events[0]).toMatchObject({ path: "/repo/clip.mp4", offset: 16, length: 8 });
    // This read is never answered; the next test's cleanup must not surface it
    // as an unhandled rejection.
    pending.catch(() => {});
    clearPending("test_cleanup");
    await expect(pending).rejects.toThrow("test_cleanup");
  });

  it("settles every read of a refused path with the daemon's reason", async () => {
    const { events } = captureDispatch();
    const first = requestFileBytes("/outside/secret.bin");
    const second = requestFileBytes("/outside/secret.bin");
    expect(events).toHaveLength(2);
    receiveBytesRefusal("/outside/secret.bin", "outside_checkout");
    await expect(first).rejects.toThrow("outside_checkout");
    await expect(second).rejects.toThrow("outside_checkout");
  });

  it("fails every in-flight read when the socket goes", async () => {
    const { events } = captureDispatch();
    const promise = requestFileBytes("/repo/shot.png");
    expect(events).toHaveLength(1);
    clearPending("socket_closed");
    await expect(promise).rejects.toThrow("socket_closed");
  });

  it("rejects with the daemon's reason and ignores a frame for an unknown request", async () => {
    const { events } = captureDispatch();
    const promise = requestFileBytes("/outside/secret.bin");
    const requestId = events[0]!.request_id as string;
    receiveBytesError({ request_id: requestId, reason: "outside_checkout" });
    await expect(promise).rejects.toThrow("outside_checkout");
    expect(() => receiveBytes(frame({ type: "file_bytes", request_id: "nobody", offset: 0, total: 1, eof: true }, [9]))).not.toThrow();
  });

  it("streams a remote download in bounded range requests into the chosen file", async () => {
    const range = 4 * 1024 * 1024;
    const written: number[] = [];
    const close = vi.fn(async () => {});
    const abort = vi.fn(async () => {});
    const showSaveFilePicker = vi.fn(async () => ({
      createWritable: async () => ({
        write: async (chunk: Uint8Array) => { written.push(chunk.byteLength); },
        close,
        abort,
      }),
    }));
    vi.stubGlobal("window", { showSaveFilePicker });
    const offsets: number[] = [];
    configureFileBytes((event) => {
      const payload = event.payload as { request_id: string; offset: number; path: string };
      offsets.push(payload.offset);
      const bytes = new Uint8Array(payload.offset === 0 ? range : 3);
      queueMicrotask(() => receiveBytes(frame({
        type: "file_bytes", request_id: payload.request_id, path: payload.path,
        offset: payload.offset, total: range + 3, eof: true,
      }, bytes)));
    });
    try {
      await downloadFile("/repo/large.txt");
      expect(showSaveFilePicker).toHaveBeenCalledWith({ suggestedName: "large.txt" });
      expect(offsets).toEqual([0, range]);
      expect(written).toEqual([range, 3]);
      expect(close).toHaveBeenCalledOnce();
      expect(abort).not.toHaveBeenCalled();
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
