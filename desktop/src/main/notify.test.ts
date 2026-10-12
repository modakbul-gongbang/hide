import { describe, expect, it } from "vitest";
import { KeptNotifications, NOTIFY_KEPT, NOTIFY_LIMITS, notifyRequest, WITHDRAW_LIMIT, withdrawRequest } from "./notify";

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
