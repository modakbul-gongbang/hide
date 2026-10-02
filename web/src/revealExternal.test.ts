import { describe, expect, it } from "vitest";
import { explorerMenuItems } from "./explorer";
import { REVEAL_HERE_ONLY, revealExternalEntry, revealLabel } from "./revealExternal";

describe("reveal_external (issue 324)", () => {
  it("names the act the way the host's OS does", () => {
    expect(revealLabel("darwin")).toBe("Reveal in Finder");
    expect(revealLabel("win32")).toBe("Reveal in File Explorer");
    expect(revealLabel("linux")).toBe("Open Containing Folder");
    expect(revealLabel("freebsd")).toBe("Show in File Manager");
    expect(revealLabel(undefined)).toBe("Show in File Manager");
  });

  it("is absent in a browser tab, disabled with the reason on another device, and blocked by a known reason here", () => {
    const host = { label: "Reveal in Finder" };
    expect(revealExternalEntry(null, "local")).toEqual([]);
    expect(revealExternalEntry(host, "local")).toEqual([{ id: "reveal_external", label: "Reveal in Finder", unavailable: null }]);
    expect(revealExternalEntry(host, "studio", "The file was deleted.")[0]?.unavailable).toBe(REVEAL_HERE_ONLY);
    expect(revealExternalEntry(host, "local", "The file was deleted.", true)).toEqual([
      { id: "reveal_external", label: "Reveal in Finder", unavailable: "The file was deleted.", separated: true },
    ]);
  });
});

describe("the Explorer's context menu", () => {
  /** Each item as the menu draws it: a separator line before it, then its label. */
  const drawn = (items: { label: string; separated?: boolean }[]) => items.flatMap((item) => [...(item.separated ? ["─"] : []), item.label]);
  const base = { html: false, besideReason: null, reveal: { label: "Reveal in Finder" }, device: "local" };
  const file = { path: "/Users/example/repo/a.html" };
  const folder = { path: "/Users/example/repo/src" };

  it("puts the reveal between a file's opens and Rename, and after a folder's creations", () => {
    expect(drawn(explorerMenuItems({ ...base, isDirectory: false, row: file, html: true }))).toEqual([
      "Open to the side",
      "Open in Browser",
      "─",
      "Reveal in Finder",
      "─",
      "Rename",
      "─",
      "Move to Trash",
    ]);
    expect(drawn(explorerMenuItems({ ...base, isDirectory: true, row: folder }))).toEqual(["New File", "New Folder", "─", "Reveal in Finder", "─", "Rename", "─", "Move to Trash"]);
  });

  it("offers only the two creations on the root, and no reveal in a browser tab", () => {
    expect(drawn(explorerMenuItems({ ...base, isDirectory: true, row: null }))).toEqual(["New File", "New Folder"]);
    expect(drawn(explorerMenuItems({ ...base, reveal: null, isDirectory: false, row: file }))).toEqual(["Open to the side", "Rename", "─", "Move to Trash"]);
  });

  it("lists the reveal disabled with the reason on another device's tree", () => {
    const remote = explorerMenuItems({ ...base, device: "studio", isDirectory: false, row: file });
    expect(remote.find((item) => item.id === "reveal_external")?.unavailable).toBe(REVEAL_HERE_ONLY);
  });
});
