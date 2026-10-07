import { useMemo } from "react";
import { useShellStore } from "../store";
import { workerPanes } from "./view";

/** The panes that are Factory workers, recomputed only when the engine's summary moves. */
export function useFactoryWorkers(): ReadonlySet<string> {
  const summary = useShellStore((s) => s.factory?.summary ?? null);
  return useMemo(() => workerPanes(summary), [summary]);
}

/** The one person-facing number (PRD software-factory-ui D-05): the sidebar's Factory badge and the 내 차례 tab both read it. */
export function useFactoryTurnCount(): number {
  return useShellStore((s) => s.factory?.summary?.my_turn ?? 0);
}
