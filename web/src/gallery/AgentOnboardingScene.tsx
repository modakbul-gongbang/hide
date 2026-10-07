// The first-run agent choice on invented data: the production gate over a
// synthetic local device that lists the seven adapters, no daemon. The installed
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
  ["grok", "Grok", "not_installed"],
  ["opencode", "OpenCode", "available"],
  ["pi", "Pi", "not_installed"],
  ["omp", "omp", "available"],
  ["cursor", "Cursor", "available"],
];

export function AgentOnboardingScene({ theme }: { theme: "light" | "dark" }) {
  const actions = useMemo(() => createActions(() => true), []);
  useLayoutEffect(() => {
    const agents: KitAgent[] = AGENTS.map(([id, label, availability]) => ({
      id,
      label,
      availability,
      enabled: false,
      chosen: false,
      skill: { state: "off", reason: null, location: null },
      hook: null,
      doc_url: "https://example.test",
    }));
    useShellStore.setState({
      connection: "live",
      rest: {
        ui_state: { agent_onboarding: "pending" },
        navigator: { devices: [{ id: "local", kit: { agents } }] },
        // Claude Code is signed in, so the first-run line names it.
        status: { background_ai: { provider: null, chosen: false, providers: [{ id: "claude", label: "Claude Code", agent: "claude-code", state: "ready", selectable: true }] } },
      },
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
