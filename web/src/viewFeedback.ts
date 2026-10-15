// The operator's comments on Views and their Send (PRD view-opener-feedback).
// The core owns every comment, its number, its recipient group and where a
// Send stands (`workspace_view.feedback`); this module answers what the page
// asks of that state and holds the few things only this window knows: which
// note form is open, whether a View is in Comment mode, and which View a Send
// or a delete was made from, so its line shows there.
//
// Nothing here runs per frame or per keystroke: a comment's lines are found
// again only when the saved document changes (B38).

import { create } from "zustand";
import type { MessageKey } from "./i18n/catalogs";
import type { ViewComment, ViewCommentAnchor, ViewDisplaySnapshot, ViewFeedback, ViewFeedbackGroup, ViewFeedbackSend, ViewFeedbackSendReason, ViewRect } from "./snapshot";

/** A View holds at most this many comments (`view_feedback::COMMENTS_PER_VIEW`, B15). */
export const COMMENTS_PER_VIEW = 50;
/** A note's longest text (`view_feedback::NOTE_CHARS`). */
export const NOTE_CHARS = 4_000;
/** A quoted range keeps at most this many lines (`view_feedback::QUOTED_LINES`). */
export const QUOTED_LINES = 20;
/** How long `Sent N to …` and `Comment deleted · Undo` stay on the line (B21, B43). */
export const SHOWN_MS = 6_000;

/** The kinds of View a comment points into; an image and a PDF are files. */
export type CommentSurface = "lines" | "browser" | "region";

/** Which way a View takes comments, or none for one that takes none (a video, a binary, a new tab). */
export function commentSurface(display: Pick<ViewDisplaySnapshot, "kind" | "url">, documentKind: string | null): CommentSurface | null {
  if (display.kind === "browser") return display.url ? "browser" : null;
  if (display.kind === "diff") return "lines";
  if (documentKind === "text" || documentKind === "markdown") return "lines";
  if (documentKind === "image" || documentKind === "pdf") return "region";
  return null;
}

/** The open comments that show on a display, in their letter's order. */
export function commentsOn(feedback: ViewFeedback | undefined, displayId: string): ViewComment[] {
  return (feedback?.comments ?? []).filter((comment) => comment.display_ids.includes(displayId)).sort((a, b) => a.number - b.number);
}

/**
 * The recipient group whose line a display shows: the group of its own
 * comments, else, for a View with none, nothing. A View's comments share one
 * group unless the View changed hands since; the first one's group speaks.
 */
export function groupOn(feedback: ViewFeedback | undefined, displayId: string): ViewFeedbackGroup | null {
  if (!feedback) return null;
  const own = commentsOn(feedback, displayId)[0];
  if (own) return feedback.groups.find((group) => group.key === own.group) ?? null;
  return feedback.groups.find((group) => group.display_ids.includes(displayId)) ?? null;
}

/** The comments of one group on one display, for `This view only` (B16). */
export function viewOnlyCount(feedback: ViewFeedback | undefined, displayId: string, group: string): number {
  return commentsOn(feedback, displayId).filter((comment) => comment.group === group).length;
}

/** A recipient group's key for a View an agent opened: its machine and Herdr terminal; `""` for nobody. */
export function noRecipient(group: ViewFeedbackGroup | null): boolean {
  return !group?.recipient;
}

/** The words for each reason a Send did not go (`contracts/snapshot-wire-enums.json`: `view_feedback_send_reason`). */
export const SEND_REASON_KEYS: Readonly<Record<ViewFeedbackSendReason, MessageKey>> = {
  attachments_full: "feedback.reason.attachments_full",
  letter_unsaved: "feedback.reason.letter_unsaved",
  letters_full: "feedback.reason.letters_full",
  shot_copy: "feedback.reason.shot_copy",
  reopen_failed: "feedback.state.reopenFailed",
  closed: "feedback.reason.closed",
};

