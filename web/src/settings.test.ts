import { describe, expect, it } from "vitest";
import tokensText from "../../design/tokens.json?raw";
import { initializeInterfaceI18n } from "./i18n/instance";
import {
  ACCENT_CHOICES,
  canRetryDevice,
  deviceFacts as deviceFactsIn,
  deviceIdFor,
  deviceProblemLine as deviceProblemLineIn,
  deviceRemovalLines as deviceRemovalLinesIn,
  deviceLine as deviceLineIn,
  diagnosticsText,
  kitConsentTerms as kitConsentTermsIn,
  kitRemovalLine as kitRemovalLineIn,
  herdrLine as herdrLineIn,
  hostLine as hostLineIn,
  kitHookMachines,
  kitPartLine as kitPartLineIn,
  kitPartNeedsReinstall,
  kitPartSwitch,
  offeredModels,
  redact,
  shownIn,
  sleepAfterLabel,
  SLEEP_AFTER_CHOICES,
  socketProblem as socketProblemIn,
  usableAccent,
  usableFontSize,
  unstoredDeviceDrafts,
} from "./settings";
import type { AiProvider, Device, DeviceHost, KitComponent, RemoteStatus } from "./snapshot";

// The English strings are what the sheet shipped with; the rules read the same under them.
const t = initializeInterfaceI18n("en").getFixedT(null, "translation");
const korean = initializeInterfaceI18n("ko").getFixedT(null, "translation");
const deviceFacts = (device: Device, remote: RemoteStatus | undefined) => deviceFactsIn(device, remote, t);
const deviceProblemLine = (problem: string | null | undefined, alias: string | null) => deviceProblemLineIn(problem, alias, t);
const deviceRemovalLines = (deviceId: string, registrations: Parameters<typeof deviceRemovalLinesIn>[1], tabs: Parameters<typeof deviceRemovalLinesIn>[2], drafts: Parameters<typeof deviceRemovalLinesIn>[3], onlyExported: (tabId: string) => boolean = () => false) =>
  deviceRemovalLinesIn(deviceId, registrations, tabs, drafts, onlyExported, t);
const deviceLine = (device: Device, remote: RemoteStatus | undefined) => deviceLineIn(device, remote, t);
const kitConsentTerms = (helperRoot: string | null, cliDir: string | null) => kitConsentTermsIn(helperRoot, cliDir, t);
const kitRemovalLine = (device: Device) => kitRemovalLineIn(device, t);
const herdrLine = (herdr: Parameters<typeof herdrLineIn>[0]) => herdrLineIn(herdr, t);
const hostLine = (host: DeviceHost | undefined) => hostLineIn(host, t);
const kitPartLine = (part: KitComponent) => kitPartLineIn(part, t);
const socketProblem = (path: string) => socketProblemIn(path, t);

const device = (patch: Partial<Device>): Device => ({
  id: "studio",
  label: "Studio",
  kind: "remote",
  state: "unavailable",
  message: null,
  ssh_alias: "studio",
  agent_count: 0,
  test: null,
  ...patch,
});

describe("where settings live and what a device reported", () => {
  it("shows only facts the device reported", () => {
    expect(deviceFacts(device({}), undefined)).toEqual([]);
    const host = { state: "ready", platform: "macos aarch64" } as DeviceHost;
    const status = { target_id: "studio", state: "connected", message: null, herdr_version: "0.9.1" };
    expect(deviceFacts(device({ host }), status)).toEqual(["Herdr 0.9.1", "helper on macos aarch64"]);
    expect(deviceFacts(device({ host: { ...host, state: "connecting" } }), status)).toEqual(["Herdr 0.9.1"]);
    expect(deviceFacts(device({ kind: "local", host }), status)).toEqual([]);
  });

  it("tells a changed host key, an unknown one and a refused sign-in apart", () => {
    expect(deviceProblemLine("host_key_changed", "studio")?.headline).toBe("Host key changed");
    expect(deviceProblemLine("host_key_unknown", "studio")?.action).toContain("ssh studio");
    expect(deviceProblemLine("authentication", "studio")?.headline).toBe("Sign-in refused");
    expect(deviceProblemLine(null, "studio")).toBeNull();
  });
});

