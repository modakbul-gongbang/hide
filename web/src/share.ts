// Structural sharing for snapshot sections.
//
// The core resends the whole `rest` section whenever any part of it changes,
// and an agent's elapsed tick changes it every second. Rows the operator sees
// are React-memoized on identity, so a delta that leaves a workspace, tab or
// pane untouched has to leave its object reference untouched too. `share`
// rebuilds `next` from the bottom up, returning the `prev` subtree wherever
// the two are deep-equal, in one pass over the new value.

export function share<T>(prev: unknown, next: T): T {
  if (prev === next) return next;
  if (Array.isArray(next)) {
    if (!Array.isArray(prev)) return next;
    let same = prev.length === next.length;
    const out = next.map((item, index) => {
      const shared = share(prev[index], item);
      if (shared !== prev[index]) same = false;
      return shared;
    });
    return (same ? prev : out) as T;
  }
  if (isPlainObject(next)) {
    if (!isPlainObject(prev)) return next;
    const nextKeys = Object.keys(next);
    let same = nextKeys.length === Object.keys(prev).length;
    const out: Record<string, unknown> = {};
    for (const key of nextKeys) {
      const shared = share(prev[key], next[key]);
      if (!(key in prev) || shared !== prev[key]) same = false;
      out[key] = shared;
    }
    return (same ? prev : out) as T;
  }
  return next;
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
