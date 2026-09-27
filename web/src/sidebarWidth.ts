// The sidebar's width drag (PRD sidebar-typography D-08, D-09, B10-B12): the
// edge follows the pointer between the bounds and the width the core keeps
// is sent once, on release or on a double-click reset. The bounds are
// `--size-sidebar-min` and `--size-sidebar-max`, the reset is
// `--size-sidebar-ideal`; the core refuses anything outside the bounds.

export type SidebarWidthBounds = { min: number; max: number };

/** Where the edge stands for a pointer that moved `travel` CSS pixels from a drag begun at `start` wide: whole pixels, never past a bound. */
export function draggedSidebarWidth(start: number, travel: number, bounds: SidebarWidthBounds): number {
  return Math.min(bounds.max, Math.max(bounds.min, Math.round(start + travel)));
}

/** The width a gesture sends, or null when it lands where it began and there is nothing to store. */
export function sidebarWidthToSend(stored: number, landed: number): number | null {
  return landed === stored ? null : landed;
}
