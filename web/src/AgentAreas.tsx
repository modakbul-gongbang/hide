import { useMemo, useState } from "react";
import type { Actions } from "./actions";
import { AreaEmpty } from "./AreaEmpty";
import { createAreaTree, type AreaAdapter } from "./AreaTree";
import { AGENT_WORDS, agentMenu, remoteGroupReason, type AgentCommand, type AgentItem, type AgentLayout } from "./agentLayout";
import { noteAreaFrame } from "./areaFrames";
import { Button } from "./components/ui/button";
import { FindBar } from "./Overlays";
import { PaneCanvas } from "./PaneGrid";
import { RelationStatus } from "./PaneRelations";
import { commandLabel } from "./shortcutLabels";
import { localDeviceId, type Checkout } from "./snapshot";
import { useShellStore } from "./store";
import { numberOf, numberedTabs } from "./numbering";
import { useUiStore } from "./ui";
import { AgentTab } from "./TabBar";
import { agentEntries, workspaceViewOf } from "./workspace";
import { areasOf } from "./areaLayout";
import { useKeyboardOwner } from "./viewFocus";
import { useInterfaceTranslation } from "./i18n/client";

const SharedAgentTree = createAreaTree<AgentItem>("agent");
export function AgentAreas({ checkout, actions, deviceId, remoteBody }: { checkout: Checkout; actions: Actions; deviceId?: string; remoteBody?: React.ReactNode }) {
  const { t } = useInterfaceTranslation();
  const [renaming, setRenaming] = useState<string | null>(null);
  const saved = useShellStore((s) => workspaceViewOf(s.rest)?.agent_layout);
  const numbered = useUiStore((s) => s.hint === "tabs");
  const owner = useKeyboardOwner();
  const entries = agentEntries(checkout);
  const paneLayouts = useShellStore((s) => s.rest?.pane_layouts);
  const remoteSessions = useShellStore((s) => s.rest?.status?.remote);
  const node = useShellStore((s) => localDeviceId(s.rest));
  const device = deviceId ?? node;
  const remote = device !== node;
  const remoteLayout = useMemo<AgentLayout>(() => ({
    root: { area: { id: "a1", active: checkout.active_tab_id, displays: agentEntries(checkout).map((entry) => ({ id: entry.source_id })) } },
    active_area: "a1", canvases: {}, limits: { areas: 1, depth: 0, displays: 64 }, display_count: agentEntries(checkout).length,
  }), [checkout]);
  const layout = remote ? remoteLayout : saved;
  if (!layout) return <AreaEmpty state="agent-layout-missing" text={t("panes.agent.waiting")} />;
  const visiblePaneIds = areasOf(layout.root).flatMap(area => {
    const tabId = layout.canvases[area.id] ?? area.active;
    const geometry = remote
      ? remoteSessions?.find(row => row.target_id === device)?.session?.pane_layouts.find(row => row.tab_id === tabId)
      : paneLayouts?.find(row => row.tab_id === tabId);
    if (!geometry) return [];
    if (geometry.zoomed) return [geometry.focused_pane_id];
    return checkout.tabs.find(tab => tab.id === tabId)?.panes.map(pane => pane.id) ?? [];
  });
  const numbers = numbered ? numberedTabs(checkout, layout) : null;
  const workspace = { device_id: device, path: checkout.path };
  const label = (id: string) => entries.find((entry) => entry.source_id === id)?.label ?? id;
  const adapter: AreaAdapter<AgentItem> = {
    words: AGENT_WORDS,
    keyboardArea: owner.kind !== "none" && owner.workspace === checkout.id
      ? owner.kind === "agent" ? owner.areaId ?? null
        : owner.kind === "pane" ? areasOf(layout.root).find((area) => {
          const shown = layout.canvases[area.id] ?? area.active;
          return area.displays.some((tab) => tab.id === shown) && checkout.tabs.find((tab) => tab.id === shown)?.panes.some((pane) => pane.id === owner.paneId);
        })?.id ?? null : null
      : null,
    barAttributes: { "data-tab-bar": checkout.id, ...(remote ? { "data-remote-tab-bar": "true" } : {}) },
    splitUnavailable: remote ? remoteGroupReason() : undefined,
    label: (item) => label(item.id),
    sameContent: (a, b) => a.id === b.id,
    shown: (area) => {
      const id = layout.canvases[area.id] ?? area.active;
      return id ? { id } : null;
    },
    tab: (item, interaction) => {
      const entry = entries.find((row) => row.source_id === item.id);
      return entry ? <AgentTab number={numbers ? numberOf(numbers, item.id) : null} entry={entry} checkout={checkout} renaming={renaming === item.id} onCancelRename={() => setRenaming(null)} interaction={interaction} actions={actions} /> : null;
    },
    body: (item, area) => <>
      {area.id === layout.active_area ? <><RelationStatus actions={actions} visiblePaneIds={visiblePaneIds} /><FindBar actions={actions} /></> : null}
      {remote ? remoteBody : <PaneCanvas key={item.id} tab={checkout.tabs.find((tab) => tab.id === item.id) ?? null} actions={actions} />}
    </>,
    empty: (area) => <AreaEmpty state="no-agent-tab" text={t("panes.agent.noTab")}><Button variant="secondary" onClick={() => actions.createTab(area.id)} data-empty-new-tab="true">{t("panes.area.newTab")}</Button></AreaEmpty>,
    floating: (item) => <span className="truncate">{label(item.id)}</span>,
    menu: (id, geometry, sizes) => agentMenu({ workspace, remote, layout, geometry, sizes }, id),
    runMenu: (command, id) => command === "rename_tab" ? setRenaming(id) : actions.runAgentCommand(command as AgentCommand, id),
    onMenuCloseAutoFocus: (event) => { if (renaming) event.preventDefault(); },
    focus: (id) => remote ? actions.focusTab(id) : actions.agentLayout({ action: "focus", tab_id: id }),
    focusArea: (id) => { if (!remote) actions.agentLayout({ action: "focus_area", area_id: id }); },
    move: (id, areaId, index) => {
      if (remote) {
        const entry = entries.find((row) => row.source_id === id);
        const other = entries.filter((row) => row.source_id !== id);
        const before = other[index];
        const strip = checkout.strip.filter((row) => row.source_id !== id);
        const slot = before ? strip.findIndex((row) => row.id === before.id) : strip.length;
        if (entry) actions.reorderTab(entry.id, slot);
      } else actions.agentLayout({ action: "move", tab_id: id, area_id: areaId, index });
    },
    split: (id, areaId, edge) => actions.agentLayout({ action: "split", tab_id: id, area_id: areaId, edge, request_id: crypto.randomUUID() }),
    resize: (id, ratio) => actions.agentLayout({ action: "resize", split_id: id, ratio }),
    newTab: (areaId) => actions.createTab(areaId),
    newTabLabel: t("panes.agent.newTabNamed", { label: checkout.next_tab_label }),
    tabListLabel: t("panes.agent.tabList"), actionsLabel: t("panes.agent.tabActions"),
    newTabShortcut: commandLabel("new_tab"),
    onDraw: (frame) => noteAreaFrame("agent", frame ? { ...frame, layout, workspace, remote } : null),
  };
  return <SharedAgentTree key={`${deviceId}\0${checkout.path}`} layout={layout} adapter={adapter} />;
}
