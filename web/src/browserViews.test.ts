import { describe, expect, it } from "vitest";
import { addressShown, addressUrl, browserDisplays, fileUrl, isHtmlFile, parseWorkspaceKey, placements, stateReport } from "./browserViews";
import type { ViewLayoutSnapshot } from "./snapshot";
import { workspaceKey } from "./viewLayout";

const page = { url: "", title: "", loading: false, canGoBack: false, canGoForward: false, failure: null };

describe("the address field", () => {
  it("keeps a scheme, opens a path as a file, and picks http only for a loopback host", () => {
    expect(addressUrl("https://example.com/a?b#c")).toBe("https://example.com/a?b#c");
    expect(addressUrl("about:blank")).toBe("about:blank");
    expect(addressUrl("/Users/example/문서/a b.html")).toBe("file:///Users/example/%EB%AC%B8%EC%84%9C/a%20b.html");
    expect(addressUrl("localhost:5173")).toBe("http://localhost:5173");
    expect(addressUrl("127.0.0.1:8080/docs")).toBe("http://127.0.0.1:8080/docs");
    expect(addressUrl("  example.com ")).toBe("https://example.com");
    expect(addressUrl("two words")).toBeNull();
    expect(addressUrl("   ")).toBeNull();
  });

  it("shows a web address without its scheme and anything else whole", () => {
    expect(addressShown("http://127.0.0.1:3000/a.html")).toBe("127.0.0.1:3000/a.html");
    expect(addressShown("https://example.com/")).toBe("example.com/");
    expect(addressShown("file:///Users/example/a.html")).toBe("file:///Users/example/a.html");
    // What is shown loads the same page again.
    expect(addressUrl(addressShown("http://localhost:5173/x"))).toBe("http://localhost:5173/x");
  });

  it("writes a file URL the way the daemon writes the checked path back", () => {
    // hided/src/file_url.rs spells the same path this way.
    expect(fileUrl("/a/100%/#x?.html")).toBe("file:///a/100%25/%23x%3F.html");
    expect(fileUrl("/a/{b}`c\"<d>")).toBe("file:///a/%7Bb%7D%60c%22%3Cd%3E");
  });
});

describe("what the core records of a page", () => {
  it("reports an address or title the core does not hold yet, once", () => {
    const core = { url: "https://a.test/", title: "" };
    expect(stateReport(core, { ...page, url: "https://a.test/" }, null)).toBeNull();
    const moved = stateReport(core, { ...page, url: "https://a.test/next", title: "Next" }, null);
    expect(moved).toEqual({ url: "https://a.test/next", title: "Next" });
    expect(stateReport(core, { ...page, url: "https://a.test/next", title: "Next" }, moved)).toBeNull();
    // A page that has said nothing yet leaves the core's address alone.
    expect(stateReport(core, page, null)).toBeNull();
  });
});

describe("placing pages", () => {
  const layout = {
    root: {
      split: {
        id: "s1",
        axis: "row",
        ratio: 0.5,
        first: { area: { id: "a1", active: "d2", displays: [{ id: "d1", kind: "file", path: "/r/a.ts" }, { id: "d2", kind: "browser", path: "", url: "https://a.test/", load: 7 }] } },
        second: { area: { id: "a2", active: "d3", displays: [{ id: "d3", kind: "browser", path: "", url: "http://localhost:3000/", load: 9 }] } },
      },
    },
  } as unknown as ViewLayoutSnapshot;

  it("names every browser display of the layout and where its page shows", () => {
    const rows = browserDisplays(layout);
    expect(rows).toEqual([
      { id: "d2", url: "https://a.test/", load: 7 },
      { id: "d3", url: "http://localhost:3000/", load: 9 },
    ]);
    const rect = { x: 10, y: 20, width: 300, height: 200 };
    expect(placements(rows, new Map([["d2", rect]]), new Set())).toEqual([
      { id: "d2", url: "https://a.test/", load: 7, rect, visible: true },
      { id: "d3", url: "http://localhost:3000/", load: 9, rect: null, visible: false },
    ]);
    // A still standing in its place keeps the page placed but not shown.
    expect(placements(rows, new Map([["d2", rect]]), new Set(["d2"]))[0]).toMatchObject({ rect, visible: false });
  });

  it("reads back the Workspace a host key names", () => {
    const workspace = { device_id: "local", path: "/Users/example/project" };
    expect(parseWorkspaceKey(workspaceKey(workspace))).toEqual(workspace);
    expect(parseWorkspaceKey("no separator")).toBeNull();
  });
});

it("offers Open in Browser only for an HTML file", () => {
  expect(["a.html", "b.HTM", "c.xhtml"].every(isHtmlFile)).toBe(true);
  expect(["a.md", "html", "a.html.txt"].some(isHtmlFile)).toBe(false);
});
