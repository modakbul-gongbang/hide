// The desktop host's entry: one instance per profile, one host, one menu.

import path from "node:path";
import { app, Menu } from "electron";
import { loadEnv } from "./env";
import { DesktopHost } from "./host";
import { HostLog } from "./log";
import { menuTemplate } from "./menu";

const env = loadEnv(process.env);
// Pinned before anything reads it: the single-instance lock, web storage and
// the log live here, apart from the Swift app's own folders.
app.setPath("userData", env.userDataDir ?? path.join(app.getPath("appData"), "hide-desktop"));
app.setAppLogsPath(path.join(app.getPath("userData"), "logs"));

if (!app.requestSingleInstanceLock()) {
  // B6: the running instance brings its window forward instead.
  app.quit();
} else {
  const log = new HostLog(app.getPath("logs"));
  const host = new DesktopHost(env, log);
  log.event("host.start", { packaged: app.isPackaged, version: app.getVersion(), electron: process.versions.electron });

  app.on("second-instance", () => host.reopen("second-instance"));
  app.on("activate", () => host.reopen("activate"));
  // B7: closing the last window leaves the app running, as macOS apps do.
  app.on("window-all-closed", () => {
    if (process.platform !== "darwin") app.quit();
  });
  app.on("before-quit", () => host.quit());

  app.whenReady().then(
    () => {
      Menu.setApplicationMenu(
        Menu.buildFromTemplate(menuTemplate({ appName: app.getName(), send: (id) => host.sendCommand(id), developer: !app.isPackaged })),
      );
      host.start();
    },
    (error: unknown) => {
      log.event("host.start_failed", { detail: String(error) });
      app.exit(1);
    },
  );
}
