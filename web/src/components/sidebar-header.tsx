import { useInterfaceTranslation } from "../i18n/client";
import { ChevronDownIcon, LaptopIcon, LayoutDashboardIcon, PlusIcon, SearchIcon, ServerIcon } from "lucide-react";
import type { ReactNode } from "react";
import { Kbd } from "./ui/kbd";
import { Tabs, TabsList, TabsTrigger } from "./ui/tabs";
import { SIDEBAR_MODES, type SidebarMode } from "../ui";
import { Button } from "./ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuSeparator, DropdownMenuTrigger } from "./ui/dropdown-menu";
import { Hint } from "./ui/tooltip";

const MODE_LABEL: Record<SidebarMode, "overview.projects" | "overview.agents"> = { projects: "overview.projects", agents: "overview.agents" };

/** A device the hidden rail's menu lists. */
export type MenuDevice = { id: string; label: string; remote: boolean; connected: boolean };

/** What the top-line name opens while the rail is hidden: the way to every device and back to the rail. */
export type DeviceMenu = {
  devices: readonly MenuDevice[];
  frontId: string;
  onSelect: (deviceId: string) => void;
  onAddDevice: () => void;
  onShowRail: () => void;
};

/**
 * The top of the sidebar (quick device-rail-badges, replacing PRD
 * home-device-rail D-13, D-14). Every device's sidebar is the same three lines:
 * the name of the device in front (`This Mac`, `mini Remote`) with Add project
 * and Search at its right end, the shared Overview row, then `Projects | Agents`. While
 * the rail is hidden the name is a menu that lists the devices, Add device
 * and Show rail, since the rail is no longer the way to another device. A
 * device that cannot be read shows its name alone: it has no list to switch.
 * Drawn from what the caller reads out of the stores, so a test renders it
 * without them.
 */
export function SidebarHeader({
  rail,
  title,
  addProject,
  tabs,
  mode,
  projectChord,
  agentChord,
  overview,
  compact = false,
  searchChord,
  newWorkspaceChord,
  deviceMenu,
  onMode,
  onSearch,
  onNewWorkspace,
}: {
  /** The device rail is showing; hidden, the name opens the device menu. */
  rail: boolean;
  /** The device in front, and the smaller word after it. */
  title: { name: string; note: string | null };
  /** Add project is offered on this line (not on the Agents tab). */
  addProject: boolean;
  /** The Projects | Agents strip is drawn (not for a device that cannot be read). */
  tabs: boolean;
  mode: SidebarMode;
  compact?: boolean;
  /** The current direct Projects and Agents shortcuts. */
  projectChord?: string | null;
  agentChord?: string | null;
  overview?: { selected: boolean; count: number; chord: string | null; onOpen: () => void };
  searchChord: string | null;
  newWorkspaceChord: string | null;
  deviceMenu: DeviceMenu;
  onMode: (mode: SidebarMode) => void;
  onSearch: () => void;
  /** Add project; null where the host cannot pick a folder (a browser tab). */
  onNewWorkspace: (() => void) | null;
}) {
  const { t } = useInterfaceTranslation();
  const add =
    addProject && onNewWorkspace ? (
      <Hint label={t("commands.new_workspace")} shortcut={newWorkspaceChord}>
        <Button variant="ghost" size="icon-sm" data-sidebar-new-workspace="true" onClick={onNewWorkspace}>
          <PlusIcon />
        </Button>
      </Hint>
    ) : null;
  const search = (
    <Hint label={t("common.search")} shortcut={searchChord}>
      <Button variant="ghost" size="icon-sm" data-sidebar-search="true" onClick={onSearch}>
        <SearchIcon />
      </Button>
    </Hint>
  );
  const name = (
    <span className="min-w-0 truncate text-subhead font-semibold text-foreground" data-sidebar-title-name="true">
      {title.name}
    </span>
  );
  const note = title.note ? <span className="shrink-0 text-caption text-muted-foreground">{title.note}</span> : null;
  return (
    <>
      <div className="flex h-(--size-tab-strip) shrink-0 items-center gap-sm border-b border-border pr-xs pl-md" data-sidebar-title="true">
        {rail ? (
          <span className="flex min-w-0 flex-1 items-baseline gap-sm">
            {name}
            {note}
          </span>
        ) : (
          <HiddenRailMenu title={title} menu={deviceMenu} />
        )}
        {add}
        {search}
      </div>
      {overview ? (
        <Hint label={overview.count > 0 ? t("overview.needsYouHint", { count: overview.count }) : t("overview.title")} shortcut={overview.chord}>
          <button type="button" aria-current={overview.selected ? "page" : undefined} data-sidebar-overview="true" onClick={overview.onOpen}
            className={`flex h-(--size-tab-strip) shrink-0 items-center gap-sm border-b border-border px-md text-body outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring ${overview.selected ? "bg-secondary text-foreground" : "text-subtle-foreground hover:bg-accent"}`}>
            <LayoutDashboardIcon aria-hidden="true" className="size-(--size-icon) shrink-0" />
            <span className="min-w-0 flex-1 truncate text-left">{t("overview.title")}</span>
            {overview.count > 0 ? <span data-overview-count="true" className="text-caption text-warning">{t("overview.askingCount", { count: overview.count })}</span> : null}
            {overview.chord && !compact ? <Kbd className="sidebar-command-keycap">{overview.chord}</Kbd> : null}
          </button>
        </Hint>
      ) : null}
      {tabs ? (
        <Tabs value={mode} onValueChange={(value) => onMode(value as SidebarMode)} className="shrink-0 border-b border-border px-xs py-xxs" data-sidebar-strip="true">
          <TabsList aria-label={t("overview.sidebarView")} className="w-full bg-transparent">
            {SIDEBAR_MODES.map((candidate) => {
              const chord = candidate === "projects" ? projectChord : agentChord;
              return <ModeHint key={candidate} label={t(MODE_LABEL[candidate])} chord={chord ?? null}>
                <TabsTrigger value={candidate} data-sidebar-mode={candidate} className="min-w-0 flex-1 gap-xs px-xs text-caption">
                  <span className="truncate">{t(MODE_LABEL[candidate])}</span>
                  {chord && !compact ? <Kbd className="sidebar-command-keycap">{chord}</Kbd> : null}
                </TabsTrigger>
              </ModeHint>;
            })}
          </TabsList>
        </Tabs>
      ) : null}
    </>
  );
}

