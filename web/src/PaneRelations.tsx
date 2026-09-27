import { CornerUpLeftIcon } from "lucide-react";
import { useState } from "react";
import type { Actions } from "./actions";
import { AgentMark } from "./AgentMark";
import { Button } from "./components/ui/button";
import { Hint } from "./components/ui/tooltip";
import { EntryPointMenu, type MenuEntry } from "./components/entry-menu";
import { StatusMark } from "./components/status-mark";
import { DeviceChip } from "./components/device-chip";
import { chipTitle, chipTone, directChildren, parentStep, relationEntries, relationState } from "./lineage";
import type { PaneRow, SnapshotRest, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";

// The delegation tree where the operator works (PRD S6 D-07, B14-B16): a
// child pane's header returns to its parent, a parent's header lists every
// direct child on one scrolling row, and the pane menu lists parent,
// siblings and children, each moved to only by its explicit Open. Every move
// is one tracked focus; its pending and failed states show in the Agent area,
// which stays on screen while the core moves the visible tab to the target,
// and a failure never splits the parent or makes a pane.

/**
 * The compact Return control in a child pane's identity row (B16): one mark,
 * so the pane's own name keeps the room, with the parent named in its
 * tooltip and accessible name.
 */
export function ReturnToParent({ pane, actions }: { pane: PaneRow; actions: Actions }) {
  const parent = parentStep(pane);
  const pending = useRelationPending(pane.id, parent?.pane_id ?? null);
  if (!parent) return null;
  const label = `Return to parent ${parent.label}`;
  return (
    <Hint label={label}>
      <Button
        variant="ghost"
        size="icon-sm"
        className="shrink-0 hover:bg-popover hover:text-foreground"
        aria-label={label}
        aria-busy={pending}
        data-pane-return={parent.pane_id}
        disabled={pending}
        onClick={() => actions.followRelation(pane.id, parent.pane_id, parent.label)}
      >
        {pending ? <span aria-hidden="true">…</span> : <CornerUpLeftIcon />}
      </Button>
    </Hint>
  );
}

/**
 * Every direct child of the pane's agent on one row under the header (B14).
 * The row scrolls sideways instead of growing; a pane with no children has
 * no row at all.
 */
export function ChildChipRow({ pane, actions }: { pane: PaneRow; actions: Actions }) {
  const chips = directChildren(pane);
  const rest = useShellStore((s) => s.rest);
  const parentLocation = paneLocation(rest, pane.id);
  const relation = useUiStore((s) => s.relation);
  const outcome = useShellStore((s) => s.rest?.status?.pane_focus_request);
  if (chips.length === 0) return null;
  const state = relation?.sourcePaneId === pane.id ? relationState(relation, outcome) : null;
  return (
    <div
      className="flex h-[var(--size-pane-child-row)] shrink-0 items-center gap-xxs overflow-x-auto overflow-y-hidden whitespace-nowrap bg-card px-sm text-caption"
      role="group"
      aria-label={`Children of ${pane.identity_label ?? pane.id}`}
      data-pane-children={pane.id}
    >
      {chips.map((chip) => {
        const pending = state?.phase === "pending" && relation?.targetPaneId === chip.pane_id;
        const childLocation = paneLocation(rest, chip.pane_id);
        const label = childLocation?.checkout !== parentLocation?.checkout && chip.checkout_label ? chip.checkout_label : chip.label;
        const device = childLocation && childLocation.deviceId !== parentLocation?.deviceId ? childLocation.deviceLabel : null;
        const title = chipTitle({ ...chip, label });
        return (
          <Hint key={chip.pane_id} label={title} reveals>
          <button
            type="button"
            className={`flex max-w-[var(--size-pane-child-chip-max)] shrink-0 items-center gap-xxs rounded-xs border border-border px-xxs outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring ${
              chip.delegated ? "text-subtle-foreground" : "text-foreground"
            }`}
            aria-label={`Open child ${title}`}
            aria-busy={pending}
            data-child-chip={chip.pane_id}
            data-pending={pending ? "true" : "false"}
            onClick={() => {
              if (!pending) actions.followRelation(pane.id, chip.pane_id, chip.label);
            }}
          >
            <StatusMark symbol={pending ? "…" : chip.symbol} className={chipTone(chip)} />
            <AgentMark kind={chip.agent_kind} />
            <span className="truncate">{label}</span>
            {device ? <DeviceChip label={device} className="max-w-2/5" /> : null}
          </button>
          </Hint>
        );
      })}
    </div>
  );
}

function paneLocation(rest: SnapshotRest | null, paneId: string): { checkout: string; deviceId: string; deviceLabel: string } | null {
  const workspaces: Workspace[] = [
    ...(rest?.navigator?.workspaces ?? []),
    ...(rest?.status?.remote ?? []).filter((status) => status.state === "connected").flatMap((status) => status.session?.workspaces ?? []),
  ];
  for (const workspace of workspaces) {
    for (const checkout of workspace.checkouts) {
      if (!checkout.tabs.some((tab) => tab.panes.some((candidate) => candidate.id === paneId))) continue;
      const deviceLabel = rest?.navigator?.devices?.find((device) => device.id === workspace.device_id)?.label ?? (workspace.device_id === "local" ? "This Mac" : workspace.device_id);
      return { checkout: checkout.id, deviceId: workspace.device_id, deviceLabel };
    }
  }
  return null;
}

/**
 * The relationship focus the operator asked for, while it is in flight or
 * after it failed (B15). It is drawn once in the Agent area rather than under
 * the pane it was asked from: the core moves the visible tab to the target in
 * the same event and keeps it there on a refusal, so the asking pane is often
 * no longer on screen when the answer comes.
 */
export function RelationStatus({ actions }: { actions: Actions }) {
  const relation = useUiStore((s) => s.relation);
  const outcome = useShellStore((s) => s.rest?.status?.pane_focus_request);
  if (!relation) return null;
  const state = relationState(relation, outcome);
  if (!state) return null;
  if (state.phase === "pending") {
    return (
      <div className="flex shrink-0 items-center gap-sm bg-card px-sm py-xxs text-caption text-muted-foreground" role="status" data-relation-status="pending">
        <span className="min-w-0 flex-1 truncate">Opening {relation.label}…</span>
        <Button variant="ghost" size="sm" className="h-auto shrink-0 px-none text-muted-foreground hover:bg-transparent hover:text-foreground" data-relation-dismiss="true" onClick={() => actions.dismissRelation()}>
          Dismiss
        </Button>
      </div>
    );
  }
  return (
    <div className="flex shrink-0 items-center gap-sm bg-card px-sm py-xxs text-caption" role="alert" data-relation-status="failed">
      <Hint label={state.message} reveals>
      <span className="min-w-0 flex-1 truncate text-destructive">
        {state.message}
      </span>
      </Hint>
      {state.retryable ? (
        <Button variant="ghost" size="sm" className="h-auto shrink-0 px-none text-subtle-foreground hover:bg-transparent hover:text-foreground" data-relation-retry="true" onClick={() => actions.retryRelation()}>
          Retry
        </Button>
      ) : null}
      <Button variant="ghost" size="sm" className="h-auto shrink-0 px-none text-muted-foreground hover:bg-transparent hover:text-foreground" data-relation-dismiss="true" onClick={() => actions.dismissRelation()}>
        Dismiss
      </Button>
    </div>
  );
}

type PaneMenuId = `open:${string}` | "sleep_agent" | "copy_name" | "close_pane";

/**
 * What the pane header offers about this pane (B16, B18): its relatives,
 * each opened only by choosing it, its name to copy, and closing it. Opening
 * the menu moves no focus and marks nothing read.
 */
export function paneMenuItems(pane: PaneRow, title: string): MenuEntry<PaneMenuId>[] {
  const relationLabel = { parent: "Open parent", sibling: "Open sibling", child: "Open child" } as const;
  const items: MenuEntry<PaneMenuId>[] = relationEntries(pane).map((entry) => ({
    id: `open:${entry.paneId}` as const,
    label: `${relationLabel[entry.relation]}: ${entry.label}`,
    unavailable: null,
  }));
  // Sleep agent (PRD agent-sleep B15): offered on a local agent pane that is
  // awake, disabled with the core's reason when this agent cannot sleep now.
  if (pane.sleep_action) {
    items.push({
      id: "sleep_agent",
      label: "Sleep agent",
      unavailable: pane.sleep_action.available ? null : (pane.sleep_action.reason ?? "This agent cannot sleep now"),
      separated: items.length > 0,
    });
  }
  items.push({ id: "copy_name", label: "Copy pane name", unavailable: null, separated: items.length > 0 && !pane.sleep_action });
  items.push({ id: "close_pane", label: `Close pane ${title}`, unavailable: null, separated: true });
  return items;
}

export function usePaneMenu(pane: PaneRow, title: string, actions: Actions) {
  const [open, setOpen] = useState<{ x: number; y: number } | null>(null);
  const select = (id: PaneMenuId) => {
    if (id === "copy_name") {
      void navigator.clipboard?.writeText(title).catch(() => undefined);
      return;
    }
    if (id === "close_pane") return actions.closePane(pane.id);
    if (id === "sleep_agent") return actions.sleepAgent(pane.id);
    const target = relationEntries(pane).find((entry) => `open:${entry.paneId}` === id);
    if (target) actions.followRelation(pane.id, target.paneId, target.label);
  };
  const menu = <EntryPointMenu label={`Pane ${title}`} items={paneMenuItems(pane, title)} onSelect={select} at={open} onClose={() => setOpen(null)} />;
  return { menu, openAt: (x: number, y: number) => setOpen({ x, y }) };
}

function useRelationPending(sourcePaneId: string, targetPaneId: string | null): boolean {
  const relation = useUiStore((s) => s.relation);
  const outcome = useShellStore((s) => s.rest?.status?.pane_focus_request);
  if (!targetPaneId || relation?.sourcePaneId !== sourcePaneId || relation.targetPaneId !== targetPaneId) return false;
  return relationState(relation, outcome)?.phase === "pending";
}
