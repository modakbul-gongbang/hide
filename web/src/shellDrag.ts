// A drag the shell draws (a tab, a divider, the side panel's edge, an
// Explorer item) marks the document root while it runs. The mark sets the
// pointer and stops text selection (`index.css`), and it is the one signal
// the browser displays freeze on: a native page is drawn above the shell, so
// it gives its place to its still until the drag ends (`browserViews.ts`).

/** The root attributes a shell drag sets, one per column its drag belongs to. */
export const SHELL_DRAG_ATTRIBUTES = ["data-view-drag", "data-agent-drag"] as const;

export type ShellDragAttribute = (typeof SHELL_DRAG_ATTRIBUTES)[number];

/**
 * What a drag is doing, as the pointer shows it: moving a tab, a place that
 * would not land, a divider along either axis, or a file the OS drag carries.
 */
export type ShellDrag = "move" | "forbidden" | "col-resize" | "row-resize" | "file";

/** Marks the root for a shell drag; the returned release removes the mark. */
export function holdShellDrag(drag: ShellDrag, attribute: ShellDragAttribute = "data-view-drag"): () => void {
  const root = document.documentElement;
  root.setAttribute(attribute, drag);
  return () => root.removeAttribute(attribute);
}

/** Whether a shell drag is running now. */
export function shellDragging(): boolean {
  const root = document.documentElement;
  return SHELL_DRAG_ATTRIBUTES.some((attribute) => root.hasAttribute(attribute));
}
