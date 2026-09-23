import { describe, expect, it } from "vitest";
import { splitFrontmatter } from "./frontmatter";

describe("splitFrontmatter", () => {
  it("finds the block at the top and leaves the rest to the body", () => {
    expect(splitFrontmatter("---\ntitle: one\n---\n# Body\n")).toEqual({
      front: "---\ntitle: one\n---\n",
      body: "# Body\n",
    });
  });

  it("answers null without a leading block", () => {
    expect(splitFrontmatter("# Body\n")).toBeNull();
    expect(splitFrontmatter("\n---\ntitle: one\n---\n")).toBeNull();
    expect(splitFrontmatter("---\ntitle: one\n")).toBeNull();
  });

  it("keeps an empty block and a bodyless document", () => {
    expect(splitFrontmatter("---\n---\n")).toEqual({ front: "---\n---\n", body: "" });
  });

  it("does not mistake a longer rule for a delimiter", () => {
    expect(splitFrontmatter("----\ntitle: one\n----\n")).toBeNull();
  });

  it("closes at the first delimiter after the opening line", () => {
    expect(splitFrontmatter("---\na: 1\n---\nb: 2\n---\n")).toEqual({
      front: "---\na: 1\n---\n",
      body: "b: 2\n---\n",
    });
  });
});
