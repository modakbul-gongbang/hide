import { describe, expect, it } from "vitest";
import { listingRootFor, localRefusal, readRecent, rememberRecent, suggestions } from "./registration";

const home = "/home/me";
const registrations = [{ id: "w1", label: "hide", path: "/home/me/hide", device_id: "local", pinned: false }];
const listing = {
  kind: "remote_file_list",
  root_path: "/home/me/projects",
  entries: [
    { name: "alpha", path: "/home/me/projects/alpha", is_directory: true },
    { name: "Beta", path: "/home/me/projects/Beta", is_directory: true },
  ],
  truncated: false,
};

describe("localRefusal", () => {
  it("refuses what the snapshot already answers", () => {
    expect(localRefusal("", home, registrations, null)).toBe("empty");
    expect(localRefusal("/etc", home, registrations, null)).toBe("outside_home");
    expect(localRefusal("/home/me-other/x", home, registrations, null)).toBe("outside_home");
    expect(localRefusal("/home/me/hide/", home, registrations, null)).toBe("already_registered");
    expect(localRefusal("/home/me/projects/nope", home, registrations, listing)).toBe("not_in_listing");
  });

  it("leaves the rest to hided", () => {
    expect(localRefusal("/home/me/projects/alpha", home, registrations, listing)).toBeNull();
    expect(localRefusal("/home/me/elsewhere/x", home, registrations, listing)).toBeNull();
    expect(localRefusal("/home/me/projects/nope", home, registrations, { ...listing, truncated: true })).toBeNull();
  });
});

describe("suggestions", () => {
  it("completes the last segment from the parent's listing", () => {
    expect(suggestions("/home/me/projects/a", listing)).toEqual(["/home/me/projects/alpha"]);
    expect(suggestions("/home/me/projects/b", listing)).toEqual(["/home/me/projects/Beta"]);
    expect(suggestions("/home/me/projects/", listing)).toEqual(["/home/me/projects/alpha", "/home/me/projects/Beta"]);
    expect(suggestions("/home/me/other/a", listing)).toEqual([]);
  });

  it("names the directory to list for what was typed", () => {
    expect(listingRootFor("", home)).toBe(home);
    expect(listingRootFor("/home/me/pro", home)).toBe("/home/me");
    expect(listingRootFor("/home/me/projects/", home)).toBe("/home/me/projects");
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
