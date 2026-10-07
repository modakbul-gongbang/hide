import { useShellStore } from "../store";

/** The one person-facing number (PRD software-factory-ui D-05): the sidebar's Factory badge and the 내 차례 tab both read it. */
export function useFactoryTurnCount(): number {
  return useShellStore((s) => s.factory?.summary?.my_turn ?? 0);
}
