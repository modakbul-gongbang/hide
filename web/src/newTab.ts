import { fuzzyScore } from "./search";
import { changesFor, type ChangesSnapshot } from "./snapshot";

/** The changed-file palette uses Git's complete set, including deleted and ignored tracked files. */
export function changedFiles(changes: ChangesSnapshot | null, root: string | null, query = "") {
  const current = changesFor(changes, root);
  if (!current || current.unavailable_reason) return [];
  return current.entries.flatMap((entry) => {
    const score = fuzzyScore(entry.relative_path.toLowerCase(), query.toLowerCase());
    return score === null ? [] : [{ entry, score }];
  }).sort((a, b) => b.score - a.score).slice(0, 80).map(({ entry }) => entry);
}
