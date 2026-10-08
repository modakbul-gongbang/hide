import { useInterfaceTranslation } from "../i18n/client";
import { ChevronDownIcon, CirclePauseIcon, FactoryIcon, FolderGit2Icon, LaptopIcon, LayoutDashboardIcon, PlusIcon, SearchIcon, ServerIcon, SparkleIcon } from "lucide-react";
import type { ReactNode } from "react";
import { Kbd } from "./ui/kbd";
import { Tabs, TabsList, TabsTrigger } from "./ui/tabs";
import { SIDEBAR_MODES, type SidebarMode } from "../ui";
import { Button } from "./ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuSeparator, DropdownMenuTrigger } from "./ui/dropdown-menu";
import { Hint } from "./ui/tooltip";

const MODE_LABEL: Record<SidebarMode, "overview.projects" | "overview.agents"> = { projects: "overview.projects", agents: "overview.agents" };

/** A device the hidden rail's menu lists. */

/** One Factory under the Factory row: its project, its 내 차례 number and whether it is paused (PRD factory-observer B34). */
export type FactoryProjectRow = { id: string; name: string; count: number; paused: boolean; selected: boolean; onOpen: () => void };
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
  factory,
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
  /**
   * The Factory place (PRD software-factory-ui D-02, B1, B12, B23): its row
   * with the 내 차례 number, and the secretary's row under it once a Factory
   * exists; `status` is the secretary agent's mark while its pane is listed.
   */
  factory?: { selected: boolean; count: number; chord: string | null; onOpen: () => void; projects: FactoryProjectRow[]; secretary: { status: ReactNode; onOpen: () => void } | null };
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
      {factory ? (
        <>
          <Hint label={factory.count > 0 ? t("factory.sidebar.turnHint", { count: factory.count }) : t("factory.title")} shortcut={factory.chord}>
            <button type="button" aria-current={factory.selected ? "page" : undefined} data-sidebar-factory="true" onClick={factory.onOpen}
              className={`flex h-(--size-tab-strip) shrink-0 items-center gap-sm px-md text-body outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring ${factory.secretary ? "" : "border-b border-border"} ${factory.selected ? "bg-secondary text-foreground" : "text-subtle-foreground hover:bg-accent"}`}>
              <FactoryIcon aria-hidden="true" className="size-(--size-icon) shrink-0" />
              <span className="min-w-0 flex-1 truncate text-left">{t("factory.title")}</span>
              {factory.count > 0 ? <span data-factory-badge={factory.count} className="text-caption text-warning">{factory.count}</span> : null}
              {factory.chord && !compact ? <Kbd className="sidebar-command-keycap">{factory.chord}</Kbd> : null}
            </button>
          </Hint>
          {factory.projects.map((project) => (
            <Hint key={project.id} label={project.paused ? `${project.name} · ${t("factory.pausedChip")}` : project.name}>
              <button type="button" aria-current={project.selected ? "page" : undefined} data-sidebar-factory-project={project.id} data-factory-paused={project.paused ? "true" : undefined} onClick={project.onOpen}
                className={`flex h-(--size-tab-strip) shrink-0 items-center gap-sm pr-md pl-xl text-body outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring ${project.selected ? "bg-secondary text-foreground" : "text-subtle-foreground hover:bg-accent"}`}>
                <FolderGit2Icon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
                <span className="min-w-0 flex-1 truncate text-left">{project.name}</span>
                {project.paused ? <CirclePauseIcon aria-label={t("factory.pausedChip")} className="size-(--size-icon-sm) shrink-0 text-muted-foreground" /> : null}
                {project.count > 0 ? <span className="text-caption text-warning">{project.count}</span> : null}
              </button>
            </Hint>
          ))}
          {factory.secretary ? (
            <button type="button" data-sidebar-secretary="true" onClick={factory.secretary.onOpen}
              className="flex h-(--size-tab-strip) shrink-0 items-center gap-sm border-b border-border pr-md pl-xl text-body text-subtle-foreground outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring">
              <span className="flex w-(--size-agent-mark) shrink-0 justify-center">{factory.secretary.status ?? <SparkleIcon aria-hidden="true" className="size-(--size-icon-sm) text-muted-foreground" />}</span>
              <span className="min-w-0 flex-1 truncate text-left">{t("factory.secretary.name")}</span>
            </button>
          ) : null}
        </>
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
