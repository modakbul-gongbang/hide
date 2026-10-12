// The main window's frame (PRD sidebar-titlebar-refresh D-15, D-17). On
// macOS the title bar is hidden and the traffic lights stay, drawn over the
// left end of the shell's own top row, which reserves room for them; on
// Windows and Linux the system's frame stays and the shell draws the same
// row without that room. Fullscreen takes the traffic lights away, so the
// shell is told when the window enters and leaves it.

import type { BrowserWindowConstructorOptions } from "electron";
import type { WindowChrome } from "../../../web/src/host";

/** The shell's top row (`--size-tab-strip`); the traffic lights are centered in it. */
export const TOP_ROW_HEIGHT = 32;
/** The traffic lights' own height on macOS, so their top sits half the spare height down. */
const TRAFFIC_LIGHTS_HEIGHT = 14;
/** Their left margin; the shell reserves `--size-traffic-light-inset` (69) for the three of them. */
const TRAFFIC_LIGHTS_X = 12;

/** Whether this system's main window hides its title bar and draws the shell's row under the traffic lights. */
export function hidesTitleBar(platform: NodeJS.Platform): boolean {
  return platform === "darwin";
}

/** The frame options of the main window on this system; browser popups keep their own frame. */
export function frameOptions(platform: NodeJS.Platform): Pick<BrowserWindowConstructorOptions, "titleBarStyle" | "trafficLightPosition"> {
  if (!hidesTitleBar(platform)) return {};
  return { titleBarStyle: "hidden", trafficLightPosition: { x: TRAFFIC_LIGHTS_X, y: Math.round((TOP_ROW_HEIGHT - TRAFFIC_LIGHTS_HEIGHT) / 2) } };
}

/** What the frame takes of the shell's top row now. */
export function windowChrome(platform: NodeJS.Platform, fullscreen: boolean): WindowChrome {
  return { trafficLights: hidesTitleBar(platform), fullscreen };
}
