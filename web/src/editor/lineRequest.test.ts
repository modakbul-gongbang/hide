import { describe, expect, it, vi } from "vitest";
import { LINE_REQUEST_TTL_MS, lineOffset, onLineRequest, requestLine, takeLine } from "./lineRequest";

describe("lineRequest", () => {
  it("gives the line to the first view of the document that takes it, under any of its spellings", () => {
    requestLine(["/links/p/a.ts", "/real/p/a.ts"], 12, 3, 0);
    expect(takeLine("/real/p/other.ts", 1)).toBeNull();
    expect(takeLine("/real/p/a.ts", 1)).toEqual({ line: 12, column: 3 });
    expect(takeLine("/links/p/a.ts", 2)).toBeNull();
  });

  it("drops a line whose open never landed", () => {
    requestLine(["/p/a.ts"], 4, null, 0);
    expect(takeLine("/p/a.ts", LINE_REQUEST_TTL_MS + 1)).toBeNull();
  });

  it("tells a view already on screen, which takes the newest request only", () => {
    const heard = vi.fn();
    const stop = onLineRequest(heard);
    requestLine(["/p/a.ts"], 1, null, 0);
    requestLine(["/p/a.ts"], 9, null, 0);
    stop();
    requestLine(["/p/a.ts"], 5, null, 0);
    expect(heard).toHaveBeenCalledTimes(2);
    expect(takeLine("/p/a.ts", 0)).toEqual({ line: 5, column: null });
  });

  it("clamps a line and column to the document", () => {
    const lines = ["one", "two", "three"];
    const starts = [0, 4, 8];
    const doc = { lines: 3, line: (n: number) => ({ from: starts[n - 1]!, length: lines[n - 1]!.length }) };
    expect(lineOffset(doc, 2, null)).toBe(4);
    expect(lineOffset(doc, 2, 3)).toBe(6);
    expect(lineOffset(doc, 2, 40)).toBe(7);
    expect(lineOffset(doc, 99, 1)).toBe(8);
    expect(lineOffset(doc, 0, 1)).toBe(0);
  });
});
