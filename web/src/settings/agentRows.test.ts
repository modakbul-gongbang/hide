import { describe, expect, it } from "vitest";
import type { Device, KitAgent, KitPiece } from "../snapshot";
import { agentInstalled, agentLeftover, checkFailedReason, docsUrl, agentMachines, agentProblems, agentStatus, installDocUrl, supportedAgents, SUPPORTED_AGENTS } from "./agentRows";

const piece = (state: KitPiece["state"], reason: string | null = null): KitPiece => ({ state, reason, location: null });
const agent = (id: string, patch: Partial<KitAgent> = {}): KitAgent => ({
  id,
  label: id,
  availability: "available",
  enabled: true,
  chosen: false,
  skill: piece("installed"),
  hook: null,
  doc_url: "https://example.test",
  ...patch,
});
const sessions = (connected: number, not: number, hidden = 0): KitAgent["sessions"] => ({
  connected,
  not_connected: Array.from({ length: not }, (_, index) => ({ pane_id: `p${index}`, title: "t", project: "p", reason: "started_before_hide" as const })),
  not_connected_hidden: hidden,
});
const device = (patch: Partial<Device> & { agents?: KitAgent[]; unavailable?: string | null }): Device => {
  const { agents = [], unavailable = null, ...rest } = patch;
  return {
    id: "studio",
    label: "Studio",
    kind: "remote",
    state: "ready",
    message: null,
    ssh_alias: "studio",
    agent_count: 0,
    test: null,
    kit: { unavailable, busy: false, components: [], agents, offers_reinstall: false, shares_account_with: null },
    ...rest,
  } as Device;
};

