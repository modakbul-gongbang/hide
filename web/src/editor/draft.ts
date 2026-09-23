// The editor's live buffer, outside React: a save and the autosave timer need
// the newest keystroke even when no render has landed since. The core still
// owns the document; this is only what the view would send it next.

let latest: { tabId: string; contents: string } | null = null;

export function noteDraft(tabId: string, contents: string): void {
  latest = { tabId, contents };
}

/** The buffer for `tabId`, or null when another tab owns the last edit. */
export function latestDraft(tabId: string): string | null {
  return latest?.tabId === tabId ? latest.contents : null;
}

export function clearDraft(tabId: string): void {
  if (latest?.tabId === tabId) latest = null;
}
