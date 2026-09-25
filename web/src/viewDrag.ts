// A View tab drag as a local session (PRD S7 B6-B8, D-04, D-05). A press
// becomes a drag only past the activation distance, so a click stays a
// click; while dragging, the target under the pointer is previewed and
// nothing else moves. A release lands only on the target the preview showed,
// and only if that target is still the same place when resolved again at
// release; anything else - Escape, a release on nothing, a target that
// vanished or became ineligible - ends with nothing sent. These functions are
// pure: the caller resolves targets (`viewLayout.dropTarget`) and dispatches.

import { sameTarget, type DropTarget, type Point } from "./viewLayout";

export type DragSession =
  | { phase: "idle" }
  | { phase: "pressed"; displayId: string; pointerId: number; origin: Point }
  | { phase: "dragging"; displayId: string; pointerId: number; point: Point; target: DropTarget };

export const IDLE: DragSession = { phase: "idle" };

export function pressTab(displayId: string, pointerId: number, origin: Point): DragSession {
  return { phase: "pressed", displayId, pointerId, origin };
}

/**
 * The session after the pointer moved to `point`. A press turns into a drag
 * once the pointer is `threshold` pixels or more from where it went down; a
 * drag re-resolves its target on every move. Another pointer changes nothing.
 */
export function movePointer(session: DragSession, pointerId: number, point: Point, threshold: number, resolve: (point: Point) => DropTarget): DragSession {
  if (session.phase === "idle" || session.pointerId !== pointerId) return session;
  if (session.phase === "pressed") {
    if (Math.hypot(point.x - session.origin.x, point.y - session.origin.y) < threshold) return session;
    return { phase: "dragging", displayId: session.displayId, pointerId, point, target: resolve(point) };
  }
  return { ...session, point, target: resolve(point) };
}

/**
 * The end of a press at `point`. `drop` is the target to send, or null: a
 * press that never became a drag is a click, and a drag lands only when the
 * target resolved at release, with the layout as it is then, is the one the
 * preview showed and is a place to land. A release outside the window
 * resolves to nothing.
 */
export function releasePointer(
  session: DragSession,
  pointerId: number,
  point: Point,
  resolve: (point: Point) => DropTarget,
): { session: DragSession; dragged: boolean; drop: Exclude<DropTarget, { kind: "none" }> | null } {
  if (session.phase === "idle" || session.pointerId !== pointerId) return { session, dragged: false, drop: null };
  if (session.phase === "pressed") return { session: IDLE, dragged: false, drop: null };
  const now = resolve(point);
  const lands = now.kind !== "none" && sameTarget(now, session.target);
  return { session: IDLE, dragged: true, drop: lands ? now : null };
}
