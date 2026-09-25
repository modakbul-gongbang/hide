// Several displays of one document on screen at once (PRD S7 B4, D-03, D-13).
// The core holds one buffer per document; each mounted editor view of it
// joins this channel by the document's editor tab id. An edit the operator
// makes in one view reaches the others at once as the smallest change that
// turns their text into it, before the core's echo arrives, and each of
// them records that text as its pending echo, so an older echo still in
// flight is dropped by every view rather than written back over the edit.
// The channel carries whole texts, not keystrokes: a view that fell behind
// catches up in one change.

/** The span of `before` to replace with `insert` so that it reads `after`. */
export type TextChange = { from: number; to: number; insert: string };

/**
 * The smallest change from `before` to `after`: everything outside the
 * common prefix and suffix. A boundary never splits a UTF-16 surrogate pair,
 * so a character is replaced whole. Null when the texts are equal.
 */
export function minimalChange(before: string, after: string): TextChange | null {
  if (before === after) return null;
  const shorter = Math.min(before.length, after.length);
  let start = 0;
  while (start < shorter && before.charCodeAt(start) === after.charCodeAt(start)) start += 1;
  if (start > 0 && isHighSurrogate(before.charCodeAt(start - 1))) start -= 1;
  let endBefore = before.length;
  let endAfter = after.length;
  while (endBefore > start && endAfter > start && before.charCodeAt(endBefore - 1) === after.charCodeAt(endAfter - 1)) {
    endBefore -= 1;
    endAfter -= 1;
  }
  if (endBefore < before.length && isLowSurrogate(before.charCodeAt(endBefore))) {
    endBefore += 1;
    endAfter += 1;
  }
  return { from: start, to: endBefore, insert: after.slice(start, endAfter) };
}

function isHighSurrogate(code: number): boolean {
  return code >= 0xd800 && code <= 0xdbff;
}

function isLowSurrogate(code: number): boolean {
  return code >= 0xdc00 && code <= 0xdfff;
}

/**
 * What a view does with the core's contents for its document (the S3 echo
 * rule, per view): equal to its own text settles its pending echo; while an
 * edit it sent or received is pending, any other text is an older echo and is
 * dropped; otherwise it is a real change (a reload, another client) and
 * replaces the view's text.
 */
export function echoDecision(incoming: string, current: string, pending: string | null): "settle" | "drop" | "replace" {
  if (incoming === current) return "settle";
  if (pending !== null && incoming !== pending) return "drop";
  return "replace";
}

/** A mounted view's text and the edit it is still waiting to see echoed. */
export type PeerState = { text: string; pending: string | null };

type Peer = { receive: (contents: string) => void; state: () => PeerState };

const peers = new Map<string, Set<Peer>>();

/**
 * Where a view about to mount starts: the text and pending echo of a view of
 * the same document already on screen, which may be ahead of the core's
 * contents, or null when there is none and the core's contents are current.
 */
export function peerState(tabId: string): PeerState | null {
  const first = peers.get(tabId)?.values().next();
  return first && !first.done ? first.value.state() : null;
}

/**
 * Joins the channel of one document. `receive` gets every text another view
 * of the same document publishes; the view applies it outside its undo
 * history and takes it as its pending echo. `state` answers `peerState` for
 * a view that mounts later. `publish` sends this view's edit to the others;
 * `leave` must be called when the view goes away.
 */
export function joinDocument(tabId: string, receive: (contents: string) => void, state: () => PeerState): { publish: (contents: string) => void; leave: () => void } {
  const peer: Peer = { receive, state };
  let members = peers.get(tabId);
  if (!members) {
    members = new Set();
    peers.set(tabId, members);
  }
  members.add(peer);
  return {
    publish: (contents) => {
      for (const other of peers.get(tabId) ?? []) if (other !== peer) other.receive(contents);
    },
    leave: () => {
      const current = peers.get(tabId);
      current?.delete(peer);
      if (current?.size === 0) peers.delete(tabId);
    },
  };
}
