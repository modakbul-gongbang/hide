import { useInterfaceTranslation } from "./i18n/client";
import { LaptopIcon, LayoutDashboardIcon } from "lucide-react";
import { memo, useMemo } from "react";
import { useShallow } from "zustand/react/shallow";
import type { Actions } from "./actions";
import { deviceScope } from "./agentScope";
import { deviceConnected, frontDeviceId, tileCounts, tileHint, tileMonogram, tileName } from "./devices";
import { machineName, windowDeviceId, windowFirst } from "./screenMachine";
import { useShellStore } from "./store";
import { overviewDeviceOf } from "./overviewTasks";
import { useUiStore } from "./ui";
import { MainScreen } from "./MainScreen";
import { Button } from "./components/ui/button";
import { Dialog, DialogContent, DialogTitle } from "./components/ui/dialog";
import { Kbd } from "./components/ui/kbd";
import { Hint } from "./components/ui/tooltip";
import { commandLabel } from "./shortcutLabels";
import { cn } from "./lib/utils";

/** The same selected-device group that the Agents sidebar draws, less the Factory's workers, which 내 차례 counts (PRD software-factory-ui B13). */
export function useOverviewCount() {
  return useShellStore((s) => deviceScope(s.rest, frontDeviceId(s.rest))?.overview_needs_you ?? 0);
}

export function OverviewButton({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const count = useOverviewCount();
  const open = useUiStore((s) => s.overviewOpen);
  const chord = useShellStore((s) => commandLabel("overview", s.rest?.ui_state));
  const description = count > 0 ? t("overview.needsYou", { count }) : undefined;
  return <Hint label={count > 0 ? t("overview.needsYouHint", { count }) : t("overview.title")} shortcut={chord}>
    <Button variant={open ? "secondary" : "ghost"} size="icon-sm" className="relative" aria-label={t("overview.title")} aria-description={description} aria-pressed={open} data-open-overview="true" onClick={actions.toggleOverview}>
      <LayoutDashboardIcon />
      {count > 0 ? <span aria-hidden="true" className="absolute right-xxs top-xxs size-(--size-tab-status-dot) rounded-full bg-warning" data-overview-dot="true" /> : null}
    </Button>
  </Hint>;
}

/**
 * The device the Overview shows (PRD overview-device-tasks D-02): the one its
 * own switch picked, else the one a device Home row named, else the rail's
 * front device. A device removed since it was picked leaves the front one.
 */
export function useOverviewDevice(): string {
  const picked = useUiStore((s) => s.overview.deviceId);
  const named = useUiStore((s) => (s.screen?.kind === "main" ? s.screen.deviceId : undefined));
  return useShellStore((s) => overviewDeviceOf(s.rest, picked, named));
}

/**
 * The Overview of one device (B1-B4): a title, the device switch, and the
 * tabs below. As a layer over work or as the page a device's Home row opens;
 * either way the switch changes only what the Overview shows (B2).
 */
export function OverviewPage({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const deviceId = useOverviewDevice();
  return <div className="flex min-h-0 flex-1 flex-col" data-overview-page="true" data-overview-device={deviceId}>
    <div className="flex shrink-0 items-center gap-md border-b border-border px-lg py-sm">
      <h1 className="shrink-0 text-title font-semibold">{t("overview.title")}</h1>
      <DeviceSwitch selected={deviceId} />
      <span className="flex-1" />
      <Button variant="ghost" size="sm" aria-label={t("overview.close")} onClick={actions.closeOverview}><Kbd>Esc</Kbd></Button>
    </div>
    <MainScreen actions={actions} deviceId={deviceId} />
  </div>;
}

/**
 * Every device as its rail tile and name (B2): the rail's front device marked
 * `rail`, one that does not answer `연결 안 됨`, and each hint carries its
 * counts. Picking one changes only the Overview.
 */
function DeviceSwitch({ selected }: { selected: string }) {
  const { t } = useInterfaceTranslation();
  const devices = useShellStore((s) => s.rest?.navigator?.devices);
  const rows = useMemo(() => {
    const own = windowDeviceId(devices);
    return windowFirst(devices).map((device) => ({ id: device.id, own: device.id === own, label: machineName(devices, device.id, t) }));
  }, [devices, t]);
  if (rows.length < 2) return null;
  return <div role="radiogroup" aria-label={t("overviewTasks.devices")} className="flex min-w-0 items-center gap-xxs overflow-x-auto rounded-md border border-border p-xxs" data-overview-devices="true">
    {rows.map((row) => <DeviceChoice key={row.id} id={row.id} own={row.own} label={row.label} selected={row.id === selected} />)}
  </div>;
}

const DeviceChoice = memo(function DeviceChoice({ id, own, label, selected }: { id: string; own: boolean; label: string; selected: boolean }) {
  const { t } = useInterfaceTranslation();
  const counts = useShellStore(useShallow((s) => tileCounts(s.rest, s.agents, id)));
  const connected = useShellStore((s) => deviceConnected(s.rest, id));
  const front = useShellStore((s) => frontDeviceId(s.rest) === id);
  const name = [tileName(label, connected, counts, t), front ? t("overviewTasks.railHint") : null].filter(Boolean).join(", ");
  return <Hint label={tileHint(label, connected, counts, t)}>
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      aria-label={name}
      data-overview-device-choice={id}
      data-connected={connected ? "true" : "false"}
      className={cn("flex shrink-0 items-center gap-xs rounded-sm px-sm py-xxs text-caption outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring", selected ? "bg-secondary text-foreground" : "text-subtle-foreground")}
      onClick={() => useUiStore.getState().setOverview({ deviceId: id })}
    >
      <span aria-hidden="true" className={cn("relative flex size-(--size-icon-lg) items-center justify-center rounded-xs bg-secondary text-micro font-semibold", !connected && "opacity-(--opacity-dimmed)")}>
        {own ? <LaptopIcon className="size-(--size-icon)" /> : tileMonogram(label)}
        {connected && counts.needs_you > 0 ? <span className="absolute -right-xxs -top-xxs size-(--size-tab-status-dot) rounded-full bg-warning" data-device-choice-dot="true" /> : null}
      </span>
      <span className="font-medium">{label}</span>
      {front ? <span aria-hidden="true" className="rounded-xs bg-muted px-xxs text-micro text-muted-foreground" data-device-choice-rail="true">{t("overviewTasks.rail")}</span> : null}
      {!connected ? <span aria-hidden="true" className="text-micro text-muted-foreground" data-device-choice-offline="true">{t("devices.rail.notConnected")}</span> : null}
    </button>
  </Hint>;
});

export function OverviewModal({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const open = useUiStore((s) => s.overviewOpen);
  return <Dialog baseEscape open={open} onOpenChange={(next) => { if (!next) actions.closeOverview(); }}>
    {open ? <DialogContent initialFocus="container" aria-describedby={undefined} className="h-[calc(100%-2*var(--size-settings-sheet-window-inset))] max-h-(--size-overview-sheet-h) w-[calc(100%-2*var(--size-settings-sheet-window-inset))] max-w-(--size-overview-sheet-w)" data-overview-modal="true" returnFocusTo={() => useUiStore.getState().overviewReturnFocus}>
      <DialogTitle className="sr-only">{t("overview.title")}</DialogTitle>
      <OverviewPage actions={actions} />
    </DialogContent> : null}
  </Dialog>;
}
