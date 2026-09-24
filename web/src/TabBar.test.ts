import { describe, expect, it } from "vitest";
import { closingSuffix } from "./TabBar";
import type { AsyncOperation } from "./snapshot";

function op(kind: string, target_id: string, phase: string): AsyncOperation {
  return { id: `${kind}:${target_id}`, kind, target_id, scope_id: "c1", phase, stage: "", message: null, retryable: false };
}

describe("closingSuffix", () => {
  it("shows while the close is in flight and not after it settles", () => {
    expect(closingSuffix("t1", "tab.close", [op("tab.close", "t1", "transmitting")])).toBe(true);
    expect(closingSuffix("t1", "tab.close", [op("tab.close", "t1", "awaiting_topology")])).toBe(true);
    expect(closingSuffix("t1", "tab.close", [op("tab.close", "t1", "unknown")])).toBe(true);
    expect(closingSuffix("t1", "tab.close", [op("tab.close", "t1", "completed")])).toBe(false);
    expect(closingSuffix("t1", "tab.close", [op("tab.close", "t1", "failed")])).toBe(false);
    expect(closingSuffix("t1", "tab.close", [op("pane.close", "t1", "transmitting")])).toBe(false);
    expect(closingSuffix("t2", "tab.close", [op("tab.close", "t1", "transmitting")])).toBe(false);
  });
});
