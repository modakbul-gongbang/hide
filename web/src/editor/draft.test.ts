import { beforeEach, describe, expect, it } from "vitest";
import { clearDraft, latestDraft, noteDraft, noteSent, pruneDrafts, settleDraft } from "./draft";

describe("draft buffers", () => {
  beforeEach(() => {
    for (const tabId of ["a", "b", "c", "d"]) clearDraft(tabId);
  });

  it("keeps one buffer per tab, so one tab's edit is never another's", () => {
    noteDraft("a", "one");
    noteDraft("b", "two");
    expect(latestDraft("a")).toBe("one");
    expect(latestDraft("b")).toBe("two");
    expect(latestDraft("c")).toBeNull();
  });

  it("settles a buffer the save confirmed, and only that one", () => {
    // A buffer that still equals the sent text goes when the save lands.
    noteDraft("a", "saved text");
    noteSent("a", "saved text");
    settleDraft("a");
    expect(latestDraft("a")).toBeNull();

    // A keystroke that arrived after the save was dispatched keeps its text.
    noteDraft("b", "saved text");
    noteSent("b", "saved text");
    noteDraft("b", "typed after the save");
    settleDraft("b");
    expect(latestDraft("b")).toBe("typed after the save");
  });

  it("prunes every buffer whose tab is no longer open", () => {
    noteDraft("a", "one");
    noteDraft("b", "two");
    pruneDrafts(new Set(["b"]));
    expect(latestDraft("a")).toBeNull();
    expect(latestDraft("b")).toBe("two");
  });

  it("clears both the buffer and its sent stamp", () => {
    noteDraft("c", "x");
    noteSent("c", "x");
    clearDraft("c");
    settleDraft("c");
    expect(latestDraft("c")).toBeNull();
  });

  it("does not settle a buffer typed again with the same text after a clear", () => {
    noteDraft("d", "x");
    noteSent("d", "x");
    clearDraft("d");
    noteDraft("d", "x");
    settleDraft("d");
    expect(latestDraft("d")).toBe("x");
  });

  it("keeps a newer buffer when an older save's echo lands after it", () => {
    noteDraft("a", "first");
    noteSent("a", "first");
    // The operator types again before that save's echo arrives; the echo must
    // not settle the text it never carried.
    noteDraft("a", "second");
    settleDraft("a");
    expect(latestDraft("a")).toBe("second");
  });
});
