import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { NewTabBody } from "./components/new-tab-body";
import { changedFiles } from "./newTab";
import { displayIdentity } from "./viewLayout";
import { browserDisplays } from "./browserViews";
import type { ChangesSnapshot, ViewLayoutSnapshot, ViewDisplaySnapshot } from "./snapshot";

describe("new-tab choices", () => {
  it("offers only File and, when changes exist, Diff", () => {
    const clean = renderToStaticMarkup(<NewTabBody hasChanges={false} onFile={() => undefined} onDiff={() => undefined} />);
    expect(clean).toContain("Open");
    expect(clean).toContain("File");
    expect(clean).toContain("⌘P");
    expect(clean).not.toContain("Diff");
    const dirty = renderToStaticMarkup(<NewTabBody hasChanges onFile={() => undefined} onDiff={() => undefined} />);
    expect(dirty).toContain("Diff");
    for (const label of ["Explorer", "Changes", "Terminal", "Recent"]) expect(dirty).not.toContain(label);
  });

  it("filters the current checkout's changed paths, retaining deleted files", () => {
    const changes = { root_path: "/project", unavailable_reason: null, entries: [
      { path: "/project/removed.ts", relative_path: "removed.ts", status: "deleted" },
      { path: "/project/수정.md", relative_path: "수정.md", status: "modified" },
    ] } as ChangesSnapshot;
    expect(changedFiles(changes, "/project").map((e) => e.relative_path)).toEqual(["removed.ts", "수정.md"]);
    expect(changedFiles(changes, "/project", "수정").map((e) => e.relative_path)).toEqual(["수정.md"]);
    expect(changedFiles(changes, "/other")).toEqual([]);
    const subfolder = { ...changes, root_path: "/project/sub" };
    expect(changedFiles(subfolder, subfolder.root_path)).toHaveLength(2);
    expect(changedFiles(subfolder, "/project")).toEqual([]);
    expect(changedFiles({ ...changes, unavailable_reason: "unavailable" }, "/project")).toEqual([]);
    expect(changedFiles(null, "/project")).toEqual([]);
  });

  it("allocates no native page for an empty address", () => {
    const layout = { root: { area: { id: "a1", active: "d1", displays: [
      { id: "d1", kind: "browser", url: null },
      { id: "d2", kind: "browser", url: "about:blank" },
    ] } } } as unknown as ViewLayoutSnapshot;
    expect(displayIdentity({ kind: "browser", url: null } as ViewDisplaySnapshot)).toBe("New tab");
    expect(browserDisplays(layout)).toEqual([{ id: "d2", url: "about:blank", load: 0 }]);
  });
});