describe("deviceRemovalLines", () => {
  it("counts the device's projects, tabs and drafts and nothing of this machine's", () => {
    const lines = deviceRemovalLines(
      "mini",
      [{ device_id: "mini" }, { device_id: "local" }],
      [
        { id: "t1", checkout_id: "remote:mini:checkout:w1", dirty: true },
        { id: "t2", checkout_id: "remote:mini:checkout:w2", dirty: false },
        { id: "t3", checkout_id: "checkout:here", dirty: true },
      ],
      [{ device: "mini" }, { device: "local" }, { device: null }],
    );
    expect(lines).toEqual([
      "Hide forgets 1 registered project and closes 2 file tabs of it here.",
      "2 unsaved drafts stay in this browser under unsaved drafts, to export or discard.",
    ]);
    expect(deviceRemovalLines("studio", [], [], [])).toEqual([]);
  });

  it("says a draft that was only exported leaves as that file, not as a stored draft (B44)", () => {
    const tabs = [
      { id: "t1", checkout_id: "remote:mini:checkout:w1", dirty: true },
      { id: "t2", checkout_id: "remote:mini:checkout:w1", dirty: true },
    ];
    expect(deviceRemovalLines("mini", [], tabs, [], (tabId) => tabId === "t1")).toEqual([
      "Hide forgets 0 registered projects and closes 2 file tabs of it here.",
      "1 unsaved draft stays in this browser under unsaved drafts, to export or discard.",
      "1 draft could not be stored in this browser and leaves only as the file you exported.",
    ]);
  });
});

