// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { createActions } from "../actions";
import { TooltipProvider } from "../components/ui/tooltip";
import type { Device, DeviceHost, Kit, KitComponent } from "../snapshot";
import { useShellStore } from "../store";
import type { DispatchFn } from "../ws";
import { DevicesTab } from "./DevicesTab";

// The shell's modules reach xterm, which asks jsdom for a canvas it lacks.
vi.hoisted(() => {
  HTMLCanvasElement.prototype.getContext = () => null;
});

const part = (id: KitComponent["id"], state: KitComponent["state"], over: Partial<KitComponent> = {}): KitComponent => ({ id, label: id === "codex_hook" ? "Codex hook" : id === "claude_code_hook" ? "Claude Code hook" : "hide command", state, reason: null, location: "~/somewhere", ...over });

const kit = (components: KitComponent[], over: Partial<Kit> = {}): Kit => ({ unavailable: null, busy: false, components, agents: [], offers_reinstall: components.some((c) => ["outdated", "not_installed", "removed", "failed"].includes(c.state)), shares_account_with: null, ...over });

const host: DeviceHost = { consent: "granted", helper_root: "~/.hide/host-helper", cli_dir: "~/.local/bin", contract: 3, bound_identity: "SHA256:abc", granted_at_unix_ms: 1, state: "ready", message: null, platform: "macos aarch64", helper_path: "~/.hide/host-helper/current/hided" };

const remote = (id: string, over: Partial<Device> = {}): Device => ({ id, label: id === "mini" ? "Mac mini" : "Studio", kind: "remote", state: "ready", message: null, ssh_alias: id, agent_count: 0, test: null, host, kit: kit([part("cli", "installed"), part("claude_code_hook", "installed"), part("codex_hook", "installed")]), ...over });

const state = (devices: Device[]) => ({
  connection: "live" as const,
  rest: {
    navigator: { focused_device_id: "local", devices: [{ id: "local", label: "This Mac", kind: "local", state: "local", message: null, ssh_alias: null, agent_count: 0, test: null, kit: kit([part("cli", "installed")]) }, ...devices] },
    status: { remote: devices.map((device) => ({ target_id: device.id, state: "connected", message: null, herdr_version: "0.9.1" })) },
    ui_state: {},
  },
});

afterEach(() => {
  document.body.innerHTML = "";
});

async function mount(next: ReturnType<typeof state>) {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} unobserve() {} });
  const events: Parameters<DispatchFn>[0][] = [];
  const actions = createActions((event) => { events.push(event); return true; });
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const saved = useShellStore.getState();
  await act(async () => {
    useShellStore.setState(next as never);
    root.render(<TooltipProvider><DevicesTab actions={actions} /></TooltipProvider>);
  });
  const q = (selector: string) => container.querySelector(selector) as HTMLElement | null;
  return { events, q, row: (id: string) => q(`[data-device-row="${id}"]`), unmount: async () => { await act(async () => root.unmount()); useShellStore.setState(saved, true); } };
}

const sent = (events: Parameters<DispatchFn>[0][], kind: string) => events.filter((event) => event.kind === kind);

it("says nothing about a healthy kit on a device row, and names the device by alias, platform and Herdr version (B54)", async () => {
  const { row, q, unmount } = await mount(state([remote("mini")]));
  const mini = row("mini")!;
  expect(q('[data-device-subtitle="mini"]')?.textContent).toBe("mini · macos aarch64 · Herdr 0.9.1");
  expect(mini.querySelector("[data-machine-kit], [data-kit-part], [data-kit-problem], [data-kit-reinstall], [data-device-host]")).toBeNull();
  // The old rows named the hide command, the hook files and the helper: none of it is on the row.
  expect(mini.textContent).not.toMatch(/hide command|Claude Code hook|Codex hook|installs to|bound to/);
  // Test and the menu are the only controls a healthy device row carries.
  expect(mini.querySelectorAll("button")).toHaveLength(2);
  expect(q('[data-device-test="mini"]')).not.toBeNull();
  expect(q('[data-device-menu="mini"]')?.getAttribute("aria-label")).toBe("More actions for Mac mini");
  await unmount();
});

