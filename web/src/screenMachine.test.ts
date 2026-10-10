import { describe, expect, it } from "vitest";
import { screenNodeFromHash } from "./screenMachine";

describe("the screen's own machine (PRD core-host-node-remote-core B13)", () => {
  it("is the node the hash names beside the token", () => {
    expect(screenNodeFromHash("#token=abc&node=14ad0319-60d7-5a7c-ae5a-f71550dc8a60")).toBe("14ad0319-60d7-5a7c-ae5a-f71550dc8a60");
  });

  it("is the core's own when the hash names none", () => {
    expect(screenNodeFromHash("#token=abc")).toBeNull();
    expect(screenNodeFromHash("")).toBeNull();
  });

  it("is no node when the value cannot be a node id", () => {
    expect(screenNodeFromHash("#node=remote:mini:pane:w1")).toBeNull();
    expect(screenNodeFromHash("#node=")).toBeNull();
    expect(screenNodeFromHash(`#node=${"a".repeat(129)}`)).toBeNull();
  });
});
