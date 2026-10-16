// ⌘⇧M's project choice (PRD overview-device-tasks D-12, D-33, B38): which
// projects the quick add offers, which one it picks first and why, and how
// Tab walks them. The expected answers are the PRD's Behaviors.

import { describe, expect, it } from "vitest";
import { cycleIndex, quickAddChoice } from "./quickAddChoice";
import type { SnapshotRest, Workspace } from "./snapshot";
import { OVERVIEW_ENTRY, type OverviewPlace } from "./ui";

const rest = { navigator: { devices: [{ id: "local", kind: "local" }, { id: "mini", kind: "remote", dials_in: false }, { id: "laptop", kind: "remote", dials_in: true }] } } as unknown as SnapshotRest;

function project(id: string, device = "local", extra: { source?: boolean; remote?: boolean } = {}): Workspace {
  return {
    id,
    device_id: device,
    remote_target_id: extra.remote ?? device !== "local" ? device : null,
    tasks: extra.source === false ? undefined : { source: { kind: "github", label: "GitHub", name: `acme/${id}`, reading: false, failure: null, last_read_at_unix_ms: null }, tasks: [], overflow: false },
  } as unknown as Workspace;
}

const ids = (projects: readonly Workspace[]) => projects.map((one) => one.id);
const place = (patch: Partial<OverviewPlace>): OverviewPlace => ({ ...OVERVIEW_ENTRY, ...patch });

describe("the projects the quick add offers (B38, D-33)", () => {
  it("offers the device's projects that keep a backlog, in the catalog's order", () => {
    const all = [project("a"), project("unread", "local", { source: false }), project("b"), project("elsewhere", "laptop")];
    expect(ids(quickAddChoice(rest, all, "local", null, null, null).projects)).toEqual(["a", "b"]);
  });

  it("offers a device that dials the core, and never one the core dials", () => {
    const all = [project("mac-app", "laptop"), project("mini-app", "mini")];
    expect(ids(quickAddChoice(rest, all, "laptop", null, null, null).projects)).toEqual(["mac-app"]);
    expect(quickAddChoice(rest, all, "mini", null, null, null).projects).toEqual([]);
  });
});

describe("the project picked first (B38)", () => {
  const all = [project("first"), project("task"), project("filter"), project("front")];

  it("takes the project of the task whose record the Overview shows, then the one it filters to, then the checkout in front", () => {
    expect(quickAddChoice(rest, all, "local", place({ projectId: "filter", panel: { kind: "record", session: { device_id: "local", agent: "claude", session_id: "s" } } }), "task", "front")).toMatchObject({ index: 1, reason: "task" });
    expect(quickAddChoice(rest, all, "local", place({ projectId: "filter", panel: { kind: "item", workspaceId: "task", taskKey: "local:/task#1" } }), null, "front")).toMatchObject({ index: 1, reason: "task" });
    expect(quickAddChoice(rest, all, "local", place({ projectId: "filter" }), null, "front")).toMatchObject({ index: 2, reason: "filter" });
    expect(quickAddChoice(rest, all, "local", null, null, "front")).toMatchObject({ index: 3, reason: "front" });
  });

  it("ignores the Overview's place while the Overview is not on screen", () => {
    expect(quickAddChoice(rest, all, "local", null, "task", null)).toMatchObject({ index: 0, reason: "first" });
  });

  it("passes over a pick that keeps no backlog here and falls to the next reason", () => {
    const withUnread = [...all, project("unread", "local", { source: false })];
    expect(quickAddChoice(rest, withUnread, "local", place({ projectId: "unread" }), null, "front")).toMatchObject({ index: 3, reason: "front" });
    expect(quickAddChoice(rest, withUnread, "local", null, null, "unread")).toMatchObject({ index: 0, reason: "first" });
  });

  it("has nothing to pick when no project keeps a backlog", () => {
    expect(quickAddChoice(rest, [project("unread", "local", { source: false })], "local", null, null, "unread")).toEqual({ projects: [], index: 0, reason: "first" });
  });
});

describe("Tab and Shift+Tab (B38)", () => {
  it("walk the projects round in both directions", () => {
    expect([0, 1, 2].map((index) => cycleIndex(index, 3, false))).toEqual([1, 2, 0]);
    expect([0, 1, 2].map((index) => cycleIndex(index, 3, true))).toEqual([2, 0, 1]);
    expect(cycleIndex(0, 1, false)).toBe(0);
    expect(cycleIndex(0, 0, true)).toBe(0);
  });
});
