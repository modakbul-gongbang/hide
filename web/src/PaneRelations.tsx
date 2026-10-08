import { CornerUpLeftIcon } from "lucide-react";
import { useState } from "react";
import type { Actions } from "./actions";
import { Button } from "./components/ui/button";
import { Hint } from "./components/ui/tooltip";
import { EntryPointMenu, type MenuEntry } from "./components/entry-menu";
import { parentStep, relationEntries, relationState } from "./lineage";
import { herdrPaneId } from "./remote";
import { type PaneRow } from "./snapshot";
import { useShellStore } from "./store";
import { copySelection, pasteClipboard, selectAllText } from "./terminals";
import { useUiStore } from "./ui";
import { translate, useInterfaceTranslation } from "./i18n/client";
import { splitLabel } from "./areaLayout";

// The delegation tree where the operator works (PRD S6 D-07, B14-B16): a
// child pane's header returns to its parent, a parent's badge lists every
// direct child in a popover, and the pane menu lists parent,
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
  const progress = useRelationProgress(pane.id, parent?.pane_id ?? null);
  const pending = progress?.phase === "pending";
  if (!parent) return null;
  const label = t("panes.relation.return", { name: parent.label });
  return (
    <Hint label={progress?.phase === "failed" ? progress.message : label}>
      <Button
        variant="ghost"
        size="icon-sm"
        className={`w-auto shrink-0 gap-xs px-xs hover:bg-popover hover:text-foreground ${progress?.phase === "failed" ? "text-destructive" : ""}`}
        aria-label={label}
        aria-busy={pending}
        data-pane-return={parent.pane_id}
        disabled={pending || (progress?.phase === "failed" && !progress.retryable)}
        onClick={() => actions.followRelation(pane.id, parent.pane_id, parent.label)}
      >
        {pending ? <span aria-hidden="true">…</span> : <CornerUpLeftIcon />}
        <span className="hidden max-w-[var(--size-pane-child-chip-max)] truncate @min-[var(--size-pane-parent-breakpoint)]/pane:inline">{progress?.phase === "failed" && progress.retryable ? t("common.retry") : parent.label}</span>
      </Button>
    </Hint>
  );
}

/**
 * The relationship focus the operator asked for, while it is in flight or
 * after it failed (B15). A visible source band owns this feedback; the Agent
 * area keeps it reachable when the core has moved that source out of view.
 */
export function RelationStatus({ actions, visiblePaneIds }: { actions: Actions; visiblePaneIds: readonly string[] }) {
  const { t } = useInterfaceTranslation();
  const relation = useUiStore((s) => s.relation);
  const outcome = useShellStore((s) => s.rest?.status?.pane_focus_request);
  const band = useShellStore((s) => s.rest?.terminal?.headers?.[relation?.sourcePaneId ?? ""]?.band);
  if (!relation) return null;
  // A visible source band owns its move's feedback. Retain the existing
  // fallback only when that band is no longer on screen or cannot show it.
  if (visiblePaneIds.includes(relation.sourcePaneId) && band?.action?.kind === "child" && band.action.pane_id === relation.targetPaneId) return null;
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

function useRelationProgress(sourcePaneId: string, targetPaneId: string | null) {
  const { t } = useInterfaceTranslation();
  const relation = useUiStore((s) => s.relation);
  const outcome = useShellStore((s) => s.rest?.status?.pane_focus_request);
  if (!targetPaneId || relation?.sourcePaneId !== sourcePaneId || relation.targetPaneId !== targetPaneId) return null;
  return relationState(relation, outcome, t);
}
