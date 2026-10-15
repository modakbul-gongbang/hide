import { useEffect } from "react";
import type { Actions } from "./actions";

// Where a backlog item came from carries its session's request and answer
// (PRD overview-device-tasks B31, B33), so the core keeps it only while a
// screen shows it. The item's panel and its Start dialog can both show one
// item's origin; each holds it, and the last to leave lets the core drop it.
const holders = new Map<string, number>();

/** Takes one hold on an item's origin. */
export function holdOrigin(taskKey: string): void {
  holders.set(taskKey, (holders.get(taskKey) ?? 0) + 1);
}

/** Lets go of one hold; true when it was the last one on that item. */
export function releaseOrigin(taskKey: string): boolean {
  const held = holders.get(taskKey);
  if (held === undefined) throw new Error(`no hold on the origin of ${taskKey}`);
  if (held > 1) {
    holders.set(taskKey, held - 1);
    return false;
  }
  holders.delete(taskKey);
  return true;
}

/** Reads an item's origin while the caller is shown, and closes it when no screen shows it any more. */
export function useBacklogOrigin(actions: Actions, workspaceId: string, taskKey: string): void {
  useEffect(() => {
    holdOrigin(taskKey);
    actions.readBacklogOrigin(workspaceId, taskKey);
    return () => {
      if (releaseOrigin(taskKey)) actions.closeBacklogOrigin(taskKey);
    };
  }, [actions, workspaceId, taskKey]);
}
