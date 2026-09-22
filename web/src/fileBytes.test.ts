import { describe, expect, it } from "vitest";
import { configureFileBytes, receiveBytes, receiveBytesError, requestFileBytes } from "./fileBytes";

/** The frame hided sends: 4-byte big-endian header length, header JSON, bytes. */
function frame(header: Record<string, unknown>, bytes: number[]): ArrayBuffer {
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

  it("carries a range request's offset and length", () => {
    const { events } = captureDispatch();
    void requestFileBytes("/repo/clip.mp4", { offset: 16, length: 8 });
    expect(events[0]).toMatchObject({ path: "/repo/clip.mp4", offset: 16, length: 8 });
  });

  it("rejects with the daemon's reason and ignores a frame for an unknown request", async () => {
    const { events } = captureDispatch();
    const promise = requestFileBytes("/outside/secret.bin");
    const requestId = events[0]!.request_id as string;
    receiveBytesError({ request_id: requestId, reason: "outside_checkout" });
    await expect(promise).rejects.toThrow("outside_checkout");
    expect(() => receiveBytes(frame({ type: "file_bytes", request_id: "nobody", offset: 0, total: 1, eof: true }, [9]))).not.toThrow();
  });
});
