import { legacyAgentRow } from "../test/legacyAgentRow";
// The Overview's lenses (PRD overview-lenses-tiles-agents): the tiles'
// values, the checkout lanes' order, columns and folds, the lineage rows,
// and where every way in lands. The expected answers are the PRD's
// Behaviors, read against small fixtures.

import { describe, expect, it } from "vitest";
import { initializeInterfaceI18n } from "./i18n/instance";
import { ageWords, bucketOf } from "./overviewLens";
import type { AgentRow } from "./snapshot";


const t = initializeInterfaceI18n("ko").getFixedT(null, "translation");
const english = initializeInterfaceI18n("en").getFixedT(null, "translation");

function agent(pane: string, group: string, extra: Partial<AgentRow> = {}): AgentRow {
  return legacyAgentRow({ id: pane, pane_id: pane, identity_label: pane, agent_kind: "claude", symbol: "●", group, status_code: "idle", changed_at_unix_ms: null, emphasized: false, unread: false, demand: "none", activity: "working", last_activity: "0000000000001", ...extra });
}

describe("the buckets", () => {
  it("reads asking and unseen finished agents as the operator's turn, a quiet root waiting on children apart from working, and the rest resting", () => {
    expect(bucketOf(agent("a", "needs_you", { demand: "question" }))).toBe("turn");
    expect(bucketOf(agent("a", "done"))).toBe("turn");
    expect(bucketOf(agent("a", "working", { wait: "children" }))).toBe("delegating");
    expect(bucketOf(agent("a", "working"))).toBe("working");
    expect(bucketOf(agent("a", "seen"))).toBe("resting");
  });
});

describe("ages", () => {
  it("says how old a value is in the language's own relative form", () => {
    const ages = [0, 30_000, 60_000, 5 * 60_000, 60 * 60_000, 3 * 3_600_000, 86_400_000, 3 * 86_400_000];
    expect(ages.map((ms) => ageWords("en", ms, english))).toEqual(["Just now", "Just now", "1 min. ago", "5 min. ago", "1 hr. ago", "3 hr. ago", "yesterday", "3 days ago"]);
    expect(ages.map((ms) => ageWords("ko", ms, t))).toEqual(["방금", "방금", "1분 전", "5분 전", "1시간 전", "3시간 전", "어제", "3일 전"]);
  });
});
