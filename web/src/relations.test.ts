import { legacyRest } from "../test/legacyAgentScope";
import { describe, expect, it } from "vitest";
import { CHILD, OTHER, PARENT, RICH } from "./gallery/cmdkSceneData";
import { initializeInterfaceI18n } from "./i18n/instance";
import { frontTarget, relationRows, relationsOf } from "./relations";
import type { SnapshotRest } from "./snapshot";

const { t } = initializeInterfaceI18n("en");

const ids = (rest: SnapshotRest | null, target: Parameters<typeof relationsOf>[1], anchor = false) => {
  const relations = relationsOf(rest, target, t, anchor);
  return relations ? relationRows(relations).map((row) => `${"  ".repeat(row.depth ?? 0)}${row.tag === "parent" ? "↑ " : ""}${row.id}${row.tag === "here" ? " *" : ""}`) : null;
};

describe("relations of an agent (PRD cmdk-navigation B2-B4)", () => {
  it("lists the issue, the checkout's group with its pull request, the agent in front and its parent in another checkout", () => {
    expect(ids(RICH, { kind: "agent", paneId: CHILD.pane_id }, true)).toEqual([
      "issue:github:acme/herdr-ide#273",
      "checkout:c-sand",
      "pr:w1:275",
      "agent:p-child *",
      "  ↑ agent:p-parent",
    ]);
  });

  it("keeps the first duplicate anchor and the last ancestor identity", () => {
    const agents = [...RICH.navigator!.agents!, { ...CHILD, identity_label: "Later child", lineage_parent_pane_id: null }, { ...PARENT, identity_label: "Later parent" }];
    const rest = legacyRest({ ...RICH, navigator: { ...RICH.navigator, agents } }, agents);
    const rows = relationsOf(rest, { kind: "agent", paneId: CHILD.pane_id }, t, true)!.groups[0]!.rows.filter(row => row.kind === "agent");
    expect(rows.map(row => [row.title, row.tag])).toEqual([[CHILD.identity_label, "here"], ["Later parent", "parent"]]);
  });

  it("leaves an agent outside the lineage out, even in the same checkout (B4)", () => {
    const relations = ids(RICH, { kind: "agent", paneId: PARENT.pane_id }, true)!;
    expect(relations.some((row) => row.includes("p-dag"))).toBe(false);
    expect(OTHER.pane_id).toBe("p-dag");
  });

  it("stands a child delegated to another checkout in that checkout's own group, naming its parent (B3)", () => {
    const relations = relationsOf(RICH, { kind: "agent", paneId: PARENT.pane_id }, t, true)!;
    expect(relations.issues).toEqual([]);
    expect(relations.groups.map((group) => group.head.id)).toEqual(["checkout:c-main", "checkout:c-sand"]);
    const delegated = relations.groups[1]!.rows.find((row) => row.id === "agent:p-child")!;
    expect(delegated.subtitle).toContain("↑ codex workspace-write 원인 조사");
  });

  it("has no row for a link the snapshot does not name: no pull request, no issue, no placeholder (B6)", () => {
    expect(ids(RICH, { kind: "agent", paneId: "p-dag" }, true)).toEqual(["checkout:c-main", "agent:p-dag *"]);
  });

  it("is null for an agent no checkout holds", () => {
    expect(relationsOf(RICH, { kind: "agent", paneId: "p-nowhere" }, t)).toBeNull();
  });
});

describe("relations of other things", () => {
  it("draws a checkout's issue, itself and its pull request, with no agents (B5)", () => {
    expect(ids(RICH, { kind: "checkout", checkoutId: "c-sand" })).toEqual(["issue:github:acme/herdr-ide#273", "checkout:c-sand", "pr:w1:275"]);
  });

  it("finds the checkout that carries a pull request or an issue", () => {
    expect(ids(RICH, { kind: "pr", workspaceId: "w1", number: 275 })).toEqual(ids(RICH, { kind: "checkout", checkoutId: "c-sand" }));
    expect(ids(RICH, { kind: "issue", taskKey: "github:acme/herdr-ide#273" })).toEqual(ids(RICH, { kind: "checkout", checkoutId: "c-sand" }));
  });

  it("has none for a pull request no checkout carries", () => {
    expect(relationsOf(RICH, { kind: "pr", workspaceId: "w1", number: 260 }, t)).toBeNull();
  });
});

describe("what is in front (B5, B7)", () => {
  it("is the agent while the keyboard is in an agent pane, the checkout otherwise, nothing off a Workspace", () => {
    expect(frontTarget(RICH, "workspace", false, "p-child", true, "c-sand")).toEqual({ kind: "agent", paneId: "p-child" });
    expect(frontTarget(RICH, "workspace", false, "p-child", false, "c-sand")).toEqual({ kind: "checkout", checkoutId: "c-sand" });
    expect(frontTarget(RICH, "workspace", false, "p-not-an-agent", true, "c-sand")).toEqual({ kind: "checkout", checkoutId: "c-sand" });
    expect(frontTarget(RICH, "overview", false, "p-child", true, "c-sand")).toBeNull();
    expect(frontTarget(RICH, "main", false, null, false, null)).toBeNull();
    expect(frontTarget(RICH, "workspace", true, "p-child", true, "c-sand")).toBeNull();
  });
});