it("grows one line under the device for a failed part, with Reinstall, and the line is gone once the kit is whole again (B54)", async () => {
  const broken = remote("mini", { kit: kit([part("cli", "installed"), part("codex_hook", "failed", { reason: "hooks.json is not valid JSON" })]) });
  const first = await mount(state([broken]));
  const line = first.q('[data-kit-problem="mini"]')!;
  expect(line.textContent).toBe("✕Hide's kit: Codex hook: Failed: hooks.json is not valid JSON" + "Reinstall");
  await act(async () => { first.q('[data-kit-reinstall="mini"]')?.click(); });
  expect(sent(first.events, "kit_reinstall")).toHaveLength(1);
  await first.unmount();

  const whole = await mount(state([remote("mini")]));
  expect(whole.q('[data-kit-problem="mini"]')).toBeNull();
  expect(whole.q('[data-kit-reinstall="mini"]')).toBeNull();
  await whole.unmount();
});

it("does not call a part that is not on the machine, or one the operator switched off, a problem, and never repeats its reason (B56)", async () => {
  const quiet = remote("mini", { kit: kit([part("cli", "installed"), part("codex_hook", "absent", { reason: "Codex is not set up on this machine" }), part("claude_code_hook", "off")]) });
  const { row, unmount } = await mount(state([quiet]));
  expect(row("mini")!.textContent).not.toContain("Not on this machine");
  expect(row("mini")!.textContent).not.toContain("Coordination");
  await unmount();
});

it("shows a reinstall under way, and the first install, as one pending line", async () => {
  const reinstalling = await mount(state([remote("mini", { kit: kit([part("claude_code_hook", "removed")], { busy: true }) })]));
  expect(reinstalling.q('[data-kit-reinstall="mini"]')?.hasAttribute("disabled")).toBe(true);
  expect(reinstalling.q('[data-kit-reinstall="mini"]')?.textContent).toBe("Reinstalling…");
  await reinstalling.unmount();

  const first = await mount(state([remote("mini", { kit: kit([], { busy: true }) })]));
  expect(first.q('[data-machine-kit="mini:busy"]')?.textContent).toContain("Installing");
  await first.unmount();
});

it("shows the helper only when it needs the operator while the connection is fine, with the actions that fix it (B54)", async () => {
  const stopped = remote("mini", { host: { ...host, state: "unavailable", message: "The helper did not answer." } });
  const unavailable = await mount(state([stopped]));
  expect(unavailable.q('[data-device-host="mini:unavailable"]')?.textContent).toContain("helper unavailable");
  await act(async () => { unavailable.q('[data-device-host-retry="mini"]')?.click(); });
  expect(sent(unavailable.events, "device_host_retry")).toHaveLength(1);
  await unavailable.unmount();

  const notAllowed = await mount(state([remote("studio", { host: { ...host, consent: "none", state: "not_allowed", helper_root: null } })]));
  expect(notAllowed.q('[data-device-host="studio:not_allowed"]')?.textContent).toContain("helper not allowed");
  expect(notAllowed.q('[data-device-host-allow-inline="studio"]')).not.toBeNull();
  await notAllowed.unmount();

  // A device that is not connected says so on its row and nothing about its helper.
  const down = await mount(state([remote("mini", { state: "unavailable", host: { ...host, state: "unavailable" } })]));
  expect(down.q('[data-device-host="mini:unavailable"]')).toBeNull();
  await down.unmount();
});

it("opens Add device from one button above the list, and shows the Add dialog only then (B50)", async () => {
  const { q, unmount } = await mount(state([remote("mini")]));
  expect(q("[data-add-device]")).toBeNull();
  await act(async () => { q("[data-device-add-open]")?.click(); });
  expect(document.body.querySelector("[data-add-device-dialog]")).not.toBeNull();
  await unmount();
});
