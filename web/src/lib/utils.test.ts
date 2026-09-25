import { describe, expect, it } from "vitest";
import { cn } from "./utils";

describe("cn", () => {
  it("keeps a token text size beside a token text color", () => {
    expect(cn("text-body text-foreground", "text-muted-foreground")).toBe("text-body text-muted-foreground");
  });
  it("lets a later token size replace an earlier one", () => {
    expect(cn("px-sm text-body", "px-md text-caption")).toBe("px-md text-caption");
  });
});
