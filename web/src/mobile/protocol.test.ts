import { describe, expect, it } from "vitest";
import { boxDrawingRow, headerLine, macNameOf, notificationRow, openKey, parseFragment, replyProblem, rowsProblem, staleTags, toBase64Url, type AgentGroup, type PhoneAgent } from "./protocol";

const CREDENTIAL = "a".repeat(64);

describe("parseFragment", () => {
  it("reads the QR's pairing payload, a credential and a deep link", () => {
    const pair = toBase64Url(JSON.stringify({ v: 1, endpoint: "https://mac.tailnet.ts.net", code: "c0de" }));
    expect(parseFragment(`#pair=${pair}`).pair).toEqual({ v: 1, endpoint: "https://mac.tailnet.ts.net", code: "c0de" });
    expect(parseFragment(`#k=${CREDENTIAL}`).credential).toBe(CREDENTIAL);
    expect(parseFragment(`#open=${encodeURIComponent("local|p1")}`).open).toEqual({ device_id: "local", pane_id: "p1" });
  });

  it("refuses a payload of another version, a malformed credential and a garbled code", () => {
    const v2 = toBase64Url(JSON.stringify({ v: 2, endpoint: "https://x", code: "c" }));
    expect(parseFragment(`#pair=${v2}`).pair).toBeNull();
    expect(parseFragment("#pair=%%%").pair).toBeNull();
    expect(parseFragment("#k=nothex").credential).toBeNull();
  });

  it("keeps a remote pane id whole after the device", () => {
    expect(openKey("dev-1|remote:dev-1:pane:w1-2")).toEqual({ device_id: "dev-1", pane_id: "remote:dev-1:pane:w1-2" });
    expect(openKey("|p")).toBeNull();
  });
});

describe("replyProblem", () => {
  it("matches what hided refuses", () => {
    expect(replyProblem("  ")).toBe("empty");
    expect(replyProblem("x".repeat(2001))).toBe("too_long");
    expect(replyProblem("a\nb")).toBe("control_characters");
    expect(replyProblem("yes, 계속")).toBeNull();
  });
});

function agent(group: AgentGroup["group"], pane: string, root = pane, demand = ""): PhoneAgent {
  return {
    device_id: "local",
    pane_id: pane,
    root_pane_id: root,
    group,
    symbol: "!",
    tone: "warning",
    agent_kind: "claude",
    title: pane,
    place: null,
    device_label: null,
    elapsed: "1m",
    line: null,
    status_label: "",
    demand,
  };
}

describe("staleTags", () => {
  it("keeps the notification of a root still waiting or done, through a child's request", () => {
    const groups: AgentGroup[] = [
      { group: "needs_you", agents: [agent("needs_you", "child", "root")] },
      { group: "seen", agents: [agent("seen", "other")] },
    ];
    expect(staleTags(["local|root", "local|other", "local|gone"], groups)).toEqual(["local|other", "local|gone"]);
  });

  it("keeps a root's notification while a delegated child, only ever Working, still asks", () => {
    const groups: AgentGroup[] = [
      { group: "working", agents: [agent("working", "child", "root", "question")] },
      { group: "seen", agents: [agent("seen", "root")] },
    ];
    expect(staleTags(["local|root"], groups)).toEqual([]);
    expect(staleTags(["local|root"], [{ group: "seen", agents: [agent("seen", "root")] }])).toEqual(["local|root"]);
  });
});

describe("notificationRow", () => {
  const base = { pushMode: "always" as const, notifications: "unasked" as const, permission: "default" as const, supported: true };

  it("offers 알림 켜기 only while push is on and the phone has not answered", () => {
    expect(notificationRow(base)).toBe("enable");
    expect(notificationRow({ ...base, pushMode: "off" })).toBeNull();
    expect(notificationRow({ ...base, notifications: "on", permission: "granted" })).toBeNull();
  });

  it("sends a Safari tab to the Home Screen first and names a refused permission", () => {
    expect(notificationRow({ ...base, supported: false, permission: "unsupported" })).toBe("install_first");
    expect(notificationRow({ ...base, permission: "denied" })).toBe("denied");
  });
});

describe("header", () => {
  it("names the Mac from its ts.net address and counts the other phones", () => {
    expect(macNameOf("https://hoyeon-mbp.tailnet.ts.net")).toBe("hoyeon-mbp");
    expect(headerLine("mac", 1)).toBe("mac · 폰 1대 더 연결됨");
    expect(headerLine("mac", 0)).toBe("mac");
  });
});

describe("rowsProblem", () => {
  it("names a closed pane and a device that is not connected (B28)", () => {
    expect(rowsProblem("ok")).toBeNull();
    expect(rowsProblem("gone")).toBe("이 pane은 더 이상 열려 있지 않아요.");
    expect(rowsProblem("device_unreachable")).toBe("이 에이전트의 기기가 연결돼 있지 않아요.");
  });
});

describe("boxDrawingRow", () => {
  it("is a row of box-drawing characters and spaces only", () => {
    expect(boxDrawingRow("─".repeat(200))).toBe(true);
    expect(boxDrawingRow("  ╭────╮  ")).toBe(true);
    expect(boxDrawingRow("│ ❯ ")).toBe(false);
    expect(boxDrawingRow("── 3 files ──")).toBe(false);
    expect(boxDrawingRow("")).toBe(false);
    expect(boxDrawingRow("    ")).toBe(false);
  });
});
