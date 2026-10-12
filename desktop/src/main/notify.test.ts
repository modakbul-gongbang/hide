import { describe, expect, it } from "vitest";
import { BADGE_LIMIT, badgeRequest, KeptNotifications, launchedId, NOTIFY_KEPT, NOTIFY_LIMITS, notifyRequest, OVERLAY_SIZE, overlayDot, toastXml, WITHDRAW_LIMIT, withdrawRequest } from "./notify";

describe("notifyRequest", () => {
  it("takes an id, a title and a body within their limits", () => {
    expect(notifyRequest({ id: "f-1/T-1/q-1", title: "Factory · 내 차례", body: "답할 것" })).toEqual({ id: "f-1/T-1/q-1", title: "Factory · 내 차례", body: "답할 것", unlessFocused: false });
    expect(notifyRequest({ id: "agent:node|w1:p1", title: "Add a dark mode switch", body: "Needs you · my-app", unlessFocused: true })).toMatchObject({ unlessFocused: true });
  });

  it("refuses anything else instead of cutting it", () => {
    expect(notifyRequest(null)).toEqual({ refused: "shape" });
    expect(notifyRequest({ id: "a", title: "t" })).toEqual({ refused: "shape" });
    expect(notifyRequest({ id: "", title: "t", body: "" })).toEqual({ refused: "shape" });
    expect(notifyRequest({ id: "a", title: "t", body: "x".repeat(NOTIFY_LIMITS.body + 1) })).toEqual({ refused: "length" });
    expect(notifyRequest({ id: "a", title: "t", body: "", unlessFocused: "yes" })).toEqual({ refused: "shape" });
  });
});

describe("withdrawRequest", () => {
  it("takes a short list of ids and refuses anything else", () => {
    expect(withdrawRequest(["agent:node|w1:p1"])).toEqual(["agent:node|w1:p1"]);
    expect(withdrawRequest("agent:node|w1:p1")).toEqual({ refused: "shape" });
    expect(withdrawRequest([""])).toEqual({ refused: "shape" });
    expect(withdrawRequest(Array.from({ length: WITHDRAW_LIMIT + 1 }, (_, n) => `id-${n}`))).toEqual({ refused: "length" });
  });
});

describe("KeptNotifications", () => {
  it("keeps the newest for their click and lets the oldest go", () => {
    const kept = new KeptNotifications<number>();
    for (let n = 0; n < NOTIFY_KEPT; n += 1) expect(kept.keep(`id-${n}`, n)).toBeNull();
    expect(kept.keep("one more", 99)).toBe(0);
    expect(kept.size).toBe(NOTIFY_KEPT);
    expect(kept.keep("id-5", 55)).toBe(5);
    kept.forget("id-5");
    expect(kept.size).toBe(NOTIFY_KEPT - 1);
    expect(kept.take("id-6")).toBe(6);
    expect(kept.take("id-6")).toBeNull();
    expect(kept.size).toBe(NOTIFY_KEPT - 2);
  });
});

describe("badgeRequest", () => {
  it("takes a count of agents and refuses anything else", () => {
    expect(badgeRequest(0)).toBe(0);
    expect(badgeRequest(3)).toBe(3);
    expect(badgeRequest(-1)).toEqual({ refused: "shape" });
    expect(badgeRequest(1.5)).toEqual({ refused: "shape" });
    expect(badgeRequest("3")).toEqual({ refused: "shape" });
    expect(badgeRequest(BADGE_LIMIT + 1)).toEqual({ refused: "length" });
  });
});

describe("toastXml", () => {
  it("writes the title and body as Electron does and names the notification in the launch a click after quit hands back", () => {
    const xml = toastXml({ id: "agent:node|w1:p1", title: "Fix <b> & \"quotes\"", body: "Needs you · my-app", unlessFocused: true });
    expect(xml).toBe(
      '<toast launch="hide-open=agent%3Anode%7Cw1%3Ap1"><visual><binding template="ToastGeneric"><text>Fix &lt;b&gt; &amp; &quot;quotes&quot;</text><text>Needs you · my-app</text></binding></visual></toast>',
    );
    expect(toastXml({ id: "test", title: "Notifications work", body: "", unlessFocused: false })).toContain('<binding template="ToastGeneric"><text>Notifications work</text></binding>');
    const launch = /launch="([^"]*)"/.exec(xml)![1]!;
    expect(launchedId(launch)).toBe("agent:node|w1:p1");
  });

  it("names nothing for a click Hide did not write", () => {
    expect(launchedId("")).toBeNull();
    expect(launchedId("type=click&tag=abc")).toBeNull();
    expect(launchedId("hide-open=%E0%A4%A")).toBeNull();
    expect(launchedId(`hide-open=${"x".repeat(NOTIFY_LIMITS.id + 1)}`)).toBeNull();
  });
});

describe("overlayDot", () => {
  it("is an opaque dot in the color, clear in the corners, as premultiplied BGRA", () => {
    const pixels = overlayDot("#ffb847");
    expect(pixels).toHaveLength(OVERLAY_SIZE * OVERLAY_SIZE * 4);
    const at = (x: number, y: number) => [...pixels.subarray((y * OVERLAY_SIZE + x) * 4, (y * OVERLAY_SIZE + x) * 4 + 4)];
    expect(at(OVERLAY_SIZE / 2, OVERLAY_SIZE / 2)).toEqual([0x47, 0xb8, 0xff, 255]);
    expect(at(0, 0)).toEqual([0, 0, 0, 0]);
    // The edge is antialiased, and premultiplied: no channel exceeds its pixel's alpha.
    const edge = Array.from({ length: OVERLAY_SIZE * OVERLAY_SIZE }, (_, n) => at(n % OVERLAY_SIZE, Math.floor(n / OVERLAY_SIZE))).filter(([, , , alpha]) => alpha! > 0 && alpha! < 255);
    expect(edge.length).toBeGreaterThan(0);
    for (const [blue, green, red, alpha] of edge) expect(Math.max(blue!, green!, red!)).toBeLessThanOrEqual(alpha!);
    expect(() => overlayDot("orange")).toThrow("#rrggbb");
  });
});
