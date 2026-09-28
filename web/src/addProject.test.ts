import { describe, expect, it } from "vitest";
import { addProjectHosts, alreadyRegistered, initialHost, trimFolder } from "./addProject";
import type { Device, WorkspaceRegistration } from "./snapshot";

const device = (id: string, label: string, kind = "remote") => ({ id, label, kind }) as Device;
const registration = (path: string, device_id = "local") => ({ id: path, label: path, path, device_id, pinned: false }) as WorkspaceRegistration;

describe("Add a project", () => {
  it("lists this Mac first, then every registered device", () => {
    expect(addProjectHosts(undefined)).toEqual([{ id: "local", label: "This Mac" }]);
    expect(addProjectHosts([device("mini", "Mini"), device("local", "Studio", "local")])).toEqual([
      { id: "local", label: "Studio" },
      { id: "mini", label: "Mini" },
    ]);
  });

  it("opens on the focused device while it is listed, this Mac otherwise", () => {
    const hosts = addProjectHosts([device("mini", "Mini")]);
    expect(initialHost(hosts, "mini")).toBe("mini");
    expect(initialHost(hosts, "gone")).toBe("local");
    expect(initialHost(hosts, null)).toBe("local");
  });

  it("sees a folder already registered on the same device, trailing slash or not", () => {
    const rows = [registration("/home/me/hide/"), registration("/home/me/app", "mini")];
    expect(alreadyRegistered(trimFolder("/home/me/hide/"), "local", rows)).toBe(true);
    expect(alreadyRegistered("/home/me/hide", "mini", rows)).toBe(false);
    expect(alreadyRegistered(trimFolder("/home/me/app//"), "mini", rows)).toBe(true);
    expect(alreadyRegistered("/home/me/other", "local", rows)).toBe(false);
  });
});
