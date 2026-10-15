import { useEffect } from "react";
import type { Actions } from "./actions";
import type { BacklogOrigin, SnapshotRest } from "./snapshot";
import { useShellStore } from "./store";

// Where a backlog item came from carries its session's request and answer
// (PRD overview-device-tasks B31, B33), so the core keeps it only while a
// screen shows it. The item's panel and its Start dialog can both show one
// item's origin; each holds it, and the last in this window to leave tells the
// daemon, which asks the core to drop it only when no other window shows it.
const holders = new Map<string, number>();

/** The item's origin, while some window shows it; each item has its own. */
export function originOf(rest: SnapshotRest | null, taskKey: string): BacklogOrigin | null {
  return rest?.issue_work?.origins?.find((origin) => origin.task_key === taskKey) ?? null;
}

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

/**
 * Reads an item's origin while the caller is shown, and closes it when no
 * screen shows it any more. A reconnect is a new connection, whose holds the
 * daemon starts empty and whose old ones it has let go, so the read is said
 * again each time the page is live.
 */
export function useBacklogOrigin(actions: Actions, workspaceId: string, taskKey: string): void {
  const live = useShellStore((state) => state.connection === "live");
  useEffect(() => {
    holdOrigin(taskKey);
    return () => {
      if (releaseOrigin(taskKey)) actions.closeBacklogOrigin(taskKey);
    };
  }, [actions, taskKey]);
  useEffect(() => {
    if (live) actions.readBacklogOrigin(workspaceId, taskKey);
  }, [actions, workspaceId, taskKey, live]);
}
