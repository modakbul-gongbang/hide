// Synthetic visual states only; native/daemon checks own persistence and reads.
import { useLayoutEffect, useMemo } from "react";
import { createActions } from "../actions";
import { AGENT_ADAPTERS } from "../agentAdapters";
import { TooltipProvider } from "../components/ui/tooltip";
import type { AgentUsage, UsagePreferences } from "../generated/hided-ws";
import { clientI18n } from "../i18n/translator";
import { SettingsGate } from "../SettingsSheet";
import { useShellStore } from "../store";
import { useUiStore } from "../ui";
import { hasLimits } from "../usageLimits";
import { REFERENCE_FOLDS, sidebarScene } from "./sceneData";

export function UsageScene({ params }: { params: URLSearchParams }) {
  const state = params.get("state") ?? "default";
  const actions = useMemo(() => createActions((event) => {
    useShellStore.setState((store) => {
      const rest = store.rest;
      const usage = rest?.ui_state?.usage;
      if (!rest || !usage) return store;
      const payload = event.payload;
      if (event.kind === "usage_hidden_set") {
        const hidden = usage.hidden.filter((id) => id !== payload.agent);
        if (payload.hidden) hidden.push(payload.agent as string);
        return { rest: { ...rest, ui_state: { ...rest.ui_state, usage: { ...usage, hidden } } } };
      }
      if (event.kind === "usage_pin_set") {
        const pinned = usage.pinned.filter((pin) => pin.agent !== payload.agent || pin.window !== payload.window);
        if (payload.enabled && pinned.length < 3) pinned.push({ agent: payload.agent, window: payload.window } as UsagePreferences["pinned"][number]);
        return { rest: { ...rest, ui_state: { ...rest.ui_state, usage: { ...usage, pinned } } } };
      }
      return store;
    });
  }), []);
  useLayoutEffect(() => {
    const now = Date.now();
    const reset = Math.floor(now / 1000);
    const agents: AgentUsage[] = AGENT_ADAPTERS.map((adapter) => ({
      agent: adapter.herdr_kind,
      consumption: { state: "unsupported" },
      limits: !hasLimits(adapter) ? { state: "unsupported" } : state === "loading" ? { state: "loading" } : state === "unavailable" ? { state: "unavailable", cause: "signed_out", last_checked_at_ms: now } : {
        state: "reading", freshness: "fresh", observed_at_ms: now,
        value: { windows: [
          { kind: "five_hour", scope: { kind: "all" }, outcome: adapter.id === "codex" && state !== "both-plans" && state !== "full" ? { state: "not_offered" } : state === "not-started" ? { state: "not_started" } : state === "expired" ? { state: "expired", reset_at_s: reset - 1 } : { state: "open", used_percent: 11, resets_at_s: reset + 11520 } },
          { kind: "weekly", scope: { kind: "all" }, outcome: { state: "open", used_percent: adapter.id === "codex" ? 76 : 81, resets_at_s: reset + 450000 } },
        ] },
      },
    }));
    const scene = sidebarScene("reference", REFERENCE_FOLDS, now, "one");
    const usage: UsagePreferences = { pinned: [{ agent: "claude", window: "weekly" }, { agent: "codex", window: "weekly" }], hidden: state === "hidden" ? ["claude"] : [] };
    if (state === "full") usage.pinned.push({ agent: "claude", window: "five_hour" });
    useShellStore.setState({ connection: "live", rest: { ...scene.rest, navigator: { ...scene.rest.navigator, usage: { nodes: [{ node: "local", agents }] } }, ui_state: { ...scene.rest.ui_state, usage } }, agents: [] });
    useUiStore.getState().openSettings("usage");
    const root = document.documentElement;
    root.classList.toggle("dark", params.get("theme") !== "light");
    root.classList.toggle("light", params.get("theme") === "light");
    root.style.setProperty("--interface-scale", params.get("scale") ?? "1");
    const language = params.get("language") ?? "en";
    root.lang = language;
    void clientI18n.changeLanguage(language);
  }, [params, state]);
  return <TooltipProvider><div className="h-screen bg-background" data-gallery-scene="usage"><SettingsGate actions={actions} /></div></TooltipProvider>;
}
