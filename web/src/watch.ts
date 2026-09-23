// Which of a checkout's folders the daemon watches (PRD B2, D-08). The rule is
// the same one `hided/src/watch.rs` applies, so the two sides agree on which
// folders are live without a second protocol: the root, then the most recently
// expanded folders under it, at most `WATCH_CAP` in total. A folder past the
// cap shows the refresh badge instead of a live listing.

export const WATCH_CAP = 64;

/** The expanded folders that belong to this checkout, the root included. */
export function expandedUnderRoot(root: string, expanded: string[]): string[] {
  const prefix = `${root}/`;
  return expanded.filter((path) => path === root || path.startsWith(prefix));
}

/** The folders the daemon watches for a checkout, root first. */
export function watchedFolders(root: string, expanded: string[]): string[] {
  const under = expandedUnderRoot(root, expanded).filter((path) => path !== root);
  const room = WATCH_CAP - 1;
  const recent = under.slice(Math.max(0, under.length - room));
  return [root, ...recent];
}

export function watchedSet(root: string, expanded: string[]): Set<string> {
  return new Set(watchedFolders(root, expanded));
}