describe("settings rules", () => {
  it("keeps only accents and sizes the sheet can offer", () => {
    // Every offered accent is a token the design authority defines, and each
    // one's value is an accent the page can store and draw.
    const tokens = (JSON.parse(tokensText) as { tokens: Record<string, { value: string }> }).tokens;
    for (const choice of ACCENT_CHOICES) {
      const value = tokens[choice.token]?.value;
      expect(value, choice.token).toBeDefined();
      expect(usableAccent(value)).toBe(value?.toLowerCase());
    }
    expect(usableAccent("red")).toBeNull();
    expect(usableAccent("#GGGGGG")).toBeNull();
    expect(usableFontSize(13)).toBe(13);
    expect(usableFontSize(10)).toBeNull();
    expect(usableFontSize(18)).toBeNull();
    expect(usableFontSize("13")).toBeNull();
  });

  it("names a device after its alias without colliding", () => {
    expect(deviceIdFor("Studio.Mac", ["local"])).toBe("studio-mac");
    expect(deviceIdFor("studio", ["local", "studio"])).toBe("studio-2");
    expect(deviceIdFor("local", ["local"])).toBe("local-2");
    expect(deviceIdFor("***", [])).toBe("device");
  });

  it("tells pending, failed and connected devices apart and offers Retry only when it is a new attempt", () => {
    expect(deviceLine(device({ state: "ready" }), undefined).tone).toBe("ok");
    const waiting = { target_id: "studio", state: "not_connected", message: null, herdr_version: null };
    expect(deviceLine(device({}), waiting).tone).toBe("pending");
    expect(canRetryDevice(device({}), waiting)).toBe(false);
    const failed = { ...waiting, state: "unreachable" };
    expect(deviceLine(device({}), failed).text).toBe("not connected");
    expect(canRetryDevice(device({}), failed)).toBe(true);
    expect(canRetryDevice(device({ state: "ready" }), undefined)).toBe(false);
    expect(canRetryDevice(device({ kind: "local", state: "local" }), undefined)).toBe(false);
  });

  it("names a protocol mismatch rather than a bare state", () => {
    expect(herdrLine({ state: "protocol_mismatch", expected_protocol: 3, received_protocol: 2 }).tone).toBe("error");
    expect(herdrLine({ state: "connected" }).text).toBe("Connected");
    expect(herdrLine(undefined).text).toBe("unavailable");
  });

  it("keeps the configured model among the offered ones", () => {
    const provider: AiProvider = {
      id: "claude",
      label: "Claude",
      state: "ready",
      headline: "Ready",
      message: null,
      model: "custom-model",
      models: ["opus", "sonnet"],
      models_unavailable_reason: null,
    };
    expect(offeredModels(provider)).toEqual(["custom-model", "opus", "sonnet"]);
  });

  it("copies diagnostics without the page token or any secret-shaped value", () => {
    const token = "a".repeat(64);
    const text = diagnosticsText({
      daemon: {
        version: "0.1.0",
        host_id: "host-00000000000000000000000000000000",
        host_name: "studio-host",
        schema_version: 2,
        pid: 42,
        started_at_unix: "1",
        state_dir: "/Users/example/.hide/state",
        core_state_path: "/Users/example/.hide/state/core-state.json",
        herdr_bin_path: null,
        herdr_socket_path: null,
        keep_alive: false,
        idle_secs: 600,
      },
      connection: "live",
      herdr: { state: "connected", received_version: "0.9.1" },
      environment: [],
      diagnostics: [{ kind: "ws.url", message: `opened #token=${token} with key=abc123`, occurred_at: 0 }],
      lastError: null,
      userAgent: "test",
    });
    expect(text).toContain("hided 0.1.0 pid 42");
    expect(text).toContain("herdr binary: unavailable");
    expect(text).not.toContain(token);
    expect(text).not.toContain("abc123");
    expect(redact("password=hunter2 ok")).toBe("password=[redacted] ok");
  });

  it("says whether a device's helper may run and never reads a refusal as ready", () => {
    const host = (patch: Partial<DeviceHost>): DeviceHost => ({
      consent: "granted",
      helper_root: "~/.hide/host-helper",
      cli_dir: "~/.local/bin",
      contract: 2,
      bound_identity: null,
      granted_at_unix_ms: 1,
      state: "ready",
      message: null,
      platform: "macos aarch64",
      helper_path: null,
      ...patch,
    });
    expect(hostLine(host({})).tone).toBe("ok");
    expect(hostLine(host({ consent: "none", state: "not_allowed" }))).toEqual({ text: "helper not allowed", tone: "muted" });
    expect(hostLine(host({ consent: "outdated", state: "not_allowed" })).text).toBe("helper needs a new consent");
    expect(hostLine(host({ state: "identity_changed" })).tone).toBe("warn");
    expect(hostLine(host({ state: "unavailable" })).tone).toBe("warn");
    expect(hostLine(host({ consent: "this_machine" })).tone).toBe("local");
  });

  it("names every kit part and where it goes in the one consent, and refuses a relative device socket", () => {
    const lines = kitConsentTerms("/opt/hide", "/opt/bin");
    const terms = lines.join(" ");
    for (const named of ["/opt/hide", "/opt/bin", "hook helper", "old records preserved", "~/.claude/settings.json", "~/.codex/hooks.json"]) {
      expect(terms).toContain(named);
    }
    expect(lines).toHaveLength(6);
    // The Codex part says once what it changes (PRD overview-request-view B33).
    expect(lines.filter((line) => line.includes("turns off Codex's background daemon"))).toHaveLength(1);
    expect(terms).toContain("~/hide");
    expect(terms).not.toContain("changes no hook");
    expect(socketProblem("")).toBeNull();
    expect(socketProblem("/tmp/herdr.sock")).toBeNull();
    expect(socketProblem("herdr.sock")).not.toBeNull();
    expect(socketProblem("/")).not.toBeNull();
    expect(socketProblem("/tmp/a\nb")).not.toBeNull();
  });
});

describe("unstoredDeviceDrafts (S5.5 B26, B44)", () => {
  it("names the device's tabs whose draft is only in the tab", () => {
    const tabs = [
      { id: "t1", checkout_id: "remote:mac:checkout:w1", path: "/r/a.txt" },
      { id: "t2", checkout_id: "remote:mac:checkout:w1", path: "/r/b.txt" },
      { id: "t3", checkout_id: "local:checkout", path: "/r/a.txt" },
    ];
    expect(unstoredDeviceDrafts("mac", tabs, new Set(["t2", "t3"]))).toEqual(["/r/b.txt"]);
    expect(unstoredDeviceDrafts("mac", tabs, new Set())).toEqual([]);
  });
});