/** Whether a Send's state is still worth a line: `sent` for a moment, the rest until something replaces them. */
export function sendShown(send: ViewFeedbackSend | undefined, now: number): boolean {
  if (!send) return false;
  if (send.state === "sent") return now - send.at_unix_ms < SHOWN_MS;
  return true;
}

// --- finding a comment's lines again (D-14, B31) ------------------------------

/** A text's lines as a View numbers them, from 1. */
export function textLines(text: string): string[] {
  const body = text.endsWith("\n") ? text.slice(0, -1) : text;
  return body.split("\n");
}

/**
 * Where `quoted` stands in `lines` now: at `start` when it still does, else
 * the nearest place it does, else null. The quoted lines are what the View
 * showed when the comment was left, so an edit above them moves the comment
 * with them, and an edit of the lines themselves makes it Changed.
 */
export function locateLines(quoted: readonly string[], lines: readonly string[], start: number): number | null {
  if (quoted.length === 0) return null;
  const at = (first: number) => quoted.every((line, index) => lines[first - 1 + index] === line);
  if (start >= 1 && at(start)) return start;
  let best: number | null = null;
  for (let first = 1; first + quoted.length - 1 <= lines.length; first += 1) {
    if (lines[first - 1] !== quoted[0] || !at(first)) continue;
    if (best === null || Math.abs(first - start) < Math.abs(best - start)) best = first;
  }
  return best;
}

/** What a `relocate` should tell the core about a lines comment, or null when the core already has it right. */
export function relocation(comment: Pick<ViewComment, "anchor" | "changed">, lines: readonly string[]): { anchor: ViewCommentAnchor | null; changed: boolean } | null {
  const anchor = comment.anchor;
  if (anchor.kind !== "lines") return null;
  const found = locateLines(anchor.lines, lines, anchor.start);
  if (found === null) return comment.changed ? null : { anchor: null, changed: true };
  if (found === anchor.start && !comment.changed) return null;
  return { anchor: { ...anchor, start: found, end: found + anchor.lines.length - 1 }, changed: false };
}

/** A lines anchor for rows `from`..`to` of `lines`, the quote capped as the core caps it. */
export function linesAnchor(lines: readonly string[], from: number, to: number, side?: "old" | "new"): ViewCommentAnchor {
  const start = Math.max(1, Math.min(from, to));
  const end = Math.max(from, to);
  const quoted = lines.slice(start - 1, Math.min(end, start - 1 + QUOTED_LINES));
  return side ? { kind: "lines", start, end, lines: quoted, side } : { kind: "lines", start, end, lines: quoted };
}

/** A rectangle dragged from one point to another, normalized and kept inside `bounds`. */
export function dragRect(from: { x: number; y: number }, to: { x: number; y: number }, bounds: { width: number; height: number }): ViewRect {
  const clamp = (value: number, limit: number) => Math.min(Math.max(value, 0), limit);
  const x1 = clamp(Math.min(from.x, to.x), bounds.width);
  const y1 = clamp(Math.min(from.y, to.y), bounds.height);
  const x2 = clamp(Math.max(from.x, to.x), bounds.width);
  const y2 = clamp(Math.max(from.y, to.y), bounds.height);
  return { x: x1, y: y1, width: x2 - x1, height: y2 - y1 };
}

/** A pixel rectangle on a page of `size` as fractions of it: what a region anchor stores, so a zoom keeps it on the same content (B13). */
export function fractionRect(rect: ViewRect, size: { width: number; height: number }): ViewRect {
  const round = (value: number) => Math.round(value * 1e6) / 1e6;
  return { x: round(rect.x / size.width), y: round(rect.y / size.height), width: round(rect.width / size.width), height: round(rect.height / size.height) };
}

/** A drag smaller than this, in CSS pixels, is a click and makes no region. */
export const MIN_DRAG = 6;

// --- this window's own state ------------------------------------------------------

