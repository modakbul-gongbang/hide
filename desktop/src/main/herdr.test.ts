import { describe, expect, it } from "vitest";
import { chooseHerdr } from "./herdr";

describe("chooseHerdr", () => {
  const packaged = { bundledDir: "/A/Contents/Resources", herdrBinPath: null, herdrPaneId: null };

  it("hands a packaged app's children its bundled herdr when nothing names one", () => {
    expect(chooseHerdr(packaged)).toEqual({ path: "/A/Contents/Resources/herdr", source: "bundled", replacedPaneValue: null });
  });

  it("passes an explicit override through unchanged", () => {
    expect(chooseHerdr({ ...packaged, herdrBinPath: "/opt/herdr" })).toEqual({ path: null, source: "inherited", replacedPaneValue: null });
  });

  it("replaces the value a Herdr pane exported with the bundled herdr", () => {
    const gone = "/A/Contents/Resources/herdr-runtime/herdr";
    expect(chooseHerdr({ ...packaged, herdrBinPath: gone, herdrPaneId: "w1:p1" })).toEqual({
      path: "/A/Contents/Resources/herdr",
      source: "bundled",
      replacedPaneValue: gone,
    });
    const current = chooseHerdr({ ...packaged, herdrBinPath: "/A/Contents/Resources/herdr", herdrPaneId: "w1:p1" });
    expect(current).toEqual({ path: "/A/Contents/Resources/herdr", source: "bundled", replacedPaneValue: null });
  });

  it("adds nothing when unpackaged, pane or not", () => {
    const unpackaged = { bundledDir: null, herdrBinPath: "/opt/herdr", herdrPaneId: "w1:p1" };
    expect(chooseHerdr(unpackaged)).toEqual({ path: null, source: "inherited", replacedPaneValue: null });
    expect(chooseHerdr({ ...unpackaged, herdrBinPath: null, herdrPaneId: null })).toEqual({ path: null, source: "inherited", replacedPaneValue: null });
  });
});
