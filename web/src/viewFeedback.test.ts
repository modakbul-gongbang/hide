import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { patchLines } from "./editor/patch";
import { diffModel, fileModel, pickedAnchor } from "./editor/useLineComments";
import { english } from "./i18n/catalogs";
import type { ViewComment, ViewFeedback, ViewFeedbackSend } from "./snapshot";
import { commentsOn, deletedHere, dragRect, fractionRect, groupOn, linesAnchor, QUOTED_LINES, relocation, sendDisplay, SEND_REASON_KEYS, sendShown, SHOWN_MS } from "./viewFeedback";

const lines = (anchor: { start: number; end: number; lines: string[] }) => ({ kind: "lines" as const, ...anchor });

function comment(overrides: Partial<ViewComment>): ViewComment {
  return {
    id: "c1",
    display_ids: ["d1"],
    kind: "file",
    path: "/repo/src/a.ts",
    anchor: lines({ start: 2, end: 2, lines: ["b"] }),
    note: "fix",
    shot: false,
    changed: false,
    not_delivered: false,
    group: "mac/t1",
    number: 1,
    ...overrides,
  };
}

describe("a comment's lines after the file changes (D-14, B31)", () => {
  it("stays where its lines still are", () => {
    expect(relocation(comment({}), ["a", "b", "c"])).toBeNull();
  });

  it("moves with its lines when an edit above them shifts them", () => {
    expect(relocation(comment({}), ["new", "a", "b", "c"])).toEqual({ anchor: lines({ start: 3, end: 3, lines: ["b"] }), changed: false });
  });

  it("says Changed when no line with its text is left, and says it once", () => {
    expect(relocation(comment({}), ["a", "B", "c"])).toEqual({ anchor: null, changed: true });
    expect(relocation(comment({ changed: true }), ["a", "B", "c"])).toBeNull();
  });

  it("stops saying Changed when its lines come back", () => {
    expect(relocation(comment({ changed: true }), ["a", "b"])).toEqual({ anchor: lines({ start: 2, end: 2, lines: ["b"] }), changed: false });
  });

  it("follows the nearest copy of a range whose text appears twice", () => {
    const range = comment({ anchor: lines({ start: 5, end: 6, lines: ["x", "y"] }) });
    expect(relocation(range, ["x", "y", "-", "-", "-", "-", "x", "y"])).toEqual({ anchor: lines({ start: 7, end: 8, lines: ["x", "y"] }), changed: false });
  });
});

describe("the lines a note quotes", () => {
  it("quotes the picked range in order, whichever way it was dragged, and caps the quote", () => {
    expect(linesAnchor(["a", "b", "c"], 3, 2)).toEqual({ kind: "lines", start: 2, end: 3, lines: ["b", "c"] });
    const many = Array.from({ length: 30 }, (_, index) => `l${index + 1}`);
    const anchor = linesAnchor(many, 1, 30);
    expect(anchor).toMatchObject({ start: 1, end: 30 });
    expect(anchor.kind === "lines" && anchor.lines.length).toBe(QUOTED_LINES);
  });

  it("numbers a body under a frontmatter pane in the whole file", () => {
    const model = fileModel("---\ntitle: x\n---\nbody\n", 3);
    expect(pickedAnchor(model, 1, 1)).toEqual({ kind: "lines", start: 4, end: 4, lines: ["body"] });
  });
});

describe("a diff's rows (B12, B24)", () => {
  const patch = patchLines(["diff --git a/f b/f", "@@ -1,3 +1,3 @@", " keep", "-old", "+new", " tail", ""].join("\n"));
  const model = diffModel(patch);

  it("takes a range of removed rows on the old side", () => {
    expect(pickedAnchor(model, 4, 4)).toEqual({ kind: "lines", start: 2, end: 2, lines: ["old"], side: "old" });
  });

  it("takes any range with a new line on the new side", () => {
    expect(pickedAnchor(model, 3, 5)).toEqual({ kind: "lines", start: 1, end: 2, lines: ["keep", "new"], side: "new" });
  });

  it("gives a hunk header no number, so it takes no comment", () => {
    expect(model.at(2)).toBeNull();
    expect(pickedAnchor(model, 2, 2)).toBeNull();
  });
});

describe("a region (B13)", () => {
  it("is kept inside its page and stored as fractions of it, so a zoom keeps it on the same content", () => {
    const page = { width: 400, height: 200 };
    const rect = dragRect({ x: 300, y: 150 }, { x: 500, y: 50 }, page);
    expect(rect).toEqual({ x: 300, y: 50, width: 100, height: 100 });
    expect(fractionRect(rect, page)).toEqual({ x: 0.75, y: 0.25, width: 0.25, height: 0.5 });
  });
});

const send = (overrides: Partial<ViewFeedbackSend>): ViewFeedbackSend => ({ request_id: "r1", state: "sent", count: 3, to: "login", at_unix_ms: 1_000, ...overrides });

describe("the line under a View", () => {
  it("shows `Sent` for a moment and a failure until it is retried", () => {
    expect(sendShown(send({}), 1_000 + SHOWN_MS - 1)).toBe(true);
    expect(sendShown(send({}), 1_000 + SHOWN_MS)).toBe(false);
    expect(sendShown(send({ state: "failed", reason: "letters_full" }), 1_000 + SHOWN_MS * 10)).toBe(true);
  });

  it("shows a Send on the View it was made from in this window, else on the recipient's first View", () => {
    expect(sendDisplay(send({}), { r1: "d2" }, ["d1", "d2"], "d1")).toBe("d2");
    expect(sendDisplay(send({}), {}, ["d1", "d2"], "d1")).toBe("d1");
    expect(sendDisplay(send({}), { r1: "gone" }, ["d1"], "d1")).toBeNull();
  });

  it("offers Undo on the View the delete was made on, for a moment", () => {
    const feedback: ViewFeedback = { comments: [], groups: [], opened: [], deleted: "c1" };
    const deletedFrom = { commentId: "c1", displayId: "d1", at: 0 };
    expect(deletedHere(feedback, deletedFrom, "d1", SHOWN_MS - 1)).toBe(true);
    expect(deletedHere(feedback, deletedFrom, "d2", 0)).toBe(false);
    expect(deletedHere(feedback, deletedFrom, "d1", SHOWN_MS)).toBe(false);
    // Another window's delete: that window offers Undo.
    expect(deletedHere(feedback, null, "d1", 0)).toBe(false);
  });

  it("speaks for the group of the View's own comments, in their letter's order", () => {
    const feedback: ViewFeedback = {
      comments: [comment({ id: "c2", number: 2 }), comment({ id: "c1", number: 1 }), comment({ id: "c3", display_ids: ["d9"], group: "" })],
      groups: [
        { key: "", comments: 1, views: 1, not_delivered: 0, display_ids: ["d9"] },
        { key: "mac/t1", comments: 2, views: 1, not_delivered: 0, display_ids: ["d1"] },
      ],
      opened: [],
    };
    expect(commentsOn(feedback, "d1").map((row) => row.id)).toEqual(["c1", "c2"]);
    expect(groupOn(feedback, "d1")?.key).toBe("mac/t1");
    expect(groupOn(feedback, "d0")).toBeNull();
  });

  it("has words for every reason the core gives a Send that did not go", () => {
    const contract = JSON.parse(fs.readFileSync(path.resolve(__dirname, "../../contracts/snapshot-wire-enums.json"), "utf8"));
    expect(Object.keys(SEND_REASON_KEYS)).toEqual(contract.view_feedback_send_reason);
    for (const key of Object.values(SEND_REASON_KEYS)) expect(english[key]).toBeTruthy();
  });
});
