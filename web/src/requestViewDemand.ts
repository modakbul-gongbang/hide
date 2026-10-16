// The one visible-board demand the core re-reads pending checks for (PRD
// work-tab-pr-lens B44): the PRs tool and an open PR card each hold it while
// shown, and the core is told it is gone only when the last holder lets go, so
// closing a card does not stop the tool still on screen.

import { useEffect } from "react";
import type { Actions } from "./actions";

const holders = new Set<symbol>();

function publish(actions: Actions): void {
  actions.observeRequestView(holders.size > 0 && document.visibilityState === "visible");
}

/** Holds the demand while `enabled` and the page is visible. */
export function useRequestViewDemand(actions: Actions, enabled: boolean): void {
  useEffect(() => {
    if (!enabled) return;
    const holder = Symbol("request-view-demand");
    holders.add(holder);
    const report = () => publish(actions);
    report();
    document.addEventListener("visibilitychange", report);
    return () => {
      document.removeEventListener("visibilitychange", report);
      holders.delete(holder);
      report();
    };
  }, [actions, enabled]);
}
