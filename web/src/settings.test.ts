import { describe, expect, it } from "vitest";
import tokensText from "../../design/tokens.json?raw";
import { ACCENT_CHOICES, canRetryDevice, deviceIdFor, deviceLine, diagnosticsText, helperConsentTerms, herdrLine, hostLine, offeredModels, redact, socketProblem, usableAccent, usableFontSize } from "./settings";
import type { AiProvider, Device, DeviceHost } from "./snapshot";

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
        schema_version: 2,
        pid: 42,
        started_at_unix: "1",
        state_dir: "/Users/example/.local/state/hide",
        core_state_path: "/Users/example/.local/state/hide/core-state.json",
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
      helper_root: "~/.local/share/hide/host-helper",
      contract: 1,
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

  it("names the install root in the consent and refuses a relative device socket", () => {
    expect(helperConsentTerms("/opt/hide")[0]).toContain("/opt/hide");
    expect(socketProblem("")).toBeNull();
    expect(socketProblem("/tmp/herdr.sock")).toBeNull();
    expect(socketProblem("herdr.sock")).not.toBeNull();
    expect(socketProblem("/")).not.toBeNull();
    expect(socketProblem("/tmp/a\nb")).not.toBeNull();
  });
});
