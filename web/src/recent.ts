// Most-recently-used order for tabs and checkouts, kept for the session.
//
// The core reports what is focused, not what was focused before, so the
// recent-tab and recent-project chords (⌥` / ⌥Tab) and the project row's
// "last checkout" (PRD S2 D-07) read this list. It is a shell convenience:
// nothing here is authority, and a reconnect rebuilds it from use.

const RECENT_CAP = 9;

const recentTabs = new Map<string, string[]>();
let recentCheckouts: string[] = [];

function bump(list: string[], id: string): string[] {
  return [id, ...list.filter((row) => row !== id)].slice(0, RECENT_CAP);
}

export function rememberTab(checkoutId: string, tabId: string) {
  recentTabs.set(checkoutId, bump(recentTabs.get(checkoutId) ?? [], tabId));
}

export function rememberCheckout(checkoutId: string) {
  recentCheckouts = bump(recentCheckouts, checkoutId);
}

/** Tab ids of `checkoutId` in MRU order, restricted to `existing`, with unseen tabs after them in their own order. */
export function recentTabOrder(checkoutId: string, existing: string[]): string[] {
  const seen = (recentTabs.get(checkoutId) ?? []).filter((id) => existing.includes(id));
  return [...seen, ...existing.filter((id) => !seen.includes(id))];
}

export function recentCheckoutOrder(existing: string[]): string[] {
  const seen = recentCheckouts.filter((id) => existing.includes(id));
  return [...seen, ...existing.filter((id) => !seen.includes(id))];
}

/** Test seam. */
export function resetRecent() {
  recentTabs.clear();
  recentCheckouts = [];
}
