import { expect, it } from "vitest";
import { holdOrigin, releaseOrigin } from "./backlogOrigin";

it("closes an item's origin only when the last screen showing it leaves", () => {
  // The panel holds it, then the Start dialog opens over it.
  holdOrigin("github:acme/repo#7");
  holdOrigin("github:acme/repo#7");
  holdOrigin("local:/notes#1");
  expect(releaseOrigin("github:acme/repo#7")).toBe(false);
  expect(releaseOrigin("local:/notes#1")).toBe(true);
  expect(releaseOrigin("github:acme/repo#7")).toBe(true);
  expect(() => releaseOrigin("github:acme/repo#7")).toThrow();
});
