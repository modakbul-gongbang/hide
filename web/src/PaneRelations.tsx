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
import { herdrPaneId } from "./remote";
import type { PaneRow, SnapshotRest, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { copySelection, pasteClipboard, selectAllText } from "./terminals";
import { useUiStore } from "./ui";
import type { TFunction } from "i18next";
import { translate, useInterfaceTranslation } from "./i18n/client";
import { splitLabel } from "./areaLayout";

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
  const { t } = useInterfaceTranslation();
  const parent = parentStep(pane);
  const pending = useRelationPending(pane.id, parent?.pane_id ?? null);
  if (!parent) return null;
  const label = t("panes.relation.return", { name: parent.label });
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
  const { t } = useInterfaceTranslation();
  const chips = directChildren(pane);
  const rest = useShellStore((s) => s.rest);
  const parentLocation = paneLocation(rest, pane.id, t);
  const relation = useUiStore((s) => s.relation);
  const outcome = useShellStore((s) => s.rest?.status?.pane_focus_request);
  if (chips.length === 0) return null;
  const state = relation?.sourcePaneId === pane.id ? relationState(relation, outcome, t) : null;
  return (
    <div
      className="flex h-[var(--size-pane-child-row)] shrink-0 items-center gap-xxs overflow-x-auto overflow-y-hidden whitespace-nowrap bg-card px-sm text-caption"
      role="group"
      aria-label={t("panes.relation.children", { name: pane.identity_label ?? pane.id })}
      data-pane-children={pane.id}
    >
      {chips.map((chip) => {
        const pending = state?.phase === "pending" && relation?.targetPaneId === chip.pane_id;
        const childLocation = paneLocation(rest, chip.pane_id, t);
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
            aria-label={t("panes.relation.openChild", { name: title })}
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

function paneLocation(rest: SnapshotRest | null, paneId: string, t: TFunction<"translation">): { checkout: string; deviceId: string; deviceLabel: string } | null {
  const workspaces: Workspace[] = [
    ...(rest?.navigator?.workspaces ?? []),
    ...(rest?.status?.remote ?? []).filter((status) => status.state === "connected").flatMap((status) => status.session?.workspaces ?? []),
  ];
  for (const workspace of workspaces) {
    for (const checkout of workspace.checkouts) {
      if (!checkout.tabs.some((tab) => tab.panes.some((candidate) => candidate.id === paneId))) continue;
      const deviceLabel = rest?.navigator?.devices?.find((device) => device.id === workspace.device_id)?.label ?? (workspace.device_id === "local" ? t("common.thisMac") : workspace.device_id);
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
  const { t } = useInterfaceTranslation();
  const relation = useUiStore((s) => s.relation);
  const outcome = useShellStore((s) => s.rest?.status?.pane_focus_request);
  if (!relation) return null;
  const state = relationState(relation, outcome, t);
  if (!state) return null;
  if (state.phase === "pending") {
    return (
      <div className="flex shrink-0 items-center gap-sm bg-card px-sm py-xxs text-caption text-muted-foreground" role="status" data-relation-status="pending">
        <span className="min-w-0 flex-1 truncate">{t("panes.relation.opening", { name: relation.label })}</span>
        <Button variant="ghost" size="sm" className="h-auto shrink-0 px-none text-muted-foreground hover:bg-transparent hover:text-foreground" data-relation-dismiss="true" onClick={() => actions.dismissRelation()}>
          {t("workspace.dismiss")}
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
          {t("common.retry")}
        </Button>
      ) : null}
      <Button variant="ghost" size="sm" className="h-auto shrink-0 px-none text-muted-foreground hover:bg-transparent hover:text-foreground" data-relation-dismiss="true" onClick={() => actions.dismissRelation()}>
        {t("workspace.dismiss")}
      </Button>
    </div>
  );
}

type PaneMenuId =
  | `open:${string}`
  | "sleep_agent"
  | "copy_name"
  | "copy_pane_id"
  | "close_pane"
  | "copy"
  | "paste"
  | "select_all"
  | "find"
  | "split_right"
  | "split_down"
  | "toggle_zoom";

/**
 * What the pane header offers about this pane (B16, B18): its relatives,
 * each opened only by choosing it, its name and Herdr id to copy, and closing it. Opening
 * the menu moves no focus and marks nothing read.
 */
export function paneMenuItems(pane: PaneRow, title: string): MenuEntry<PaneMenuId>[] {
  const relationLabel = { parent: "panes.relation.parent", sibling: "panes.relation.sibling", child: "panes.relation.child" } as const;
  const items: MenuEntry<PaneMenuId>[] = relationEntries(pane).map((entry) => ({
    id: `open:${entry.paneId}` as const,
    label: translate(relationLabel[entry.relation], { name: entry.label }),
    unavailable: null,
  }));
  // Sleep agent (PRD agent-sleep B15): offered on a local agent pane that is
  // awake, disabled with the core's reason when this agent cannot sleep now.
  if (pane.sleep_action) {
    items.push({
      id: "sleep_agent",
      label: translate("panes.menu.sleep"),
      unavailable: pane.sleep_action.available ? null : (pane.sleep_action.reason ?? translate("panes.menu.sleepUnavailable")),
      separated: items.length > 0,
    });
  }
  items.push({ id: "copy_name", label: translate("panes.menu.copyName"), unavailable: null, separated: items.length > 0 && !pane.sleep_action });
  items.push({ id: "copy_pane_id", label: translate("panes.menu.copyPaneId"), unavailable: null });
  items.push({ id: "close_pane", label: translate("panes.close", { name: title }), unavailable: null, separated: true });
  return items;
}

/** What a right-click in the terminal knows when it opens. */
export type TerminalMenuContext = {
  /** The pane has a drag selection, so Copy has something to copy. */
  selection: boolean;
  zoomed: boolean;
  /** Panes in the tab, the zoomed one's hidden siblings included. */
  paneCount: number;
  /** The chords the registry and the terminal bind here; "" draws none. */
  chords: { copy: string; paste: string; find: string; splitRight: string; splitDown: string; zoom: string };
};

/**
 * A right-click in the terminal: editing the text first, then the tab's
 * layout around this pane, then everything the header menu offers.
 */
export function terminalMenuItems(pane: PaneRow, title: string, context: TerminalMenuContext): MenuEntry<PaneMenuId>[] {
  const { chords } = context;
  const items: MenuEntry<PaneMenuId>[] = [];
  if (context.selection) items.push({ id: "copy", label: translate("common.copy"), unavailable: null, shortcut: chords.copy });
  items.push(
    { id: "paste", label: translate("panes.menu.paste"), unavailable: null, shortcut: chords.paste },
    { id: "select_all", label: translate("panes.menu.selectAll"), unavailable: null },
    { id: "find", label: translate("panes.menu.find"), unavailable: null, shortcut: chords.find },
    { id: "split_right", label: splitLabel("right"), unavailable: null, separated: true, shortcut: chords.splitRight },
    { id: "split_down", label: splitLabel("down"), unavailable: null, shortcut: chords.splitDown },
    {
      id: "toggle_zoom",
      label: context.zoomed ? translate("panes.unzoom") : translate("panes.menu.zoom"),
      unavailable: context.zoomed || context.paneCount > 1 ? null : translate("panes.menu.onlyPane"),
      shortcut: chords.zoom,
    },
  );
  const paneItems = paneMenuItems(pane, title).map((item, index) => (index === 0 ? { ...item, separated: true } : item));
  return [...items, ...paneItems];
}

type MenuOpen = { x: number; y: number; items: MenuEntry<PaneMenuId>[] };


export function usePaneMenu(pane: PaneRow, title: string, actions: Actions) {
  const { t } = useInterfaceTranslation();
  const [open, setOpen] = useState<MenuOpen | null>(null);
  const select = (id: PaneMenuId) => {
    switch (id) {
      case "copy_name":
        return actions.copyText(title, "pane name");
      case "copy_pane_id":
        return actions.copyText(herdrPaneId(pane.id), "pane id");
      case "close_pane":
        return actions.closePane(pane.id);
      case "sleep_agent":
        return actions.sleepAgent(pane.id);
      case "copy":
        void copySelection(pane.id, "pane menu");
        return;
      case "paste":
        void pasteClipboard(pane.id, "pane menu");
        return;
      case "select_all":
        return selectAllText(pane.id);
      // The right-click focused this pane; each command still names it.
      case "find":
        return actions.openFind(pane.id);
      case "split_right":
        return actions.split("right", pane.id);
      case "split_down":
        return actions.split("down", pane.id);
      case "toggle_zoom":
        return actions.toggleZoom(pane.id);
    }
    const target = relationEntries(pane).find((entry) => `open:${entry.paneId}` === id);
    if (target) actions.followRelation(pane.id, target.paneId, target.label);
  };
  const menu = <EntryPointMenu label={t("panes.menu.aria", { name: title })} items={open?.items ?? []} onSelect={select} at={open} onClose={() => setOpen(null)} />;
  return {
    menu,
    open: open !== null,
    openAt: (x: number, y: number) => setOpen({ x, y, items: paneMenuItems(pane, title) }),
    openTerminalAt: (x: number, y: number, context: TerminalMenuContext) => setOpen({ x, y, items: terminalMenuItems(pane, title, context) }),
  };
}

function useRelationPending(sourcePaneId: string, targetPaneId: string | null): boolean {
  const { t } = useInterfaceTranslation();
  const relation = useUiStore((s) => s.relation);
  const outcome = useShellStore((s) => s.rest?.status?.pane_focus_request);
  if (!targetPaneId || relation?.sourcePaneId !== sourcePaneId || relation.targetPaneId !== targetPaneId) return false;
  return relationState(relation, outcome, t)?.phase === "pending";
}
