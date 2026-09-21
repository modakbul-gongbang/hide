import { describe, expect, it } from "vitest";
import { share } from "./share";

describe("share", () => {
  it("keeps the previous reference for an unchanged subtree", () => {
    const prev = { a: { rows: [{ id: 1, v: "x" }, { id: 2, v: "y" }] }, b: { t: 1 } };
    const next = { a: { rows: [{ id: 1, v: "x" }, { id: 2, v: "y" }] }, b: { t: 2 } };
    const out = share(prev, next);
    expect(out).not.toBe(prev);
    expect(out.a).toBe(prev.a);
    expect(out.a.rows[0]).toBe(prev.a.rows[0]);
    expect(out.b).toEqual({ t: 2 });
    expect(out.b).not.toBe(prev.b);
  });

  it("keeps sibling rows when one row changes", () => {
    const prev = [{ id: 1, v: "x" }, { id: 2, v: "y" }];
    const next = [{ id: 1, v: "x" }, { id: 2, v: "z" }];
    const out = share(prev, next);
    expect(out).not.toBe(prev);
    expect(out[0]).toBe(prev[0]);
    expect(out[1]).toEqual({ id: 2, v: "z" });
  });

  it("notices a removed key and a changed length", () => {
    expect(share({ a: 1, b: 2 }, { a: 1 })).toEqual({ a: 1 });
    const prev = [1, 2, 3];
    expect(share(prev, [1, 2])).not.toBe(prev);
    expect(share(prev, [1, 2, 3])).toBe(prev);
  });

  it("returns the new value for a type change", () => {
    expect(share({ a: 1 }, [1])).toEqual([1]);
    expect(share(null, { a: 1 })).toEqual({ a: 1 });
    expect(share("x", "y")).toBe("y");
  });
});
