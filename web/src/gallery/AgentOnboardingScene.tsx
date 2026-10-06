// The first-run agent choice on invented data: the production gate over a
// synthetic local device that lists every adapter, no daemon. The installed
// set mirrors a typical machine so the on, off and not-installed tiles all show.
import { useLayoutEffect, useMemo } from "react";
import { createActions } from "../actions";
import { AgentOnboardingGate } from "../AgentOnboarding";
import { TooltipProvider } from "../components/ui/tooltip";
import type { KitAgent } from "../snapshot";
import { useShellStore } from "../store";

const AGENTS: [string, string, KitAgent["availability"]][] = [
  ["claude-code", "Claude Code", "available"],
  ["codex", "Codex", "available"],
  ["opencode", "OpenCode", "available"],
  ["gemini-cli", "Gemini CLI", "available"],
  ["cursor", "Cursor", "available"],
  ["copilot-cli", "GitHub Copilot CLI", "not_installed"],
  ["amp", "Amp", "not_installed"],
  ["factory-droid", "Factory Droid", "available"],
  ["kiro", "Kiro", "not_installed"],
  ["qwen-code", "Qwen Code", "not_installed"],
  ["goose", "Goose", "not_installed"],
  ["cline", "Cline", "not_installed"],
  ["kilo-code", "Kilo Code", "not_installed"],
  ["crush", "Crush", "not_installed"],
  ["junie", "Junie", "not_installed"],
  ["augment", "Augment", "not_installed"],
  ["pi", "Pi", "not_installed"],
  ["grok", "Grok", "not_installed"],
  ["kimi-code", "Kimi Code", "not_installed"],
  ["mistral-vibe", "Mistral Vibe", "not_installed"],
];

export function AgentOnboardingScene({ theme }: { theme: "light" | "dark" }) {
  const actions = useMemo(() => createActions(() => true), []);
  useLayoutEffect(() => {
    const agents: KitAgent[] = AGENTS.map(([id, label, availability]) => ({
      id,
      label,
      availability,
      enabled: false,
      skill: { state: "off", reason: null, location: null },
      hook: null,
      doc_url: "https://example.test",
    }));
    useShellStore.setState({
      connection: "live",
      rest: { ui_state: { agent_onboarding: "pending" }, navigator: { devices: [{ id: "local", kit: { agents } }] } },
    } as never);
    document.documentElement.classList.toggle("dark", theme === "dark");
    document.documentElement.classList.toggle("light", theme === "light");
  }, [theme]);
  return (
    <TooltipProvider>
      <div className="h-full bg-background" data-gallery-scene="agent-onboarding" />
      <AgentOnboardingGate actions={actions} />
    </TooltipProvider>
  );
}
