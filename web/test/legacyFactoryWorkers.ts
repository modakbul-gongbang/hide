// Frozen worker membership from main a612a7de. Fixture data only.
import type { FactorySummary } from "../src/factory/model";

/** The panes that are Factory workers, which the Overview leaves out of its requests and count (B13). */
export function workerPanes(summary: FactorySummary | null | undefined): ReadonlySet<string> {
  const panes = new Set<string>();
  for (const factory of summary?.factories ?? []) {
    for (const column of factory.columns) for (const card of column.cards) if (card.worker_pane) panes.add(card.worker_pane);
  }
  return panes;
}
