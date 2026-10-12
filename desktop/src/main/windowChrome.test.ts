import { describe, expect, it } from "vitest";
import { frameOptions, windowChrome } from "./windowChrome";

describe("the main window's frame", () => {
  it("hides the title bar only on macOS, with the traffic lights centered in the 32-point top row", () => {
    expect(frameOptions("darwin")).toEqual({ titleBarStyle: "hidden", trafficLightPosition: { x: 12, y: 9 } });
    expect(frameOptions("win32")).toEqual({});
    expect(frameOptions("linux")).toEqual({});
  });

  it("tells the shell about traffic lights only on macOS, and whether the window fills the screen", () => {
    expect(windowChrome("darwin", false)).toEqual({ trafficLights: true, fullscreen: false });
    expect(windowChrome("darwin", true)).toEqual({ trafficLights: true, fullscreen: true });
    expect(windowChrome("linux", true)).toEqual({ trafficLights: false, fullscreen: true });
  });
});
