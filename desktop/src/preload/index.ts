// The renderer's whole view of the host (B11): which host this is, the app
// menu's commands in, the stored macOS pane chords out for the menu to show,
// and the pages of browser displays (issue 155). No Node API, no other
// channel. Browser pages load in their own session with no preload, so none
// of this reaches them.

import { contextBridge, ipcRenderer, type IpcRendererEvent } from "electron";
import type { BrowserCommand, BrowserHostEvent, BrowserSync } from "../../../web/src/host";
import { BINDINGS_CHANNEL, BROWSER_CAPTURE_CHANNEL, BROWSER_COMMAND_CHANNEL, BROWSER_EVENT_CHANNEL, BROWSER_SYNC_CHANNEL, COMMAND_CHANNEL } from "../channel";

contextBridge.exposeInMainWorld("hideHost", {
  kind: "electron",
  onCommand(listener: (id: string) => void): () => void {
    const handler = (_event: IpcRendererEvent, id: unknown) => {
      if (typeof id === "string") listener(id);
    };
    ipcRenderer.on(COMMAND_CHANNEL, handler);
    return () => {
      ipcRenderer.removeListener(COMMAND_CHANNEL, handler);
    };
  },
  reportBindings(bindings: Record<string, string>): void {
    ipcRenderer.send(BINDINGS_CHANNEL, bindings);
  },
  browser: {
    sync(state: BrowserSync): void {
      ipcRenderer.send(BROWSER_SYNC_CHANNEL, state);
    },
    capture(workspace: string, id: string): Promise<string | null> {
      return ipcRenderer.invoke(BROWSER_CAPTURE_CHANNEL, { workspace, id }) as Promise<string | null>;
    },
    command(workspace: string, id: string, command: BrowserCommand): void {
      ipcRenderer.send(BROWSER_COMMAND_CHANNEL, { workspace, id }, command);
    },
    onEvent(listener: (event: BrowserHostEvent) => void): () => void {
      // The main process is the only sender on this channel.
      const handler = (_event: IpcRendererEvent, event: BrowserHostEvent) => listener(event);
      ipcRenderer.on(BROWSER_EVENT_CHANNEL, handler);
      return () => {
        ipcRenderer.removeListener(BROWSER_EVENT_CHANNEL, handler);
      };
    },
  },
});
