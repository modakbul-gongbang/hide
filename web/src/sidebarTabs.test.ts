import { describe, expect, it } from "vitest";
import { legacyAgentRow } from "../test/legacyAgentRow";
import type { BoardRow } from "./projectBoard";
import type { Checkout, Tab } from "./snapshot";
import { sidebarTabGroups } from "./sidebarTabs";

const agent = (id: string, device_id = "local") => legacyAgentRow({ id, pane_id: id, device_id, identity_label: id, agent_kind: "codex", group: "working" });
const tab = (id: string, panes: string[]): Tab => ({ id, label: "Automatic task label", sidebar: { manual_name: null, number: Number(id.slice(1)) }, panes: panes.map((id) => ({ id })) } as Tab);
const checkout = (tabs: Tab[]) => ({ id: "c", tabs } as Checkout);
const row = (id: string, depth = 0): BoardRow => ({ agent: agent(id), depth });

describe("Projects sidebar tab groups", () => {
  it("orders complete root subtrees by tabs and keeps cross-tab descendants under their root", () => {
    const tabs = [tab("t1", ["a", "b", "shell"]), tab("t2", ["child", "c"]), tab("t3", ["d", "other-child"])];
    const rows = [row("c"), row("a"), row("child", 1), row("b")];
    const groups = sidebarTabGroups(checkout(tabs), "local", rows, [agent("a"), agent("b"), agent("child"), agent("c"), agent("d"), agent("other-child")]);
    expect(groups.map((group) => [group.tab?.id, group.count, group.rows.map((value) => value.agent.pane_id)])).toEqual([["t1", 2, ["a", "child", "b"]], ["t2", 2, ["c"]]]);
    expect(groups.every((group) => group.glyph)).toBe(true);
  });

  it("counts physical agents independently of rendered roots, shell panes, devices and child disclosure", () => {
    const groups = sidebarTabGroups(checkout([tab("t1", ["a", "child", "shell", "remote"])]), "local", [row("a")], [agent("a"), agent("child"), agent("remote", "mini")]);
    expect(groups[0]).toMatchObject({ count: 2, glyph: true });
    expect(groups[0]!.rows).toHaveLength(1);
  });

  it("wears the glyph through 2 -> 1 -> 2 and keeps a manual name", () => {
    const named = { ...tab("t1", ["a", "b"]), sidebar: { manual_name: "긴 한국어 탭 이름", number: 1 } };
    const at = (panes: string[]) => sidebarTabGroups(checkout([{ ...named, panes: panes.map((id) => ({ id })) as Tab["panes"] }]), "local", panes.map((id) => row(id)), panes.map((id) => agent(id)))[0]!;
    expect(at(["a", "b"])).toMatchObject({ glyph: true });
    expect(at(["a"])).toMatchObject({ glyph: false });
    expect(at(["a", "b"])).toMatchObject({ glyph: true, tab: { sidebar: { manual_name: "긴 한국어 탭 이름" } } });
  });

  it("leaves partial legacy snapshots readable without fabricating a tab name", () => {
    const legacy = { ...tab("t1", ["a", "b"]), sidebar: undefined };
    expect(sidebarTabGroups(checkout([legacy]), "local", [row("a"), row("b")], [agent("a"), agent("b")])[0]).toMatchObject({ glyph: false });
    expect(sidebarTabGroups(checkout([tab("t1", [])]), "local", [row("unplaced")], [agent("unplaced")])[0]).toMatchObject({ tab: null, glyph: false });
  });
});
