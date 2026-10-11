import { describe, expect, it } from "vitest";
import { emptyHome, failedAgents, firstRunStep, firstStartAgent, notificationState, tileStatus, whatChanges } from "./firstRun";
import type { AiProvider, Kit, KitAgent, SnapshotRest } from "./snapshot";

const agent = (id: string, overrides: Partial<KitAgent> = {}): KitAgent => ({
  id,
  label: id,
  availability: "available",
  enabled: false,
  chosen: false,
  skill: { state: "off", reason: null, location: `/home/.${id}/skills/hide` },
  hook: null,
  doc_url: "https://example.test",
  ...overrides,
});

const provider = (agentId: string, state: string): AiProvider => ({ id: agentId, label: agentId, agent: agentId, state, headline: "", message: null, installed: true, selectable: state === "ready", retry_at_ms: null, model: "", models: [], models_fixed: false, cli_default: true, models_unavailable_reason: null });

const rest = (first_run: unknown) => ({ ui_state: { first_run } }) as unknown as SnapshotRest;

describe("the step on screen", () => {
  it("is the core's, for this flow version, and only the first in a browser tab (D-14)", () => {
    expect(firstRunStep(rest({ version: 1, step: "notifications" }), true)).toBe("notifications");
    expect(firstRunStep(rest({ version: 1, step: "notifications" }), false)).toBeNull();
    expect(firstRunStep(rest({ version: 1, step: "agents" }), false)).toBe("agents");
    expect(firstRunStep(rest({ version: 1, step: "done" }), true)).toBeNull();
    expect(firstRunStep(rest({ version: 2, step: "agents" }), true)).toBeNull();
    expect(firstRunStep(rest(undefined), true)).toBeNull();
  });
});

describe("a tile's line (B8, B9)", () => {
  it("says sign-in only for the agents Hide checks, and never invents it for the others", () => {
    expect(tileStatus(agent("claude-code"), undefined)).toBe("checking");
    expect(tileStatus(agent("claude-code"), [provider("claude-code", "unread")])).toBe("checking");
    expect(tileStatus(agent("codex"), [provider("codex", "ready")])).toBe("ready");
    expect(tileStatus(agent("codex"), [provider("codex", "usage_limited")])).toBe("ready");
    expect(tileStatus(agent("grok"), [provider("grok", "needs_login")])).toBe("not_signed_in");
    expect(tileStatus(agent("opencode"), [provider("opencode", "needs_login")])).toBe("installed");
    expect(tileStatus(agent("cursor", { availability: "not_installed" }), undefined)).toBe("not_installed");
  });
});

describe("What changes (B11)", () => {
  it("lists each piece's place for the agents on, skips a piece the agent lacks, and ends with the hide link", () => {
    const kit = { components: [{ id: "cli", label: "hide", state: "off", reason: null, location: "/home/.local/bin/hide" }] } as unknown as Kit;
    const agents = [
      agent("claude-code", { hook: { state: "off", reason: null, location: "/home/.claude/settings.json" }, herdr: { state: "absent", reason: "none", location: null } }),
      agent("codex"),
    ];
    expect(whatChanges(agents, new Set(["claude-code"]), kit)).toEqual({
      agents: [{ id: "claude-code", label: "claude-code", locations: ["/home/.claude/settings.json", "/home/.claude-code/skills/hide"] }],
      cli: "/home/.local/bin/hide",
    });
  });
});

describe("a failed agent (B12)", () => {
  it("is a chosen agent that is on with a failed piece, named with the kit's reason", () => {
    const agents = [agent("claude-code", { enabled: true, hook: { state: "failed", reason: "read-only file", location: "/x" } }), agent("codex", { enabled: true, skill: { state: "failed", reason: "denied", location: "/y" } })];
    expect(failedAgents(agents, ["claude-code"])).toEqual([{ id: "claude-code", label: "claude-code", reason: "read-only file" }]);
  });
});

describe("the start panel's first agent (D-15)", () => {
  it("is the first signed-in agent that is on, then the first that is on, then none", () => {
    const on = (id: string) => agent(id, { enabled: true });
    expect(firstStartAgent([on("claude-code"), on("codex")], [provider("claude-code", "needs_login"), provider("codex", "ready")])).toBe("codex");
    expect(firstStartAgent([on("opencode"), on("codex")], [provider("codex", "needs_login")])).toBe("opencode");
    expect(firstStartAgent([agent("codex")], [provider("codex", "ready")])).toBeNull();
  });
});

describe("the notification line (B14-B19, B30)", () => {
  it("follows the switch and what the system says", () => {
    expect(notificationState(false, "granted")).toBe("off");
    expect(notificationState(true, "not_determined")).toBe("asking");
    expect(notificationState(true, "granted")).toBe("on");
    expect(notificationState(true, "denied")).toBe("blocked");
    expect(notificationState(false, "denied")).toBe("blocked");
    expect(notificationState(true, "unknown")).toBe("unreadable");
    expect(notificationState(true, "unsupported")).toBe("on_unchecked");
    expect(notificationState(false, "unsupported")).toBe("off");
  });
});

describe("the empty Home (D-16, B27-B29)", () => {
  it("draws the graph whenever it has an agent, and otherwise the next step", () => {
    expect(emptyHome(0, 0)).toBe("no_project");
    expect(emptyHome(2, 0)).toBe("no_agent");
    expect(emptyHome(0, 1)).toBeNull();
  });
});