/** The device name as a menu while the rail is hidden: every device, Add device, and the way to show the rail again. */
function HiddenRailMenu({ title, menu }: { title: { name: string; note: string | null }; menu: DeviceMenu }) {
  const { t } = useInterfaceTranslation();
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          aria-label={t("sidebar.switchDevice", { name: `${title.name}${title.note ? ` ${title.note}` : ""}` })}
          data-sidebar-device-menu="true"
          className="flex min-w-0 flex-1 items-baseline gap-sm rounded-sm text-left outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          <span className="min-w-0 truncate text-subhead font-semibold text-foreground" data-sidebar-title-name="true">
            {title.name}
          </span>
          {title.note ? <span className="shrink-0 text-caption text-muted-foreground">{title.note}</span> : null}
          <ChevronDownIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0 self-center text-muted-foreground" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" aria-label={t("sidebar.devices")} data-device-menu="true">
        <DropdownMenuRadioGroup value={menu.frontId} onValueChange={menu.onSelect}>
          {menu.devices.map((device) => {
            const Icon = device.remote ? ServerIcon : LaptopIcon;
            return (
              <DropdownMenuRadioItem key={device.id} value={device.id} data-device-menu-item={device.id}>
                <Icon aria-hidden="true" />
                <span className="min-w-0 flex-1 truncate">{device.label}</span>
                {device.connected ? null : <span className="shrink-0 text-caption text-muted-foreground">{t("devices.state.unavailable")}</span>}
              </DropdownMenuRadioItem>
            );
          })}
        </DropdownMenuRadioGroup>
        <DropdownMenuSeparator />
        <DropdownMenuItem data-device-menu-add="true" onSelect={menu.onAddDevice}>
          <PlusIcon aria-hidden="true" />
          {t("sidebar.addDevice")}
        </DropdownMenuItem>
        <DropdownMenuItem data-device-menu-show-rail="true" onSelect={menu.onShowRail}>
          {t("sidebar.showRail")}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/** A tab's hint exists only to show the switch chord, so a tab without one has none. */
function ModeHint({ label, chord, children }: { label: string; chord: string | null; children: ReactNode }) {
  if (!chord) return children;
  return (
    <Hint label={label} shortcut={chord}>
      {children}
    </Hint>
  );
}