describe("the agents of a machine (B8, B9, D-06)", () => {
  it("lists the seven supported agents in their order and nothing a machine still reports from before", () => {
    const reported = ["cursor", "kiro", "claude-code", "amp", "pi", "codex"].map((id) => agent(id));
    expect(supportedAgents(reported).map((row) => row.id)).toEqual(["claude-code", "codex", "pi", "cursor"]);
    expect(SUPPORTED_AGENTS).toHaveLength(7);
  });

  it("calls an agent installed only when its program was found; an agent the operator switched on keeps its switch after its program goes (B9)", () => {
    // The program is there.
    expect(agentInstalled(agent("codex"))).toBe(true);
    // The program is not there and nothing is on: not installed.
    expect(agentInstalled(agent("codex", { availability: "not_installed", enabled: false }))).toBe(false);
    expect(agentInstalled(agent("codex", { availability: "unsupported_system", enabled: false }))).toBe(false);
    // On only because Claude Code and Codex are on by default (no recorded choice): not installed, and never Ready.
    const byDefault = agent("codex", { availability: "not_installed", enabled: true, chosen: false, sessions: sessions(0, 0) });
    expect(agentInstalled(byDefault)).toBe(false);
    expect(agentInstalled(agent("codex", { availability: "not_installed", enabled: true }))).toBe(false);
    expect(agentStatus(byDefault)).toEqual({ kind: "none" });
    // The operator switched it on and its program then went away: it stays listed so it can be switched off.
    const gone = agent("codex", { availability: "not_installed", enabled: true, chosen: true, sessions: sessions(0, 0) });
    expect(agentInstalled(gone)).toBe(true);
    expect(agentStatus(gone)).toEqual({ kind: "none" });
  });

  it("splits each machine into the installed and the not installed, This Mac first, and says why a machine has no list", () => {
    const set = [agent("claude-code"), agent("codex", { enabled: false }), agent("pi", { availability: "not_installed", enabled: false })];
    const local = device({ id: "local", label: "mini", kind: "local", state: "local", agents: set });
    const gone = device({ id: "box", label: "Box", state: "unavailable", agents: set });
    const noKit = device({ id: "bare", label: "Bare", unavailable: "A daemon outside the package installs nothing" });
    const unread = device({ id: "new", label: "New" });
    const machines = agentMachines([local, gone, noKit, unread]);
    expect(machines.map((row) => row.device.id)).toEqual(["local", "box", "bare", "new"]);
    expect(machines[0]).toMatchObject({ blocked: null, unread: false });
    expect(machines[0]?.installed.map((row) => row.id)).toEqual(["claude-code", "codex"]);
    expect(machines[0]?.notInstalled.map((row) => row.id)).toEqual(["pi"]);
    expect(machines[1]?.blocked).toBe("unreachable");
    expect(machines[2]).toMatchObject({ blocked: "unavailable", unread: false });
    expect(machines[3]).toMatchObject({ blocked: null, unread: true });
  });

  it("says why a machine shows no list in the Devices tab's own words: connecting is not unreachable, and Switched off points at Devices (B11)", () => {
    const set = [agent("claude-code")];
    const down = device({ id: "box", state: "unavailable", agents: set });
    const remote = (state: "not_connected" | "connecting" | "ready" | "failed"): never => [{ target_id: "box", state }] as never;
    expect(agentMachines([down], remote("connecting"))[0]?.blocked).toBe("connecting");
    expect(agentMachines([down], remote("not_connected"))[0]?.blocked).toBe("connecting");
    expect(agentMachines([down], remote("failed"))[0]?.blocked).toBe("unreachable");
    expect(agentMachines([down])[0]?.blocked).toBe("unreachable");
    expect(agentMachines([{ ...down, state: "disabled" } as Device], remote("failed"))[0]?.blocked).toBe("disabled");
    expect(agentMachines([{ ...down, state: "ready" } as Device], remote("failed"))[0]?.blocked).toBeNull();
  });

  it("opens each agent's installation guide and none for an agent Hide does not support", () => {
    for (const id of SUPPORTED_AGENTS) expect(installDocUrl(id)).toMatch(/^https:\/\//);
    expect(installDocUrl("kiro")).toBeNull();
    // The Docs link is this build's table, never a URL a machine reports (a device's helper supplies `doc_url`).
    for (const id of SUPPORTED_AGENTS) expect(docsUrl(id)).toMatch(/^https:\/\//);
    expect(docsUrl("kiro")).toBeNull();
  });

  it("words a failed check from a code and reads a code it does not know as the general line (B10, B68)", () => {
    expect(checkFailedReason("unreachable")).toBe("agents.checkReason.unreachable");
    expect(checkFailedReason("timed_out")).toBe("agents.checkReason.timed_out");
    expect(checkFailedReason("something_new")).toBe("agents.checkReason.failed");
  });
});

describe("what a row says (B13, B14, B16, B19, B20)", () => {
  it("names the pieces a Reinstall would repair, the failed ones first, only for an agent that is on (B13, B20)", () => {
    const row = agent("codex", { hook: piece("removed"), herdr: piece("failed", "herdr refused"), skill: piece("outdated") });
    expect(agentProblems(row).map(({ part }) => part)).toEqual(["herdr", "skill", "hook"]);
    expect(agentProblems(agent("codex", { hook: piece("absent", "no ~/.codex"), skill: piece("installed"), herdr: piece("off") }))).toEqual([]);
    expect(agentProblems(agent("codex", { enabled: false, skill: piece("removed") }))).toEqual([]);
    // Gemini CLI has no Herdr integration: nothing to fail (B15).
    expect(agentProblems(agent("gemini-cli", { herdr: null }))).toEqual([]);
  });

  it("keeps a switched-off agent whose removal did not finish from reading Off (B14)", () => {
    expect(agentLeftover(agent("pi", { enabled: false, skill: piece("failed", "still there") }))?.reason).toBe("still there");
    expect(agentLeftover(agent("pi", { enabled: false }))).toBeNull();
    expect(agentLeftover(agent("pi", { enabled: true, skill: piece("failed") }))).toBeNull();
  });

  it("says Ready, how many sessions are connected and how many are not, and nothing for the rest", () => {
    expect(agentStatus(agent("codex", { sessions: sessions(0, 0) }))).toEqual({ kind: "ready" });
    expect(agentStatus(agent("codex", { sessions: sessions(3, 0) }))).toEqual({ kind: "sessions", connected: 3, notConnected: 0 });
    // The list holds at most 32; the rest are counted, so the number is never short (B17).
    expect(agentStatus(agent("codex", { sessions: sessions(1, 2, 14) }))).toEqual({ kind: "sessions", connected: 1, notConnected: 16 });
    expect(agentStatus(agent("codex", { enabled: false, sessions: sessions(3, 0) }))).toEqual({ kind: "none" });
    expect(agentStatus(agent("grok", { partial: true, sessions: null }))).toEqual({ kind: "none" });
    expect(agentStatus(agent("grok", { partial: true, sessions: sessions(2, 0) }))).toEqual({ kind: "none" });
    expect(agentStatus(agent("codex", { sessions: null }))).toEqual({ kind: "none" });
  });
});
