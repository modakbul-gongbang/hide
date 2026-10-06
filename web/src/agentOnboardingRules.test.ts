import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { agentLogo, monogram } from "./agentLogos";
import { appliedAgents, selection, tileSwitchable } from "./agentOnboardingRules";
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
    expect([...selection(agents, new Set())]).toEqual(["claude-code", "codex"]);
    // The operator's flip turns one off; a flip of a tile with no switch changes nothing.
    expect([...selection(agents, new Set(["codex", "cursor"]))]).toEqual(["claude-code"]);
    expect(agents.map(tileSwitchable)).toEqual([true, false, true, false]);
  });

  it("sends the agents still on in the adapters' order, and nothing for a tile with no switch", () => {
    expect(appliedAgents(agents, new Set())).toEqual(["claude-code", "codex"]);
    expect(appliedAgents(agents, new Set(["claude-code", "codex", "cursor"]))).toEqual([]);
  });

  it("draws every mark the logo manifest lists, in whatever format it is bundled, and a monogram for the rest", () => {
    type Entry = { id: string };
    const manifest = JSON.parse(fs.readFileSync(path.resolve(__dirname, "assets/agents/manifest.json"), "utf8")) as Record<"logos" | "existing" | "monogram", Entry[]>;
    const drawn = [...manifest.logos, ...manifest.existing].map(({ id }) => id);
    expect(drawn.filter((id) => agentLogo(id) === null)).toEqual([]);
    expect(manifest.monogram.filter(({ id }) => agentLogo(id) !== null)).toEqual([]);
  });

  it("makes a monogram from a name without drawing anything", () => {
    expect(monogram("Gemini CLI")).toBe("GC");
    expect(monogram("Kiro")).toBe("KI");
    expect(monogram("Factory Droid")).toBe("FD");
    expect(monogram("")).toBe("?");
  });
});
