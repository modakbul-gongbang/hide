import { describe, expect, it } from "vitest";
import { listingRootFor, localRefusal, readRecent, rememberRecent, suggestions } from "./registration";

const home = "/Users/me";
const registrations = [{ id: "w1", label: "hide", path: "/Users/me/hide", device_id: "local", pinned: false }];
const listing = {
  root_path: "/Users/me/projects",
  entries: [
    { name: "alpha", path: "/Users/me/projects/alpha" },
    { name: "Beta", path: "/Users/me/projects/Beta" },
  ],
  truncated: false,
};

describe("localRefusal", () => {
  it("refuses what the snapshot already answers", () => {
    expect(localRefusal("", home, registrations, null)).toBe("empty");
    expect(localRefusal("/etc", home, registrations, null)).toBe("outside_home");
    expect(localRefusal("/Users/me-other/x", home, registrations, null)).toBe("outside_home");
    expect(localRefusal("/Users/me/hide/", home, registrations, null)).toBe("already_registered");
    expect(localRefusal("/Users/me/projects/nope", home, registrations, listing)).toBe("not_in_listing");
  });

  it("leaves the rest to hided", () => {
    expect(localRefusal("/Users/me/projects/alpha", home, registrations, listing)).toBeNull();
    expect(localRefusal("/Users/me/elsewhere/x", home, registrations, listing)).toBeNull();
    expect(localRefusal("/Users/me/projects/nope", home, registrations, { ...listing, truncated: true })).toBeNull();
  });
});

describe("suggestions", () => {
  it("completes the last segment from the parent's listing", () => {
    expect(suggestions("/Users/me/projects/a", listing)).toEqual(["/Users/me/projects/alpha"]);
    expect(suggestions("/Users/me/projects/b", listing)).toEqual(["/Users/me/projects/Beta"]);
    expect(suggestions("/Users/me/projects/", listing)).toEqual(["/Users/me/projects/alpha", "/Users/me/projects/Beta"]);
    expect(suggestions("/Users/me/other/a", listing)).toEqual([]);
  });

  it("names the directory to list for what was typed", () => {
    expect(listingRootFor("", home)).toBe(home);
    expect(listingRootFor("/Users/me/pro", home)).toBe("/Users/me");
    expect(listingRootFor("/Users/me/projects/", home)).toBe("/Users/me/projects");
    expect(listingRootFor("/etc/x", home)).toBe(home);
  });
});

describe("recent registrations", () => {
  it("keeps the newest first, capped, and survives bad storage", () => {
    const store = new Map<string, string>();
    const storage = { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => void store.set(k, v) };
    for (const p of ["a", "b", "c", "d", "e", "f", "b"]) rememberRecent(storage, p);
    expect(readRecent(storage)).toEqual(["b", "f", "e", "d", "c"]);
    expect(readRecent({ getItem: () => "{not json" })).toEqual([]);
  });
});
