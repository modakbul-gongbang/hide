// What the issue panel and the half-second preview card read about an issue
// (PRD overview-lenses-issues D-40, D-42, B15, B22). The core answers one read
// at a time in `issue_work.detail`; this keeps each answer by issue so the
// panel shows the last one while it reads again and the preview reads an
// issue once. The cache is this window's presentation state and publishes
// nothing: the only thing it sends is a read, and the preview sends at most
// one per issue, never while another read is in flight.

import { create } from "zustand";
import type { IssueDetail } from "./snapshot";
import { useShellStore } from "./store";

/** The issues a window keeps, the most recently read last; past it the oldest goes. */
const CACHE_LIMIT = 200;

type DetailStore = {
  /** The last ready answer per issue. */
  ready: Map<string, IssueDetail>;
  /** The issues the preview has asked for and not yet had answered, so it asks once; a failed one may be asked again. */
  previewed: Set<string>;
};

export const useIssueDetails = create<DetailStore>(() => ({ ready: new Map(), previewed: new Set() }));

function remember(detail: IssueDetail) {
  const ready = new Map(useIssueDetails.getState().ready);
  ready.delete(detail.task_key);
  ready.set(detail.task_key, detail);
  while (ready.size > CACHE_LIMIT) {
    const oldest = ready.keys().next();
    if (oldest.done) break;
    ready.delete(oldest.value);
  }
  useIssueDetails.setState({ ready });
}

// Each ready answer the core hands over is kept by its issue, and an
// answered preview ask is settled: a ready one is in the cache, a failed one
// may be asked again on the next rest.
useShellStore.subscribe((state, previous) => {
  const detail = state.rest?.issue_work?.detail ?? null;
  if (!detail || detail === previous.rest?.issue_work?.detail || detail.phase === "reading") return;
  if (detail.phase === "ready") remember(detail);
  const { previewed } = useIssueDetails.getState();
  if (previewed.has(detail.task_key)) {
    const rest = new Set(previewed);
    rest.delete(detail.task_key);
    useIssueDetails.setState({ previewed: rest });
  }
});

/** The core's slot while it holds `taskKey`: reading, its answer, or why it failed. */
export function useDetailSlot(taskKey: string | null): IssueDetail | null {
  return useShellStore((s) => {
    const detail = s.rest?.issue_work?.detail ?? null;
    return taskKey !== null && detail?.task_key === taskKey ? detail : null;
  });
}

/** The last ready answer for `taskKey`, if any. */
export function useCachedDetail(taskKey: string | null): IssueDetail | null {
  return useIssueDetails((s) => (taskKey === null ? null : (s.ready.get(taskKey) ?? null)));
}

/** A read is in flight in the core's one slot. */
function reading(): boolean {
  return useShellStore.getState().rest?.issue_work?.detail?.phase === "reading";
}

/**
 * The preview card asks for an issue it has no answer for, once per issue and
 * never over a read in flight, so resting across a column sends at most one
 * `gh` read per issue and one at a time (B22).
 */
export function previewRead(taskKey: string, request: () => void) {
  const { ready, previewed } = useIssueDetails.getState();
  if (ready.has(taskKey) || previewed.has(taskKey) || reading()) return;
  const asked = new Set(previewed).add(taskKey);
  // An ask whose answer a later read replaced is never settled; the oldest goes past the cap.
  for (const key of asked) {
    if (asked.size <= CACHE_LIMIT) break;
    asked.delete(key);
  }
  useIssueDetails.setState({ previewed: asked });
  request();
}
