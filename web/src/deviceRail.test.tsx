import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { RailTileView, type Tile } from "./components/device-rail";
import { TooltipProvider } from "./components/ui/tooltip";
import { sidebarBody, type TileCounts } from "./devices";
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

const NONE: TileCounts = { needs_you: 0, done: 0 };

function view(props: { tile: Tile; selected?: boolean; counts?: TileCounts; connected?: boolean }) {
  return renderToStaticMarkup(
    <TooltipProvider>
      <RailTileView selected={false} counts={NONE} connected onSelect={() => undefined} {...props} />
    </TooltipProvider>,
  );
}

/** Each mark drawn, in source order, with its text: the Done dot has none. */
const marks = (html: string) => [...html.matchAll(/data-rail-badge="(\w+)"[^>]*>([^<]*)</g)].map((match) => [match[1], match[2]]);

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

describe("a rail tile (quick device-rail-slack)", () => {
  const local: Tile = { id: "local", label: "This Mac", icon: "local" };
  const mini: Tile = { id: "mini", label: "Mac mini", icon: "remote" };
  const off: Tile = { id: "build-box", label: OFF.label, icon: "remote" };

  it("is a focusable button whose name carries the device and each count it marks (B4)", () => {
    const html = view({ tile: mini, counts: { needs_you: 2, done: 1 } });
    expect(tile(html)).toContain('aria-label="Mac mini, Needs You 2, Done 1"');
    expect(html.startsWith("<button")).toBe(true);
    expect(html).not.toContain("tabindex");
  });

  it("draws no name under the tile: This Mac is the laptop and a device its monogram", () => {
    expect(view({ tile: local })).toContain("lucide-laptop");
    const remote = view({ tile: mini });
    expect(remote).toMatch(/data-rail-glyph="true"[^>]*>Mm</);
    expect(remote).not.toContain("data-rail-label");
    expect(remote).not.toContain(">Mac mini<");
  });

  it("notches the Needs You count into a corner and marks unseen Done with a dot that has no number", () => {
    const html = view({ tile: mini, counts: { needs_you: 2, done: 8 } });
    expect(marks(html)).toEqual([
      ["done", ""],
      ["needs_you", "2"],
    ]);
    expect(html).toContain("bg-warning");
    expect(html).toContain("bg-success");
    expect(html).toContain("text-status-foreground");
    expect(marks(view({ tile: mini, counts: { needs_you: 0, done: 3 } }))).toEqual([["done", ""]]);
  });

  it("draws a count of ten or more as `9+`, and no mark at all when both counts are zero (B4)", () => {
    const busy = view({ tile: mini, counts: { needs_you: 12, done: 0 } });
    expect(marks(busy)).toEqual([["needs_you", "9+"]]);
    expect(tile(busy)).toContain("Needs You 12");
    expect(view({ tile: mini })).not.toContain("data-rail-badge");
  });

  it("rings the tile only while selected, with no bar beside it", () => {
    const quiet = tile(view({ tile: mini }));
    expect(quiet).not.toContain("ring-foreground");
    const chosen = tile(view({ tile: mini, selected: true }));
    expect(chosen).toContain("ring-foreground");
    expect(chosen).toContain('aria-pressed="true"');
    expect(view({ tile: mini, selected: true })).not.toContain("data-rail-bar");
  });

  it("dims a disconnected device's glyph with a cross, no mark, and reads 연결 안 됨 (B8)", () => {
    // Counts a stale session still carries are not drawn.
    const html = view({ tile: off, connected: false, counts: { needs_you: 1, done: 1 } });
    expect(html).toContain("data-rail-off");
    expect(html).not.toContain("data-rail-badge");
    expect(html).toMatch(/opacity-\(--opacity-dimmed\)[^>]*data-rail-glyph|data-rail-glyph[^>]*opacity-\(--opacity-dimmed\)/);
    expect(tile(html)).toContain('aria-label="연구실 빌드 서버 자동화 장비, 연결 안 됨"');
  });
});
