import { describe, expect, it } from "vitest";
import { closeShortcutPolicy, type KeyboardOwner } from "./viewFocus";

describe("close shortcut policy", () => {
  const decide = (owner: KeyboardOwner, displayId: string | null = "d1", paneIds = ["p1", "p2"]) =>
    closeShortcutPolicy({ owner, workspace: "workspace", displayId, paneIds });

  it("closes the view even while an agent pane remains focused in the core", () => {
    expect(decide({ kind: "view", workspace: "workspace", areaId: "a1" })).toEqual({ kind: "view", id: "d1" });
  });

  it("closes only the keyboard's pane in a split tab or a one-pane tab", () => {
    const owner: KeyboardOwner = { kind: "pane", workspace: "workspace", paneId: "p2" };
    expect(decide(owner)).toEqual({ kind: "pane", id: "p2" });
    expect(decide(owner, null, ["p2"])).toEqual({ kind: "pane", id: "p2" });
  });

  it("never falls through from tools, no focus, a retired view or a retired pane", () => {
    expect(decide({ kind: "tool", workspace: "workspace" })).toEqual({ kind: "nothing", reason: "the tool column owns the keyboard" });
    expect(decide({ kind: "agent", workspace: "workspace" })).toMatchObject({ kind: "nothing" });
    expect(decide({ kind: "none" })).toEqual({ kind: "nothing", reason: "no keyboard owner" });
    expect(decide({ kind: "view", workspace: "workspace", areaId: "gone" }, null)).toMatchObject({ kind: "nothing" });
    expect(decide({ kind: "pane", workspace: "workspace", paneId: "gone" })).toMatchObject({ kind: "nothing" });
    expect(decide({ kind: "pane", workspace: "elsewhere", paneId: "p1" })).toMatchObject({ kind: "nothing" });
  });
});
