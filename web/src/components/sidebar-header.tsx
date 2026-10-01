import { CheckIcon, ChevronDownIcon, LaptopIcon, PlusIcon, SearchIcon, ServerIcon } from "lucide-react";
import type { ReactNode } from "react";
import { SIDEBAR_MODES, type SidebarMode } from "../ui";
import { Button } from "./ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "./ui/dropdown-menu";
import { Hint } from "./ui/tooltip";

const MODE_LABEL: Record<SidebarMode, string> = { projects: "Projects", agents: "Agents" };

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
 * home-device-rail D-13, D-14). Every device's sidebar is the same two lines:
 * the name of the device in front (`This Mac`, `mini Remote`) with Add project
 * and Search at its right end, then the `Projects | Agents` tab strip. While
 * the rail is hidden the name is a menu that lists the devices, `기기 추가…`
 * and `레일 표시`, since the rail is no longer the way to another device. A
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
  switchChord,
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
  /** Add project is offered on this line (on the Projects tab of a device that can be read). */
  addProject: boolean;
  /** The Projects | Agents strip is drawn (not for a device that cannot be read). */
  tabs: boolean;
  mode: SidebarMode;
  /** The bound `toggle_sidebar_view` chord; it has none until the operator binds one. */
  switchChord: string | null;
  searchChord: string | null;
  newWorkspaceChord: string | null;
  deviceMenu: DeviceMenu;
  onMode: (mode: SidebarMode) => void;
  onSearch: () => void;
  /** Add project; null where the host cannot pick a folder (a browser tab). */
  onNewWorkspace: (() => void) | null;
}) {
  const add =
    addProject && onNewWorkspace ? (
      <Hint label="Add project" shortcut={newWorkspaceChord}>
        <Button variant="ghost" size="icon-sm" data-sidebar-new-workspace="true" onClick={onNewWorkspace}>
          <PlusIcon />
        </Button>
      </Hint>
    ) : null;
  const search = (
    <Hint label="Search" shortcut={searchChord}>
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
      {tabs ? (
        <div className="flex h-(--size-tab-strip) shrink-0 items-center gap-sm border-b border-border pr-xs pl-md text-caption" data-sidebar-strip="true">
          {SIDEBAR_MODES.map((candidate) => (
            <ModeHint key={candidate} label={MODE_LABEL[candidate]} chord={switchChord}>
              <button
                type="button"
                data-sidebar-mode={candidate}
                aria-pressed={mode === candidate}
                className={mode === candidate ? "text-foreground" : "text-muted-foreground hover:text-subtle-foreground"}
                onClick={() => onMode(candidate)}
              >
                {MODE_LABEL[candidate]}
              </button>
            </ModeHint>
          ))}
        </div>
      ) : null}
    </>
  );
}

/** The device name as a menu while the rail is hidden: every device, Add device, and the way to show the rail again. */
function HiddenRailMenu({ title, menu }: { title: { name: string; note: string | null }; menu: DeviceMenu }) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          aria-label={`${title.name}, 기기 전환`}
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
      <DropdownMenuContent align="start" aria-label="Devices" data-device-menu="true">
        {menu.devices.map((device) => {
          const Icon = device.remote ? ServerIcon : LaptopIcon;
          return (
            <DropdownMenuItem key={device.id} data-device-menu-item={device.id} onSelect={() => menu.onSelect(device.id)}>
              <Icon aria-hidden="true" />
              <span className="min-w-0 flex-1 truncate">{device.label}</span>
              {device.connected ? null : <span className="shrink-0 text-caption text-muted-foreground">연결 안 됨</span>}
              {device.id === menu.frontId ? <CheckIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0 text-foreground" /> : null}
            </DropdownMenuItem>
          );
        })}
        <DropdownMenuSeparator />
        <DropdownMenuItem data-device-menu-add="true" onSelect={menu.onAddDevice}>
          <PlusIcon aria-hidden="true" />
          기기 추가…
        </DropdownMenuItem>
        <DropdownMenuItem data-device-menu-show-rail="true" onSelect={menu.onShowRail}>
          레일 표시
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
