import { LaptopIcon, PlusIcon, ServerIcon, XIcon } from "lucide-react";
import { memo, useMemo, type ReactNode } from "react";
import { useShallow } from "zustand/react/shallow";
import type { Actions } from "../actions";
import { badgeText, deviceConnected, deviceStateCounts, frontDeviceId, tileBadges, tileName, type BadgeState, type StateCounts, type TileBadge } from "../devices";
import { cn } from "../lib/utils";
import { useShellStore } from "../store";
import { EntryContextMenu } from "./entry-menu";
import { Hint } from "./ui/tooltip";

export type Tile = { id: string; label: string; icon: "local" | "remote" };

/** A circle's fill, by the state it counts: the colors the Agents tab's counts wear as text. */
const BADGE_FILL: Record<BadgeState, string> = { needs_you: "bg-warning", done: "bg-success", working: "bg-agent-working" };

/**
 * The rail on the sidebar's left (PRD home-device-rail D-09, reworked by quick
 * device-rail-badges): This Mac, each registered device in the core's order,
 * and `+` directly under the last one, which is Add device. It is shown
 * with one device alone, and a right-click on it offers `레일 숨기기`. One tile
 * is selected, with a bar at its left edge. A tile reads only its own facts
 * (its three counts, whether it is connected), so a snapshot that changes
 * another device redraws no tile, and nothing here touches the sidebar's rows.
 */
export function DeviceRail({ actions }: { actions: Actions }) {
  const devices = useShellStore((s) => s.rest?.navigator?.devices);
  const front = useShellStore((s) => frontDeviceId(s.rest));
  const tiles = useMemo<Tile[]>(() => {
    const list = devices ?? [];
    const local = list.find((device) => device.kind !== "remote");
    return [
      { id: local?.id ?? "local", label: local?.label ?? "This Mac", icon: "local" },
      ...list.filter((device) => device.kind === "remote").map((device): Tile => ({ id: device.id, label: device.label, icon: "remote" })),
    ];
  }, [devices]);
  return (
    <EntryContextMenu
      asChild
      label="Device rail"
      items={() => [{ id: "hide", label: "레일 숨기기", unavailable: null }]}
      onSelect={() => actions.hideDeviceRail()}
      data-device-rail-menu="true"
    >
      <nav aria-label="Devices" data-device-rail="true" className="flex w-(--size-rail) shrink-0 flex-col items-stretch overflow-y-auto overflow-x-hidden border-r border-border py-xs">
        <ul className="flex flex-col gap-xs">
          {tiles.map((tile) => (
            <li key={tile.id} className="flex flex-col items-stretch">
              <RailTile tile={tile} selected={tile.id === front} actions={actions} />
            </li>
          ))}
          <li className="flex flex-col items-stretch">
            <Hint label="기기 추가">
              <button
                type="button"
                aria-label="기기 추가"
                data-rail-add="true"
                className="mx-auto flex w-full flex-col items-center gap-xxs rounded-md px-xxs py-xxs text-micro text-muted-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring"
                onClick={() => actions.openAddDevice()}
              >
                <TileRow>
                  <TileFrame>
                    <PlusIcon aria-hidden="true" className="size-(--size-icon-lg)" />
                  </TileFrame>
                </TileRow>
                <span className="max-w-full truncate px-xxs">기기 추가</span>
              </button>
            </Hint>
          </li>
        </ul>
      </nav>
    </EntryContextMenu>
  );
}

/**
 * The frame and the circles' column side by side, the column always reserved
 * so every frame stands at the same place whatever its device reports. The
 * row is as tall as three circles with their gaps, so a tile does not grow
 * when a second or third state appears and the label stays under the stack.
 */
function TileRow({ children, badges }: { children: ReactNode; badges?: ReactNode }) {
  return (
    <span className="flex min-h-[calc(3*var(--size-badge-height)+2*var(--spacing-xxs))] items-start gap-xxs">
      {children}
      <span className="flex w-(--size-badge-wide) flex-col items-start gap-xxs">{badges}</span>
    </span>
  );
}

