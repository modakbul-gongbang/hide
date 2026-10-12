// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { coreMachineName, coreShared, machineName, moveMachines, screenNodeFromHash, windowFirst } from "./screenMachine";

describe("the screen's own machine (PRD core-host-node-remote-core B13)", () => {
  it("is the node the hash names beside the token", () => {
    expect(screenNodeFromHash("#token=abc&node=14ad0319-60d7-5a7c-ae5a-f71550dc8a60")).toBe("14ad0319-60d7-5a7c-ae5a-f71550dc8a60");
  });

  it("is the core's own when the hash names none", () => {
    expect(screenNodeFromHash("#token=abc")).toBeNull();
    expect(screenNodeFromHash("")).toBeNull();
  });

  it("is no node when the value cannot be a node id", () => {
    expect(screenNodeFromHash("#node=remote:mini:pane:w1")).toBeNull();
    expect(screenNodeFromHash("#node=")).toBeNull();
    expect(screenNodeFromHash(`#node=${"a".repeat(129)}`)).toBeNull();
  });
});

describe("what this window calls each machine (PRD core-host-node-move B2, N1)", () => {
  const device = (id: string, kind: string, over: Record<string, unknown> = {}) => ({ id, kind, label: id === "core" ? "This Mac" : id, ...over }) as never;
  const devices = [device("core", "local", { machine_name: "Mac mini" }), device("mbp", "remote", { label: "MacBook Pro", dials_in: true }), device("box", "remote", { label: "build-box" })];
  const t = ((key: string) => ({ "common.thisMac": "이 Mac", "common.thisPc": "내 PC" })[key] ?? key) as never;

  beforeEach(() => {
    vi.stubGlobal("navigator", { platform: "MacIntel" });
  });

  afterEach(() => {
    window.location.hash = "";
    vi.unstubAllGlobals();
  });

  it("names the core's own machine This Mac in a window on the core, and marks it only once a node dials in", () => {
    window.location.hash = "#token=t";
    expect(machineName(devices, "core", t)).toBe("이 Mac");
    expect(machineName(devices, "mbp", t)).toBe("MacBook Pro");
    expect(windowFirst(devices).map((row) => (row as { id: string }).id)).toEqual(["core", "mbp", "box"]);
    expect(coreShared(devices)).toBe(true);
    expect(coreShared([devices[0], devices[2]] as never)).toBe(false);
  });

  it("names the node This Mac and the core's machine by its own name in a window on a node, which comes first", () => {
    window.location.hash = "#token=t&node=mbp";
    expect(machineName(devices, "mbp", t)).toBe("이 Mac");
    expect(machineName(devices, "core", t)).toBe("Mac mini");
    expect(coreMachineName(devices, t)).toBe("Mac mini");
    expect(windowFirst(devices).map((row) => (row as { id: string }).id)).toEqual(["mbp", "core", "box"]);
    expect(coreShared([devices[0], devices[2]] as never)).toBe(true);
    expect(moveMachines(devices, { direction: "back", device: null }, t)).toEqual({ from: "Mac mini", to: "이 Mac" });
  });

  it("names this window's machine as its own system does, whichever machine the core runs on", () => {
    vi.stubGlobal("navigator", { platform: "Win32" });
    window.location.hash = "#token=t&node=mbp";
    expect(machineName(devices, "mbp", t)).toBe("내 PC");
    expect(machineName(devices, "core", t)).toBe("Mac mini");
  });
});
