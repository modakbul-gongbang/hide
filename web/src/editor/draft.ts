// The editor's live buffers, outside React: a save, a close and the autosave
// timer need the newest keystroke even when no render has landed since. The
// core still owns the document; this is only what the view would send it next,
// kept per tab so one tab's edit can never stand in for another's. An entry
// lives while the core has not confirmed the exact text, so a closed or saved
// tab cannot hand its old edit to whatever opens at that path next (B5, D-14).

const drafts = new Map<string, string>();
const sent = new Map<string, string>();
// Documents whose close is carrying their draft as its own save (S7 B5): the
// display goes at once while the document waits for that save, and neither
// autosave nor the save on leaving sends the draft a second time meanwhile.
const closing = new Set<string>();

export function noteClosing(tabId: string, inFlight: boolean): void {
  if (inFlight) closing.add(tabId);
  else closing.delete(tabId);
}

export function closingWithSave(tabId: string): boolean {
  return closing.has(tabId);
}

export function noteDraft(tabId: string, contents: string): void {
  drafts.set(tabId, contents);
}

/** The newest buffer for `tabId`, or null when that tab has no unsaved edit. */
export function latestDraft(tabId: string): string | null {
  return drafts.get(tabId) ?? null;
}

/** What the last save of `tabId` carried, so its echo can settle the buffer. */
export function noteSent(tabId: string, contents: string): void {
  sent.set(tabId, contents);
}

/**
 * Drops the buffer once the core reports the document clean, but only when it
 * still holds exactly the text that save carried: a keystroke that arrived
 * after the save was dispatched keeps its buffer.
 */
export function settleDraft(tabId: string): void {
  if (drafts.get(tabId) === sent.get(tabId)) drafts.delete(tabId);
  sent.delete(tabId);
}

export function clearDraft(tabId: string): void {
  drafts.delete(tabId);
  sent.delete(tabId);
}

/** Drops every buffer whose tab is no longer open: a tab id names a path, so
 * an edit that outlived its tab would otherwise be applied to the next
 * document opened there (B5, D-14). */
export function pruneDrafts(openTabIds: Set<string>): void {
  for (const tabId of [...drafts.keys(), ...sent.keys(), ...closing]) {
    if (!openTabIds.has(tabId)) {
      drafts.delete(tabId);
      sent.delete(tabId);
      closing.delete(tabId);
    }
  }
}
