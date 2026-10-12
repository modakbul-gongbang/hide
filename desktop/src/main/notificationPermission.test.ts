import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { NOTIFICATION_HELPER, NotificationPermissionReader, parsePermission } from "./notificationPermission";

describe("parsePermission", () => {
  it("reads the helper's authorization as the permission the shell shows", () => {
    expect(parsePermission('{"authorization":"authorized"}\n')).toBe("granted");
    expect(parsePermission('{"authorization":"provisional"}')).toBe("granted");
    expect(parsePermission('{"authorization":"denied"}')).toBe("denied");
    expect(parsePermission('{"authorization":"not-determined"}')).toBe("not_determined");
    expect(parsePermission('{"authorization":"unknown"}')).toEqual({ unreadable: "authorization_unknown" });
    expect(parsePermission("Terminating app")).toEqual({ unreadable: "not_json" });
  });
});

describe("NotificationPermissionReader", () => {
  const folders: string[] = [];
  afterEach(() => {
    for (const folder of folders.splice(0)) fs.rmSync(folder, { recursive: true, force: true });
  });

  function helperDir(script: string): string {
    const folder = fs.mkdtempSync(path.join(os.tmpdir(), "hide-notification-helper-"));
    folders.push(folder);
    const helper = path.join(folder, NOTIFICATION_HELPER);
    fs.writeFileSync(helper, `#!/bin/sh\n${script}\n`);
    fs.chmodSync(helper, 0o755);
    return folder;
  }

  function reader(dir: string, platform: NodeJS.Platform = "darwin", supported = true) {
    const events: { event: string; fields: unknown }[] = [];
    return { events, reader: new NotificationPermissionReader(platform, dir, { event: (event, fields) => events.push({ event, fields }) }, () => supported) };
  }

  it.runIf(process.platform !== "win32")("asks the helper once for reads that overlap and answers both", async () => {
    const dir = helperDir(`echo run >> "${"$"}(dirname "$0")/runs"; sleep 0.2; echo '{"authorization":"denied"}'`);
    const { reader: permission } = reader(dir);
    expect(await Promise.all([permission.read(), permission.read()])).toEqual(["denied", "denied"]);
    expect(fs.readFileSync(path.join(dir, "runs"), "utf8").trim().split("\n")).toHaveLength(1);
  });

  it.runIf(process.platform !== "win32")("answers unknown, with the reason in the log, when the helper fails or is missing", async () => {
    const failing = reader(helperDir("echo 'bundleProxyForCurrentProcess is nil' >&2; exit 134"));
    expect(await failing.reader.read()).toBe("unknown");
    expect(failing.events).toEqual([{ event: "notify.permission_unreadable", fields: { reason: "exit", code: 134, signal: null } }]);
    const missing = reader(path.join(os.tmpdir(), "hide-no-such-folder"));
    expect(await missing.reader.read()).toBe("unknown");
    expect(missing.events[0]?.fields).toEqual({ reason: "helper_missing" });
  });

  it("has nothing to read outside macOS, and says so where no notification service answers (D-24)", async () => {
    const { reader: permission, events } = reader("/nowhere", "win32");
    expect(await permission.read()).toBe("unsupported");
    expect(events).toEqual([]);
    expect(await reader("/nowhere", "linux", false).reader.read()).toBe("no_service");
  });

  it("says no notification service for the rest of the run once a notification failed to show on Linux, and only there", async () => {
    const linux = reader("/nowhere", "linux").reader;
    expect(await linux.read()).toBe("unsupported");
    expect(linux.canShow).toBe(true);
    linux.showFailed();
    expect(await linux.read()).toBe("no_service");
    expect(linux.canShow).toBe(false);
    const windows = reader("/nowhere", "win32").reader;
    windows.showFailed();
    expect(await windows.read()).toBe("unsupported");
    expect(windows.canShow).toBe(true);
  });
});