describe("the install kit rows (PRD device-parity B7, B8, B27)", () => {
  const part = (id: KitComponent["id"], state: KitComponent["state"]): KitComponent => ({ id, label: id, state, reason: null, location: null });
  const kit = (components: KitComponent[], unavailable: string | null = null) => ({ unavailable, busy: false, components, offers_reinstall: false, shares_account_with: null });

  it("offers Reinstall only for a part a reinstall would change", () => {
    const offered = (["installed", "outdated", "not_installed", "removed", "failed", "absent", "off"] as const).filter((state) => kitPartNeedsReinstall(part("cli", state)));
    expect(offered).toEqual(["outdated", "not_installed", "removed", "failed"]);
    expect(kitPartLine(part("coordination_retirement", "removed"))).toEqual({ text: "Removed", tone: "warn" });
    expect(kitPartLine(part("coordination_retirement", "absent")).tone).toBe("muted");
  });

  it("gives the Codex part a switch while it has a setting to switch, and reads off as neutral (PRD overview-request-view B36)", () => {
    expect(kitPartSwitch(part("codex_per_pane", "installed"))).toEqual({ on: true });
    expect(kitPartSwitch(part("codex_per_pane", "off"))).toEqual({ on: false });
    expect(kitPartSwitch(part("codex_per_pane", "absent"))).toBeNull();
    expect(kitPartSwitch(part("codex_per_pane", "failed"))).toBeNull();
    expect(kitPartSwitch(part("cli", "installed"))).toBeNull();
    expect(kitPartLine(part("codex_per_pane", "off"))).toEqual({ text: "Off", tone: "muted" });
  });

  it("lists This Mac first and then each device, with only their hook parts", () => {
    const local = device({ id: "local", label: "mini", kind: "local", state: "ready", ssh_alias: null, kit: kit([part("cli", "installed"), part("claude_code_hook", "installed"), part("codex_hook", "absent")]) });
    const studio = device({ kit: kit([], "Allow the helper to install Hide on Studio") });
    const unchecked = device({ id: "box", label: "Box" });
    const machines = kitHookMachines([local, studio, unchecked]);
    expect(machines.map((machine) => machine.device.id)).toEqual(["local", "studio", "box"]);
    expect(machines[0]?.parts.map((row) => row.id)).toEqual(["claude_code_hook", "codex_hook"]);
    expect(machines[1]).toMatchObject({ parts: [], unavailable: "Allow the helper to install Hide on Studio" });
    expect(machines[2]).toMatchObject({ parts: [], unavailable: null });
  });

  it("says in one line what removing a device takes off it and what stays (B22, B24)", () => {
    const connected = device({ host: { state: "ready", helper_root: "~/.hide/host-helper" } as DeviceHost });
    expect(kitRemovalLine(connected)).toMatch(/removes its hook entries, its hide link and its helper folder \(~\/\.hide\/host-helper\); the records in ~\/\.hide stay/);
    const offline = device({ host: { state: "unavailable" } as DeviceHost });
    expect(kitRemovalLine(offline)).toMatch(/not connected .* its kit stays there/);
    const shared = device({ host: { state: "ready" } as DeviceHost, kit: { ...kit([]), shares_account_with: "Studio, second Herdr" } });
    expect(kitRemovalLine(shared)).toMatch(/^Studio, second Herdr reaches the same account on .*, so Hide's kit stays there for it\.$/);
  });
});

describe("in Korean", () => {
  it("words the removal summary, the sleep choices and a missing value in the interface language", () => {
    const lines = deviceRemovalLinesIn("mini", [{ device_id: "mini" }], [{ id: "t1", checkout_id: "remote:mini:checkout:w1", dirty: true }], [{ device: "mini" }], () => false, korean);
    expect(lines).toEqual([
      "여기서 프로젝트 1개의 등록을 지우고 파일 탭 1개을 닫습니다.",
      "저장하지 않은 초안 2개는 이 브라우저의 저장하지 않은 초안 목록에 남아 내보내거나 버릴 수 있습니다.",
    ]);
    expect(SLEEP_AFTER_CHOICES.map((choice) => sleepAfterLabel(choice, korean))).toEqual(["절전 안 함", "12시간", "24시간", "3일"]);
    expect(shownIn(undefined, korean)).toBe("사용할 수 없음");
    expect(shownIn("/state", korean)).toBe("/state");
    expect(kitPartLineIn({ id: "cli", label: "cli", state: "installed", reason: null, location: null }, korean).text).toBe("설치됨");
  });
});

