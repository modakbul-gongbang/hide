import { useEffect } from "react";
import type { Actions } from "../actions";
import { useShellStore } from "../store";

/**
 * The provider probe and the hook diagnosis run only while a page shows a tab
 * that reads them (B8). A hidden browser tab is not looking either; the
 * daemon releases this page's demand if the socket drops. A reconnect is a
 * new connection whose demand starts empty, so the demand is declared again
 * each time the page is live. `kit` also reads this machine's kit once.
 */
export function useAgentsDemand(actions: Actions, kit: boolean) {
  const live = useShellStore((s) => s.connection === "live");
  useEffect(() => {
    if (!live) return;
    const report = () => actions.observeAgents(document.visibilityState === "visible");
    report();
    if (kit) actions.checkKit();
    document.addEventListener("visibilitychange", report);
    return () => {
      document.removeEventListener("visibilitychange", report);
      actions.observeAgents(false);
    };
  }, [actions, live, kit]);
}
