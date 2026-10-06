// A shared reader for the Settings tabs: an edit is pending until the core says it landed.

import { useShellStore } from "../store";

/** The core's newest error, if it arrived after `since` and is one of `kinds`' prefixes. */
export function useErrorSince(since: number | null, prefixes: readonly string[]): string | null {
  const error = useShellStore((s) => s.rest?.status?.last_error ?? null);
  if (since === null || !error || error.occurred_at < since) return null;
  return prefixes.some((prefix) => error.kind.startsWith(prefix)) ? error.message : null;
}
