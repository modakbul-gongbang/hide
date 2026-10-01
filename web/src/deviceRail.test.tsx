import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { RailTileView, type Tile } from "./components/device-rail";
import { TooltipProvider } from "./components/ui/tooltip";
import { sidebarBody, type StateCounts } from "./devices";
import type { SnapshotRest } from "./snapshot";

const LOCAL = { id: "local", label: "This Mac", kind: "local", state: "local", message: null };
const MINI = { id: "mini", label: "mini", kind: "remote", state: "ready", message: null };
const OFF = { id: "build-box", label: "연구실 빌드 서버 자동화 장비", kind: "remote", state: "unavailable", message: null };

function world(devices: unknown[], front = "local", connected = ["mini"]): SnapshotRest {
  return {
    navigator: { focused_device_id: front, devices },
    status: { remote: [MINI, OFF].map((device) => ({ target_id: device.id, state: connected.includes(device.id) ? "connected" : "stale", session: null })) },
  } as unknown as SnapshotRest;
}

const tile = (html: string) => html.match(/<button[^>]*data-rail-tile[^>]*>/)?.[0] ?? "";

const NONE: StateCounts = { needs_you: 0, done: 0, working: 0 };

function view(props: { tile: Tile; selected?: boolean; counts?: StateCounts; connected?: boolean }) {
  return renderToStaticMarkup(
    <TooltipProvider>
      <RailTileView selected={false} counts={NONE} connected onSelect={() => undefined} {...props} />
    </TooltipProvider>,
  );
}

/** The state of each circle drawn, top to bottom, with its text. */
const circles = (html: string) => [...html.matchAll(/data-rail-badge="(\w+)" data-rail-badge-count="(\d+)"[^>]*>([^<]*)</g)].map((match) => [match[1], match[3]]);

describe("which list fills the sidebar (quick device-rail-badges B3)", () => {
  it("is the chosen tab for the device in front, one device or many", () => {
    expect(sidebarBody(world([LOCAL]), "projects")).toBe("projects");
    expect(sidebarBody(world([LOCAL]), "agents")).toBe("agents");
    expect(sidebarBody(world([LOCAL, MINI], "mini"), "agents")).toBe("agents");
    expect(sidebarBody(world([LOCAL, MINI]), "projects")).toBe("projects");
  });

  it("is the reconnect view for a front device that is not connected, even when it is the only remote one", () => {
    expect(sidebarBody(world([LOCAL, OFF], "build-box"), "projects")).toBe("disconnected");
    expect(sidebarBody(world([LOCAL, MINI], "mini", []), "agents")).toBe("disconnected");
  });
});

describe("a rail tile", () => {
  const mini: Tile = { id: "mini", label: "mini", icon: "remote" };
  const off: Tile = { id: "build-box", label: OFF.label, icon: "remote" };

  it("is a focusable button whose name carries the device and each non-zero count (B4)", () => {
    const html = view({ tile: mini, counts: { needs_you: 2, done: 0, working: 1 } });
    expect(tile(html)).toContain('aria-label="mini, Needs You 2, Working 1"');
    expect(html.startsWith("<button")).toBe(true);
    expect(html).not.toContain("tabindex");
  });

  it("stacks a circle per non-zero state in the order Needs You, Done, Working, and takes no slot for a zero (B4)", () => {
    expect(circles(view({ tile: mini, counts: { needs_you: 2, done: 1, working: 3 } }))).toEqual([
      ["needs_you", "2"],
      ["done", "1"],
      ["working", "3"],
    ]);
    // Only Working: one blue circle in the top slot.
    const working = view({ tile: mini, counts: { needs_you: 0, done: 0, working: 3 } });
    expect(circles(working)).toEqual([["working", "3"]]);
    expect(working).toContain("bg-agent-working");
    expect(working).not.toContain("bg-warning");
  });

  it("draws a count of ten or more as `9+` in the wide pill, and nothing at all when every count is zero (B4)", () => {
    const busy = view({ tile: mini, counts: { needs_you: 12, done: 0, working: 0 } });
    expect(circles(busy)).toEqual([["needs_you", "9+"]]);
    expect(busy).toContain("w-(--size-badge-wide)");
    expect(tile(busy)).toContain("Needs You 12");
    expect(view({ tile: mini })).not.toContain("data-rail-badge");
  });

  it("draws a bar only while selected (B2)", () => {
    const quiet = view({ tile: mini });
    expect(quiet).not.toContain("data-rail-bar");
    expect(view({ tile: mini, selected: true })).toContain("data-rail-bar");
    expect(tile(view({ tile: mini, selected: true }))).toContain('aria-pressed="true"');
  });

  it("dims a disconnected device with a cross, no badge, and reads 연결 안 됨 (B8)", () => {
    // Counts a stale session still carries are not drawn.
    const html = view({ tile: off, connected: false, counts: { needs_you: 1, done: 1, working: 1 } });
    expect(html).toContain("data-rail-off");
    expect(html).not.toContain("data-rail-badge");
    expect(html).toContain("opacity-50");
    expect(tile(html)).toContain("연결 안 됨");
    expect(tile(html)).not.toContain("Needs You");
  });

  it("keeps a long Korean device name to one truncated line under the tile (B46)", () => {
    const html = view({ tile: off, connected: false });
    expect(html).toMatch(/data-rail-label="true">연구실 빌드 서버 자동화 장비</);
    expect(html).toMatch(/class="[^"]*truncate[^"]*"[^>]*data-rail-label/);
  });
});
