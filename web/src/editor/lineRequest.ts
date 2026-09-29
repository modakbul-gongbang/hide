// A line a terminal link asked a document to show (`src/foo.ts:42`).
//
// The core's file events carry no position, and where a view is scrolled is
// the page's own state (each display keeps its place, `CodeMirrorEditor.tsx`),
// so the request waits here for a view of that document to take it: the one
// that mounts when the open lands, or one already on screen when the open
// only brought it forward. Only the newest request is kept, and only one
// view takes it.

/** How long an open may take to land before its line is dropped. */
export const LINE_REQUEST_TTL_MS = 10_000;

type LineRequest = { paths: readonly string[]; line: number; column: number | null; expires: number };

let pending: LineRequest | null = null;
const listeners = new Set<() => void>();

/** Asks the next view of the document at any of `paths` (its spellings) to show `line`, and `column` on it. */
export function requestLine(paths: readonly string[], line: number, column: number | null, now = Date.now()): void {
  pending = { paths, line, column, expires: now + LINE_REQUEST_TTL_MS };
  for (const listener of listeners) listener();
}

/** Takes the line asked for the document at `path`, so no other view moves too; null when none is waiting for it. */
export function takeLine(path: string, now = Date.now()): { line: number; column: number | null } | null {
  if (pending && pending.expires < now) pending = null;
  if (!pending || !pending.paths.includes(path)) return null;
  const { line, column } = pending;
  pending = null;
  return { line, column };
}

/** Calls `listener` on every new request; returns the unsubscribe. */
export function onLineRequest(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** The offset of 1-based `line` and `column` in a document of `lines` lines, both clamped to what exists. */
export function lineOffset(doc: { lines: number; line(n: number): { from: number; length: number } }, line: number, column: number | null): number {
  const target = doc.line(Math.min(Math.max(1, line), doc.lines));
  return target.from + Math.min(Math.max(0, (column ?? 1) - 1), target.length);
}
