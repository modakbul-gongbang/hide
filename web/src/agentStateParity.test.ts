// Frozen surface values before consolidating status ownership in the core.
// A delegated question and a read question intentionally count differently
// in the Agents headings, physical group totals and the Requests tile.
import { expect, it } from "vitest";
import { initializeInterfaceI18n } from "./i18n/instance";
import { agentTree, groupCounts } from "./navigation";
import { agentsTile, scopeAgents } from "./overviewLens";
import { requestGroups, requestRows, requestsTile } from "./requestList";
import type { AgentRow, Workspace } from "./snapshot";

const t = initializeInterfaceI18n("en").getFixedT(null, "translation");
const ready = { state: "ready" } as const;

it("preserves physical counts, root headings and scope-dependent request exclusion independently", () => {
  const row = (pane: string, group: string, verb: string, extra = {}) => ({
    id: pane, pane_id: pane, identity_label: pane, agent_kind: "claude",
    symbol: "○", group, demand: "none", activity: "stopped", unread: false,
    emphasized: false, status_code: "idle", changed_at_unix_ms: 10,
    request: { verb, verb_since_unix_ms: 10 }, ...extra,
  }) as AgentRow;
  const agents = [
    row("root", "working", "waiting", { waiting_on_descendants: true, lineage_child_pane_ids: ["child"], close_descendant_pane_ids: ["child"] }),
    row("child", "seen", "answer", { delegated: true, demand: "question", lineage_parent_pane_id: "root" }),
    row("read", "seen", "answer", { demand: "question" }),
    row("done", "done", "result", { unread: true }),
    row("unknown", "seen", "idle", { activity: "unknown" }),
  ];
  const workspace = (id: string, panes: string[]) => ({
    id, label: id, device_id: "local", checkouts: [{ id: `${id}-checkout`,
      tabs: [{ panes: panes.map((id) => ({ id })) }] }],
  }) as Workspace;
  const a = { workspace: workspace("a", ["root", "read", "done", "unknown"]), agents, device: null };
  const b = { workspace: workspace("b", ["child"]), agents, device: null };
  const all = scopeAgents([a, b]);

  expect(groupCounts(agents)).toEqual({ needs_you: 0, done: 1, working: 1, seen: 3 });
  expect(agentTree(agents.map((agent) => ({ agent, device: null }))).sections.map(({ group, count }) => [group, count]))
    .toEqual([["done", 1], ["working", 2], ["seen", 2]]);
  expect(agentsTile(all, ready, t)).toMatchObject({ value: 5, badge: { count: 1 },
    bar: [{ key: "turn", count: 1 }, { key: "working", count: 0 }, { key: "delegating", count: 1 }, { key: "resting", count: 3 }] });

  const combined = requestRows(all, agents);
  expect(combined.map(({ lens }) => lens.agent.pane_id)).toEqual(["root", "read", "done", "unknown"]);
  expect(requestGroups(combined).map(({ verb, rows }) => [verb, rows.length]))
    .toEqual([["answer", 1], ["result", 1], ["waiting", 1], ["idle", 1]]);
  expect(requestsTile(combined, ready, t)).toMatchObject({ value: 2, badge: { count: 1 } });
  const childScope = requestRows(scopeAgents([b]), agents);
  expect(childScope.map(({ lens }) => lens.agent.pane_id)).toEqual(["child"]);
  expect(requestsTile(childScope, ready, t)).toMatchObject({ value: 1, badge: { count: 1 } });
});