/** A note being written: a new comment on an anchor, or an edit of one. */
export type NoteForm = {
  displayId: string;
  anchor: ViewCommentAnchor;
  commentId: string | null;
  note: string;
  /** A region's screenshot, cropped as the note is added (B25). */
  shot?: () => Promise<string | null>;
  /** The file's revision as the comment was left, which finds a changed image or PDF (B31). */
  revision?: string | null;
};

/** A browser View's Comment mode as hided last answered it. */
export type BrowserComment = { on: boolean; reason: string | null };

type FeedbackStore = {
  /** One open note per display. */
  forms: Record<string, NoteForm>;
  /** Image and PDF Views in Comment mode. */
  regionMode: Record<string, true>;
  /** Whether an image or PDF View's file could be read, which Comment needs (B13). */
  readable: Record<string, boolean>;
  /** Browser Views in Comment mode, by display. */
  browser: Record<string, BrowserComment>;
  /** The display each Send of this window was made from, by request id. */
  sentFrom: Record<string, string>;
  /** The display the last delete of this window was made on, and when. */
  deletedFrom: { commentId: string; displayId: string; at: number } | null;
};

export const useFeedbackStore = create<FeedbackStore>(() => ({ forms: {}, regionMode: {}, readable: {}, browser: {}, sentFrom: {}, deletedFrom: null }));

/** The longest list of Sends a window remembers the display of. */
const SENT_FROM_CAP = 32;

export function openNote(form: NoteForm): void {
  useFeedbackStore.setState((state) => ({ forms: { ...state.forms, [form.displayId]: form } }));
}

export function closeNote(displayId: string): void {
  useFeedbackStore.setState((state) => {
    if (!state.forms[displayId]) return state;
    const forms = { ...state.forms };
    delete forms[displayId];
    return { forms };
  });
}

export function setRegionMode(displayId: string, on: boolean): void {
  useFeedbackStore.setState((state) => {
    const regionMode = { ...state.regionMode };
    if (on) regionMode[displayId] = true;
    else delete regionMode[displayId];
    return { regionMode };
  });
  if (!on) closeNote(displayId);
}

/** An image or PDF viewer read its file, or could not; a viewer that goes takes its answer with it. */
export function noteReadable(displayId: string, readable: boolean | null): void {
  useFeedbackStore.setState((state) => {
    const next = { ...state.readable };
    if (readable === null) delete next[displayId];
    else next[displayId] = readable;
    return { readable: next };
  });
}

export function noteBrowserComment(displayId: string, comment: BrowserComment): void {
  useFeedbackStore.setState((state) => ({ browser: { ...state.browser, [displayId]: comment } }));
}

export function noteSentFrom(requestId: string, displayId: string): void {
  useFeedbackStore.setState((state) => {
    const entries = Object.entries({ ...state.sentFrom, [requestId]: displayId });
    return { sentFrom: Object.fromEntries(entries.slice(-SENT_FROM_CAP)) };
  });
}

export function noteDeletedFrom(commentId: string, displayId: string, at: number): void {
  useFeedbackStore.setState({ deletedFrom: { commentId, displayId, at } });
}

/**
 * The display a Send's line shows on: the one it was made from in this
 * window, else (a Send another window made, or one from before a reload) the
 * first display the recipient's comments were on, else none.
 */
export function sendDisplay(send: ViewFeedbackSend | undefined, sentFrom: Record<string, string>, displayIds: readonly string[], fallback: string | null): string | null {
  if (!send) return null;
  const own = sentFrom[send.request_id];
  if (own) return displayIds.includes(own) ? own : null;
  return fallback;
}

/** Whether `Comment deleted · Undo` belongs on this display now. */
export function deletedHere(feedback: ViewFeedback | undefined, deletedFrom: FeedbackStore["deletedFrom"], displayId: string, now: number, browserOn: boolean): boolean {
  const deleted = feedback?.deleted;
  if (!deleted) return false;
  if (deletedFrom?.commentId === deleted) return deletedFrom.displayId === displayId && now - deletedFrom.at < SHOWN_MS;
  // A delete made in a page's own overlay: the page in Comment mode says it.
  return browserOn;
}
