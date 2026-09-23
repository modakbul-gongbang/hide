// The editor's live buffers, outside React: a save, a close and the autosave
// timer need the newest keystroke even when no render has landed since. The
// core still owns the document; this is only what the view would send it next,
// kept per tab so one tab's edit can never stand in for another's.

const drafts = new Map<string, string>();

export function noteDraft(tabId: string, contents: string): void {
  drafts.set(tabId, contents);
}

/** The newest buffer for `tabId`, or null when that tab has no unsaved edit. */
export function latestDraft(tabId: string): string | null {
  return drafts.get(tabId) ?? null;
}

export function clearDraft(tabId: string): void {
  drafts.delete(tabId);
}