/** The rounded square every tile draws its glyph in. */
function TileFrame({ children, dimmed = false, selected = false }: { children: ReactNode; dimmed?: boolean; selected?: boolean }) {
  return (
    <span
      className={cn(
        "relative flex size-(--size-icon-button-standard) shrink-0 items-center justify-center rounded-md border",
        selected ? "border-foreground bg-secondary text-foreground" : "border-border bg-card text-subtle-foreground",
        dimmed && "opacity-50",
      )}
    >
      {children}
    </span>
  );
}

const RailTile = memo(function RailTile({ tile, selected, actions }: { tile: Tile; selected: boolean; actions: Actions }) {
  const counts = useShellStore(useShallow((s) => deviceStateCounts(s.rest, s.agents, tile.id)));
  const connected = useShellStore((s) => deviceConnected(s.rest, tile.id));
  return <RailTileView tile={tile} selected={selected} counts={counts} connected={connected} onSelect={() => actions.focusDevice(tile.id)} />;
});

/**
 * One tile as drawn: a focusable button named by its device, its connection
 * and each non-zero count, with a left bar while selected, up to three circles
 * stacked down from the frame's top right in the order Needs You, Done,
 * Working (a zero takes no slot), and a cross while not connected, which also
 * shows no circle since what it last reported is not current (B3, B8, B41).
 */
export function RailTileView({ tile, selected, counts, connected, onSelect }: { tile: Tile; selected: boolean; counts: StateCounts; connected: boolean; onSelect: () => void }) {
  const remote = tile.icon === "remote";
  const Icon = remote ? ServerIcon : LaptopIcon;
  const badges = connected ? tileBadges(counts) : [];
  return (
    <Hint label={tile.label}>
      <button
        type="button"
        aria-label={tileName(tile.label, connected, badges)}
        aria-pressed={selected}
        data-rail-tile={tile.id}
        data-rail-connected={connected ? "true" : "false"}
        className="group/tile relative mx-auto flex w-full flex-col items-center gap-xxs rounded-md px-xxs py-xxs text-micro text-subtle-foreground outline-none focus-visible:ring-1 focus-visible:ring-ring"
        onClick={onSelect}
      >
        {selected ? <span aria-hidden="true" data-rail-bar="true" className="absolute inset-y-0 left-0 my-auto h-(--size-icon-lg) w-(--spacing-xs) rounded-r-xs bg-foreground" /> : null}
        <TileRow badges={badges.map((badge) => <BadgeCircle key={badge.state} badge={badge} />)}>
          <TileFrame dimmed={!connected} selected={selected}>
            <Icon aria-hidden="true" className="size-(--size-icon-lg)" />
            {connected ? null : (
              <span aria-hidden="true" data-rail-off="true" className="absolute -right-xs -bottom-xs flex size-(--size-badge-height) items-center justify-center rounded-full border border-border bg-card text-muted-foreground">
                <XIcon className="size-(--size-icon-sm)" />
              </span>
            )}
          </TileFrame>
        </TileRow>
        <span className={cn("max-w-full truncate", selected ? "text-foreground" : "", !connected && "opacity-50")} data-rail-label="true">
          {tile.label}
        </span>
      </button>
    </Hint>
  );
}

/** One state's count as a badge-height circle, or a wide pill reading `9+` from ten. */
function BadgeCircle({ badge }: { badge: TileBadge }) {
  return (
    <span
      aria-hidden="true"
      data-rail-badge={badge.state}
      data-rail-badge-count={badge.count}
      className={cn(
        "flex h-(--size-badge-height) items-center justify-center rounded-full text-micro leading-none font-semibold text-primary-foreground tabular-nums",
        badge.count >= 10 ? "w-(--size-badge-wide)" : "w-(--size-badge-height)",
        BADGE_FILL[badge.state],
      )}
    >
      {badgeText(badge.count)}
    </span>
  );
}
