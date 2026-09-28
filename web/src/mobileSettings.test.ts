import { describe, expect, it } from "vitest";
import { checklistRows, codeCountdown, exposureLine, phoneLine, type MobileState, type PhoneRow } from "./mobileSettings";

const DAY = 24 * 60 * 60 * 1000;

function state(overrides: Partial<MobileState>): MobileState {
  return {
    enabled: true,
    exposure: "blocked",
    checklist: { installed: "ok", logged_in: "failed", https: "waiting", host_name: null },
    download_url: "https://tailscale.com/download",
    admin_url: "https://login.tailscale.com/admin/dns",
    foreign_target: null,
    failure: null,
    url: null,
    qr: null,
    code_expires_at_ms: null,
    phones: [],
    max_phones: 4,
    push_mode: "app_closed",
    now_ms: 0,
    ...overrides,
  };
}

describe("checklistRows", () => {
  it("puts the action on the first failing step and leaves the rest waiting", () => {
    const rows = checklistRows(state({}));
    expect(rows.map((row) => row.state)).toEqual(["ok", "failed", "waiting", "waiting"]);
    expect(rows.filter((row) => row.action !== null).map((row) => row.id)).toEqual(["logged_in"]);
  });

  it("links the download page when Tailscale is missing and the admin console when HTTPS is off", () => {
    const missing = checklistRows(state({ checklist: { installed: "failed", logged_in: "waiting", https: "waiting", host_name: null } }));
    expect(missing[0].link?.href).toBe("https://tailscale.com/download");
    const https = checklistRows(state({ checklist: { installed: "ok", logged_in: "ok", https: "failed", host_name: "mac" } }));
    expect(https[1].title).toContain("mac");
    expect(https[2].link?.href).toBe("https://login.tailscale.com/admin/dns");
  });

  it("marks the phone step done once the QR shows", () => {
    const rows = checklistRows(state({ exposure: "exposed", qr: "https://mac.ts.net/m/#pair=x", checklist: { installed: "ok", logged_in: "ok", https: "ok", host_name: "mac" } }));
    expect(rows.every((row) => row.state === "ok")).toBe(true);
  });
});

describe("exposureLine", () => {
  it("names the foreign entry hide will not touch and the failed serve command", () => {
    expect(exposureLine(state({ exposure: "foreign", foreign_target: "http://127.0.0.1:3000" }))?.text).toContain("http://127.0.0.1:3000");
    const failed = exposureLine(state({ exposure: "failed", failure: { step: "add", message: "serve config denied" } }));
    expect(failed?.tone).toBe("error");
    expect(failed?.text).toContain("serve config denied");
  });

  it("is empty while the QR shows", () => {
    expect(exposureLine(state({ exposure: "exposed", qr: "q" }))).toBeNull();
  });
});

describe("codeCountdown", () => {
  it("counts down from the daemon clock plus the time since the frame", () => {
    expect(codeCountdown(300_000, 0, 0)).toBe("5:00");
    expect(codeCountdown(300_000, 0, 61_000)).toBe("3:59");
    expect(codeCountdown(300_000, 0, 300_000)).toBeNull();
    expect(codeCountdown(null, 0, 0)).toBeNull();
  });
});

describe("phoneLine", () => {
  const phone: PhoneRow = { id: "p", name: "iPhone", last_seen_ms: 0, connected: false, notifications: "on", revoke_at_ms: 7 * DAY };

  it("reads 방금 while connected and carries the notification state", () => {
    expect(phoneLine({ ...phone, connected: true }, 3 * DAY)).toBe("방금 · 알림 받는 중");
  });

  it("counts the days to the seven-day revoke once away a day", () => {
    expect(phoneLine(phone, 3 * DAY)).toBe("3일 전 · 알림 받는 중 · 4일 뒤 자동 해지");
    expect(phoneLine({ ...phone, notifications: "unasked" }, 60 * 60 * 1000)).toBe("1시간 전");
  });
});
