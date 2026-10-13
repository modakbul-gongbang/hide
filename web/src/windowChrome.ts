// The desktop window's frame as the shell's top row has to leave room for it
// (PRD sidebar-titlebar-refresh D-15, D-17, B21, B23): macOS's traffic lights
// over the row's left end while the window is not full screen; nothing on
// Windows, Linux or in a browser tab.

import { useEffect, useState } from "react";
import { hostBridge, type WindowChrome } from "./host";

/** Before the host has answered, a macOS window is assumed to have its traffic lights, so no first frame draws under them. */
function firstGuess(): WindowChrome {
  return { trafficLights: hostBridge()?.platform === "darwin", fullscreen: false };
}

/** Whether the top row starts after the traffic lights' space. */
export function trafficLightSpace(chrome: WindowChrome): boolean {
  return chrome.trafficLights && !chrome.fullscreen;
}

export function useWindowChrome(): WindowChrome {
  const [chrome, setChrome] = useState<WindowChrome>(firstGuess);
  useEffect(() => hostBridge()?.onWindowChrome(setChrome), []);
  return chrome;
}
