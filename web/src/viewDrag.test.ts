import { describe, expect, it } from "vitest";
import { IDLE, movePointer, pressTab, releasePointer } from "./viewDrag";
import type { DropTarget, Point } from "./viewLayout";

const reorder: DropTarget = { kind: "bar", areaId: "a1", index: 2, line: { x: 200, y: 0, height: 32 } };
const splitRight: DropTarget = { kind: "edge", areaId: "a2", edge: "right", preview: { x: 0, y: 0, width: 1, height: 1 }, label: "Split right" };
const refused: DropTarget = { kind: "none", reason: "This view area is too narrow to split." };
const always = (target: DropTarget) => () => target;
const origin: Point = { x: 100, y: 10 };

describe("a View tab drag", () => {
  it("stays a press until the pointer travels the activation distance", () => {
    const pressed = pressTab("d1", 1, origin);
    expect(movePointer(pressed, 1, { x: 105, y: 10 }, 6, always(reorder)).phase).toBe("pressed");
    expect(movePointer(pressed, 1, { x: 100, y: 15.99 }, 6, always(reorder)).phase).toBe("pressed");
    expect(movePointer(pressed, 1, { x: 106, y: 10 }, 6, always(reorder))).toMatchObject({ phase: "dragging", target: reorder });
  });

  it("is a click when released before it became a drag", () => {
    expect(releasePointer(pressTab("d1", 1, origin), 1, origin, always(reorder))).toEqual({ session: IDLE, dragged: false, drop: null });
  });

  it("lands once on the target the preview showed", () => {
    const dragging = movePointer(pressTab("d1", 1, origin), 1, { x: 300, y: 10 }, 6, always(reorder));
    expect(releasePointer(dragging, 1, { x: 300, y: 10 }, always(reorder))).toEqual({ session: IDLE, dragged: true, drop: reorder });
  });

  it("sends nothing on nothing valid, or when the target changed or vanished by release", () => {
    const onEdge = movePointer(pressTab("d1", 1, origin), 1, { x: 900, y: 300 }, 6, always(splitRight));
    expect(releasePointer(onEdge, 1, { x: 900, y: 300 }, always(refused)).drop).toBeNull();
    expect(releasePointer(onEdge, 1, { x: 900, y: 300 }, always({ ...splitRight, edge: "down" })).drop).toBeNull();
    const onNothing = movePointer(pressTab("d1", 1, origin), 1, { x: 900, y: 300 }, 6, always(refused));
    expect(releasePointer(onNothing, 1, { x: 900, y: 300 }, always(refused))).toEqual({ session: IDLE, dragged: true, drop: null });
  });

  it("ignores another pointer", () => {
    const pressed = pressTab("d1", 1, origin);
    expect(movePointer(pressed, 2, { x: 400, y: 10 }, 6, always(reorder))).toBe(pressed);
    expect(releasePointer(pressed, 2, origin, always(reorder)).session).toBe(pressed);
  });
});
