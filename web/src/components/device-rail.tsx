import { InboxIcon, LaptopIcon, PlusIcon, ServerIcon, XIcon } from "lucide-react";
import { memo, useMemo, type ReactNode } from "react";
import type { Actions } from "../actions";
import { deviceBadge, deviceConnected, frontDeviceId, inboxBadge, INBOX, tileName } from "../devices";
import { cn } from "../lib/utils";
import { useShellStore } from "../store";
import { useUiStore } from "../ui";
import { Hint } from "./ui/tooltip";

export type Tile = { id: string; label: string; icon: "inbox" | "local" | "remote" };

/**
 * The rail on the sidebar's left while at least one remote device is
 * registered (PRD home-device-rail D-09..D-11, D-27): the Inbox on top, a
 * divider, This Mac, each registered device, and `+` at the bottom, which is
 * Add device. One tile is selected, with a bar at its left edge. A tile
 * reads only its own facts (its Needs You count, whether it is connected), so
 * a snapshot that changes another device redraws no tile, and nothing here
 * touches the sidebar's rows.
 */
export function DeviceRail({ actions }: { actions: Actions }) {
  const devices = useShellStore((s) => s.rest?.navigator?.devices);
  const inbox = useUiStore((s) => s.inbox);
  const front = useShellStore((s) => frontDeviceId(s.rest));
  const tiles = useMemo<Tile[]>(() => {
    const list = devices ?? [];
    const local = list.find((device) => device.kind !== "remote");
    return [
      { id: INBOX, label: "Inbox", icon: "inbox" },
      { id: local?.id ?? "local", label: local?.label ?? "This Mac", icon: "local" },
      ...list.filter((device) => device.kind === "remote").map((device): Tile => ({ id: device.id, label: device.label, icon: "remote" })),
    ];
  }, [devices]);
  return (
    <nav aria-label="Devices" data-device-rail="true" className="flex w-(--size-rail) shrink-0 flex-col items-stretch border-r border-border py-xs">
      <ul className="flex min-h-0 flex-1 flex-col gap-xs overflow-y-auto overflow-x-hidden">
        {tiles.map((tile, index) => (
          <li key={tile.id} className="flex flex-col items-stretch">
            <RailTile tile={tile} selected={tile.id === INBOX ? inbox : !inbox && tile.id === front} actions={actions} />
            {index === 0 ? <span aria-hidden="true" data-rail-divider="true" className="mx-sm mt-xs h-(--size-hairline) bg-border" /> : null}
          </li>
        ))}
      </ul>
      <Hint label="기기 추가">
        <button
          type="button"
          aria-label="기기 추가"
          data-rail-add="true"
          className="mx-auto flex flex-col items-center gap-xxs rounded-md text-micro text-muted-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring"
          onClick={() => actions.openAddDevice()}
        >
          <TileFrame>
            <PlusIcon aria-hidden="true" className="size-(--size-icon-lg)" />
          </TileFrame>
          <span className="max-w-full truncate px-xxs">기기 추가</span>
        </button>
      </Hint>
    </nav>
  );
}

/** The rounded square every tile draws its glyph in. */
function TileFrame({ children, dimmed = false, selected = false }: { children: ReactNode; dimmed?: boolean; selected?: boolean }) {
  return (
    <span
      className={cn(
        "relative flex size-(--size-icon-button-standard) items-center justify-center rounded-md border",
        selected ? "border-foreground bg-secondary text-foreground" : "border-border bg-card text-subtle-foreground",
        dimmed && "opacity-50",
      )}
    >
      {children}
    </span>
  );
}

const RailTile = memo(function RailTile({ tile, selected, actions }: { tile: Tile; selected: boolean; actions: Actions }) {
  const inboxTile = tile.icon === "inbox";
  const badge = useShellStore((s) => (inboxTile ? inboxBadge(s.rest, s.agents) : deviceBadge(s.rest, s.agents, tile.id)));
  const connected = useShellStore((s) => inboxTile || deviceConnected(s.rest, tile.id));
  return <RailTileView tile={tile} selected={selected} badge={badge} connected={connected} onSelect={() => (inboxTile ? actions.showInbox() : actions.focusDevice(tile.id))} />;
});

/** One tile as drawn: a focusable button named by its device and state, with a left bar while selected, a count, and a cross while not connected (B3, B8, B41). */
export function RailTileView({ tile, selected, badge, connected, onSelect }: { tile: Tile; selected: boolean; badge: number | null; connected: boolean; onSelect: () => void }) {
  const inboxTile = tile.icon === "inbox";
  const remote = tile.icon === "remote";
  const Icon = inboxTile ? InboxIcon : remote ? ServerIcon : LaptopIcon;
  const name = inboxTile ? tileName("Inbox 모든 기기", true, badge) : tileName(tile.label, connected, badge);
  return (
    <Hint label={tile.label}>
      <button
        type="button"
        aria-label={name}
        aria-pressed={selected}
        data-rail-tile={tile.id}
        data-rail-connected={connected ? "true" : "false"}
        className="group/tile relative mx-auto flex w-full flex-col items-center gap-xxs rounded-md px-xxs py-xxs text-micro text-subtle-foreground outline-none focus-visible:ring-1 focus-visible:ring-ring"
        onClick={onSelect}
      >
        {selected ? <span aria-hidden="true" data-rail-bar="true" className="absolute inset-y-0 left-0 my-auto h-(--size-icon-lg) w-(--spacing-xs) rounded-r-xs bg-foreground" /> : null}
        <TileFrame dimmed={!connected} selected={selected}>
          <Icon aria-hidden="true" className="size-(--size-icon-lg)" />
          {badge === null ? null : (
            <span
              aria-hidden="true"
              data-rail-badge={badge}
              className="absolute -top-xs -right-xs flex h-(--size-badge-height) min-w-(--size-badge-height) items-center justify-center rounded-full bg-warning px-xs text-micro font-semibold text-primary-foreground"
            >
              {badge}
            </span>
          )}
          {connected ? null : (
            <span aria-hidden="true" data-rail-off="true" className="absolute -right-xs -bottom-xs flex size-(--size-badge-height) items-center justify-center rounded-full border border-border bg-card text-muted-foreground">
              <XIcon className="size-(--size-icon-sm)" />
            </span>
          )}
        </TileFrame>
        <span className={cn("max-w-full truncate", selected ? "text-foreground" : "", !connected && "opacity-50")} data-rail-label="true">
          {tile.label}
        </span>
      </button>
    </Hint>
  );
}
