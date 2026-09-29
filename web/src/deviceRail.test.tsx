import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { RailTileView, type Tile } from "./components/device-rail";
import { TooltipProvider } from "./components/ui/tooltip";
import { sidebarBody } from "./devices";
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

function view(props: { tile: Tile; selected?: boolean; badge?: number | null; connected?: boolean }) {
  return renderToStaticMarkup(
    <TooltipProvider>
      <RailTileView selected={false} badge={null} connected onSelect={() => undefined} {...props} />
    </TooltipProvider>,
  );
}

describe("which list fills the sidebar (PRD home-device-rail B2, B4, B8, B11)", () => {
  it("is the chosen tab while no remote device is registered, whatever the Inbox state", () => {
    expect(sidebarBody(world([LOCAL]), false, "projects")).toBe("projects");
    expect(sidebarBody(world([LOCAL]), true, "agents")).toBe("agents");
  });

  it("is the Inbox while it is selected, else the front device's projects, with no tabs", () => {
    expect(sidebarBody(world([LOCAL, MINI]), true, "projects")).toBe("inbox");
    expect(sidebarBody(world([LOCAL, MINI]), false, "agents")).toBe("projects");
    expect(sidebarBody(world([LOCAL, MINI], "mini"), false, "agents")).toBe("projects");
  });

  it("is the reconnect view for a front device that is not connected, even when it is the only remote one", () => {
    expect(sidebarBody(world([LOCAL, OFF], "build-box"), false, "projects")).toBe("disconnected");
    expect(sidebarBody(world([LOCAL, MINI], "mini", []), false, "projects")).toBe("disconnected");
  });
});

describe("a rail tile", () => {
  const mini: Tile = { id: "mini", label: "mini", icon: "remote" };
  const off: Tile = { id: "build-box", label: OFF.label, icon: "remote" };

  it("is a focusable button whose name carries the device and its Needs You count (B41)", () => {
    const html = view({ tile: mini, badge: 2 });
    expect(tile(html)).toContain('aria-label="mini, Needs You 2"');
    expect(html).toContain('data-rail-badge="2"');
    expect(html.startsWith("<button")).toBe(true);
    expect(html).not.toContain("tabindex");
  });

  it("draws no badge at zero, and a bar only while selected (B2, B3)", () => {
    const quiet = view({ tile: mini });
    expect(quiet).not.toContain("data-rail-badge");
    expect(quiet).not.toContain("data-rail-bar");
    expect(view({ tile: mini, selected: true })).toContain("data-rail-bar");
    expect(tile(view({ tile: mini, selected: true }))).toContain('aria-pressed="true"');
  });

  it("dims a disconnected device with a cross, no badge, and reads 연결 안 됨 (B8)", () => {
    const html = view({ tile: off, connected: false, badge: null });
    expect(html).toContain("data-rail-off");
    expect(html).toContain("opacity-50");
    expect(tile(html)).toContain("연결 안 됨");
    expect(tile(html)).not.toContain("Needs You");
  });

  it("keeps a long Korean device name to one truncated line under the tile (B46)", () => {
    const html = view({ tile: off, connected: false });
    expect(html).toMatch(/data-rail-label="true">연구실 빌드 서버 자동화 장비</);
    expect(html).toMatch(/class="[^"]*truncate[^"]*"[^>]*data-rail-label/);
  });

  it("names the Inbox as every device", () => {
    expect(tile(view({ tile: { id: "inbox", label: "Inbox", icon: "inbox" }, badge: 3 }))).toContain('aria-label="Inbox 모든 기기, Needs You 3"');
  });
});
