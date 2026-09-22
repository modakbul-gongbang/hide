import { beforeEach, describe, expect, it } from "vitest";
import { lastCheckoutOf, recentCheckoutOrder, recentTabOrder, rememberCheckout, rememberTab, resetRecent } from "./recent";

describe("recent order", () => {
  beforeEach(() => resetRecent());

  it("puts the most recent first and unseen rows after in their own order", () => {
    rememberTab("c1", "t1");
    rememberTab("c1", "t2");
    rememberTab("c1", "t1");
    expect(recentTabOrder("c1", ["t0", "t1", "t2", "t3"])).toEqual(["t1", "t2", "t0", "t3"]);
    expect(recentTabOrder("c2", ["a", "b"])).toEqual(["a", "b"]);
  });

  it("drops tabs that no longer exist", () => {
    rememberTab("c1", "gone");
    rememberTab("c1", "t1");
    expect(recentTabOrder("c1", ["t1"])).toEqual(["t1"]);
  });

  it("answers a project row with its last focused checkout", () => {
    rememberCheckout("a");
    rememberCheckout("b");
    expect(lastCheckoutOf(["a", "c"])).toBe("a");
    expect(lastCheckoutOf(["c"])).toBeNull();
    expect(recentCheckoutOrder(["a", "b", "c"])).toEqual(["b", "a", "c"]);
  });
});
