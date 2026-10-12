import { expect, it } from "vitest";
import { AGENT_ADAPTERS, agentAdapter } from "./agentAdapters";
import type { LimitCell, WindowOutcome } from "./generated/hided-ws";
import { initializeInterfaceI18n } from "./i18n/instance";
import { absenceReason, accountWindows, hasLimits, limitValue } from "./usageLimits";

const t = initializeInterfaceI18n("en").getFixedT(null, "translation");
const read = (outcome: WindowOutcome): LimitCell => ({ state: "reading", freshness: "fresh", observed_at_ms: 1000, value: { windows: [{ kind: "five_hour", scope: { kind: "all" }, outcome }] } });

it("offers only declared account windows and leaves the five unsupported agents with reasons", () => {
  expect(accountWindows(agentAdapter("codex")!, read({ state: "not_offered" }))).toEqual(["weekly"]);
  expect(accountWindows(agentAdapter("codex")!, read({ state: "not_started" }))).toEqual(["five_hour", "weekly"]);
  const unsupported = AGENT_ADAPTERS.filter((adapter) => !hasLimits(adapter));
  expect(unsupported.map((adapter) => adapter.id)).toEqual(["grok", "opencode", "pi", "omp", "cursor"]);
  for (const adapter of unsupported) {
    expect(accountWindows(adapter, undefined)).toEqual([]);
    expect(absenceReason(adapter, t)).not.toBe("");
  }
});

it("never substitutes a percent for a not-started or expired window", () => {
  expect(limitValue(read({ state: "not_started" }), "five_hour", 1000, t)).toBe("Not started");
  expect(limitValue(read({ state: "expired", reset_at_s: 1 }), "five_hour", 1000, t)).toBe("Reset passed");
  expect(limitValue(read({ state: "open", used_percent: 90, resets_at_s: 1 }), "five_hour", 1000, t)).toBe("Reset passed");
  expect(limitValue(read({ state: "open", used_percent: 11, resets_at_s: 601 }), "five_hour", 1000, t)).toBe("11% · in 10m");
});
