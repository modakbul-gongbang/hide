import { expect, it } from "vitest";
import { holdOrigin, originOf, releaseOrigin } from "./backlogOrigin";
import type { BacklogOrigin, SnapshotRest } from "./snapshot";

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

it("reads each item's own origin, so another window's item does not replace it", () => {
  const origin = (task_key: string) => ({ task_key, phase: "ready" }) as BacklogOrigin;
  const rest = { issue_work: { origins: [origin("github:acme/repo#7"), origin("local:/notes#1")] } } as SnapshotRest;
  expect(originOf(rest, "local:/notes#1")?.task_key).toBe("local:/notes#1");
  expect(originOf(rest, "github:acme/repo#8")).toBeNull();
});
