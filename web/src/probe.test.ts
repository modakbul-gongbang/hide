import { describe, expect, it } from "vitest";
import { probeEnabled } from "./probe";

describe("probeEnabled", () => {
  it("installs only for probe=1 in the query", () => {
    expect(probeEnabled("?probe=1")).toBe(true);
    expect(probeEnabled("?probe=1&x=2")).toBe(true);
    expect(probeEnabled("?probe=0")).toBe(false);
    expect(probeEnabled("")).toBe(false);
  });
});
