// The renderer's whole view of the host (B11): which host this is, and the
// app menu's commands. No Node API, no other channel.

import { contextBridge, ipcRenderer, type IpcRendererEvent } from "electron";
import { COMMAND_CHANNEL } from "../channel";

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
});
