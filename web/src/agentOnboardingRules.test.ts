import { describe, expect, it } from "vitest";
import { monogram } from "./agentLogos";
import { appliedAgents, initialSelection, tileSwitchable } from "./agentOnboardingRules";
import type { KitAgent } from "./snapshot";

const agent = (id: string, availability: KitAgent["availability"]): KitAgent => ({
  id,
  label: id,
  availability,
  enabled: false,
  skill: { state: "off", reason: null, location: null },
  hook: null,
  doc_url: "https://example.test",
});

describe("the first-run agent choice", () => {
  const agents = [agent("claude-code", "available"), agent("cursor", "not_installed"), agent("codex", "available"), agent("amp", "unsupported_system")];

  it("starts with the agents that are set up on, and never switches on one that is not", () => {
    expect([...initialSelection(agents)]).toEqual(["claude-code", "codex"]);
    expect(agents.map(tileSwitchable)).toEqual([true, false, true, false]);
  });

  it("sends the agents still on in the adapters' order, and nothing for a tile with no switch", () => {
    expect(appliedAgents(agents, new Set(["codex", "claude-code", "cursor"]))).toEqual(["claude-code", "codex"]);
    expect(appliedAgents(agents, new Set())).toEqual([]);
  });

  it("makes a monogram from a name without drawing anything", () => {
    expect(monogram("Gemini CLI")).toBe("GC");
    expect(monogram("Kiro")).toBe("KI");
    expect(monogram("Factory Droid")).toBe("FD");
    expect(monogram("")).toBe("?");
  });
});
