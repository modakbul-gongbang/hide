import { describe, expect, it } from "vitest";
import {
  BACKOFF_MAX_MS,
  BACKOFF_START_MS,
  badgeText,
  connectionAfterHealthFails,
  nextBackoff,
} from "./connection";

describe("connection machine", () => {
  it("caps backoff at 30s", () => {
    let value = BACKOFF_START_MS;
    for (let i = 0; i < 12; i += 1) value = nextBackoff(value);
    expect(value).toBe(BACKOFF_MAX_MS);
  });

  it("goes gone after three health failures", () => {
    expect(connectionAfterHealthFails(2)).toBe("reconnecting");
    expect(connectionAfterHealthFails(3)).toBe("gone");
  });

  it("hides the badge while live", () => {
    expect(badgeText("live")).toBe("");
    expect(badgeText("reconnecting")).toBe("reconnecting");
    expect(badgeText("gone")).toContain("hide");
  });
});
