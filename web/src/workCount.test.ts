// The number on the rail's Work icon (PRD work-tab-pr-lens B4): Sessions asks
// and PRs asks added, an agent that asks in both counted once.

import { describe, expect, it } from "vitest";
import { countAsks } from "./workCount";

describe("countAsks", () => {
  it("adds the asks of Sessions and of PRs", () => {
    expect(countAsks(2, new Set(["p1", "p2"]), [null, null, "p9"])).toEqual({ sessions: 2, prs: 3, total: 5 });
  });

  it("counts an agent that asks in both lenses once", () => {
    expect(countAsks(2, new Set(["p1", "p2"]), ["p1", null])).toEqual({ sessions: 2, prs: 2, total: 3 });
  });

  it("is zero when nothing asks", () => {
    expect(countAsks(0, new Set(), [])).toEqual({ sessions: 0, prs: 0, total: 0 });
  });
});
