import { beforeEach, describe, expect, it } from "vitest";
import { configureAttachments, frameChunk, receiveAttachmentRefusal, refusalText, submitAttachments } from "./attachments";
import { useShellStore } from "./store";

function capture() {
  const events: { kind: string; payload: Record<string, unknown> }[] = [];
  const binaries: Uint8Array[] = [];
  configureAttachments({
    dispatch: (event) => events.push({ kind: event.kind, payload: event.payload }),
    sendBinary: (bytes) => binaries.push(bytes),
  });
  return { events, binaries };
}

function decodeFrame(bytes: Uint8Array): { header: Record<string, unknown>; body: Uint8Array } {
  const length = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint32(0, false);
  const header = JSON.parse(new TextDecoder().decode(bytes.subarray(4, 4 + length)));
  return { header, body: bytes.subarray(4 + length) };
}

describe("attachment upload", () => {
  beforeEach(() => {
    useShellStore.setState({ attachmentRefusal: null });
  });

  it("frames a chunk with its header and bytes", () => {
    const frame = frameChunk("stage-1", 42, new Uint8Array([1, 2]), true);
    const { header, body } = decodeFrame(frame);
    expect(header).toEqual({ request_id: "stage-1", offset: 42, eof: true });
    expect(Array.from(body)).toEqual([1, 2]);
  });

  it("stages each file, sends its bytes, then commits the batch", () => {
    const { events, binaries } = capture();
    submitAttachments("pane-1", [{ name: "shot.png", bytes: new Uint8Array([9, 9, 9]) }], false, true);
    expect(events.map((event) => event.kind)).toEqual(["attachment_stage", "attachment_commit"]);
    expect(events[0]!.payload).toMatchObject({ name: "shot.png", size: 3, clipboard: false });
    expect(events[1]!.payload).toMatchObject({ pane_id: "pane-1", bracketed_paste: true, clipboard: false });
    expect(Array.isArray(events[1]!.payload.stages)).toBe(true);
    const { header, body } = decodeFrame(binaries[0]!);
    expect(header).toMatchObject({ offset: 0, eof: true });
    expect(Array.from(body)).toEqual([9, 9, 9]);
  });

  it("routes a refusal to the pane the request belonged to", () => {
    const { events } = capture();
    submitAttachments("pane-9", [{ name: "shot.png", bytes: new Uint8Array([1]) }], true, false);
    const stage = events[0]!.payload.request_id as string;
    receiveAttachmentRefusal({ request_id: stage, reason: "too_large" });
    expect(useShellStore.getState().attachmentRefusal).toEqual({ pane_id: "pane-9", reason: "too_large" });
  });

  it("turns a reason into one line", () => {
    expect(refusalText("too_large")).toContain("20 MiB");
    expect(refusalText("batch_too_large")).toContain("40 MiB");
    expect(refusalText("too_many_files")).toContain("1 and 8");
    expect(refusalText("wat")).toBe("The attachment was refused.");
  });
});
