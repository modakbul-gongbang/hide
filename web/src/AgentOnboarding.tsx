import { CheckIcon } from "lucide-react";
import { useState } from "react";
import type { Actions } from "./actions";
import { agentLogo, monogram } from "./agentLogos";
import { appliedAgents, selection, tileSwitchable } from "./agentOnboardingRules";
import { Button } from "./components/ui/button";
import { Dialog, DialogBody, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "./components/ui/dialog";
import { useInterfaceTranslation } from "./i18n/client";
import type { KitAgent } from "./snapshot";
import { useShellStore } from "./store";

/**
 * The first-run agent choice: shown while the core says it is pending and
 * this Mac's kit has listed the agents. It has one way out, Apply: Escape, a
 * click outside and the window's other routes do nothing, so a stray key
 * cannot finish a choice that leaves Claude Code and Codex off.
 */
export function AgentOnboardingGate({ actions }: { actions: Actions }) {
  const pending = useShellStore((s) => s.rest?.ui_state?.agent_onboarding === "pending");
  const agents = useShellStore((s) => s.rest?.navigator?.devices?.find((device) => device.id === "local")?.kit?.agents);
  return pending && agents && agents.length > 0 ? <AgentOnboarding actions={actions} agents={agents} /> : null;
}

function AgentOnboarding({ actions, agents }: { actions: Actions; agents: KitAgent[] }) {
  const { t } = useInterfaceTranslation();
  // Only the operator's flips are state; what is on is read from the live availability each render.
  const [flipped, setFlipped] = useState<ReadonlySet<string>>(new Set());
  const on = selection(agents, flipped);
  const toggle = (id: string) =>
    setFlipped((current) => {
      const next = new Set(current);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  return (
    // Nothing closes it from here: Apply is the only way out, and the core ends the question.
    <Dialog open onOpenChange={() => {}}>
      <DialogContent
        data-agent-onboarding="true"
        className="w-(--size-onboarding-dialog-w)"
        onEscapeKeyDown={(event) => event.preventDefault()}
        onPointerDownOutside={(event) => event.preventDefault()}
        onInteractOutside={(event) => event.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle className="text-headline">{t("onboarding.title")}</DialogTitle>
          <DialogDescription>{t("onboarding.description")}</DialogDescription>
        </DialogHeader>
        <DialogBody>
          <div role="group" aria-label={t("onboarding.grid")} className="grid grid-cols-[repeat(auto-fill,minmax(var(--size-onboarding-tile),1fr))] gap-sm">
            {agents.map((agent) => (
              <AgentTile key={agent.id} agent={agent} on={on.has(agent.id)} onToggle={() => toggle(agent.id)} />
            ))}
          </div>
          <p className="mt-md text-caption text-muted-foreground">{t("onboarding.devices")}</p>
        </DialogBody>
        <DialogFooter>
          <Button onClick={() => actions.applyAgentOnboarding(appliedAgents(agents, flipped))} data-onboarding-apply="true">
            {t("onboarding.apply")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/**
 * One square tile: the agent's logo (or a monogram when no official mark may
 * be bundled) and its name. On or off is a mark and a word as well as a
 * colour; a tile for an agent that is not installed here is dimmed and has no
 * switch.
 */
function AgentTile({ agent, on, onToggle }: { agent: KitAgent; on: boolean; onToggle: () => void }) {
  const { t } = useInterfaceTranslation();
  const switchable = tileSwitchable(agent);
  const logo = agentLogo(agent.id);
  // The mark and the name dim; the caption that says why stays at full strength to remain legible.
  const dim = switchable ? "" : "opacity-(--opacity-dimmed)";
  const body = (
    <>
      <span className={`flex size-(--size-agent-logo) items-center justify-center overflow-hidden rounded-md bg-(--logo-plate) ${dim}`} aria-hidden="true">
        {logo ? (
          <img src={logo} alt="" className="size-full object-contain p-xxs" data-agent-logo={agent.id} />
        ) : (
          <span className="font-mono text-body font-semibold text-muted-foreground" data-agent-monogram={agent.id}>
            {monogram(agent.label)}
          </span>
        )}
      </span>
      <span className={`max-w-full truncate text-body font-semibold ${dim}`}>{agent.label}</span>
      <span className="text-caption text-muted-foreground">
        {switchable ? (on ? t("onboarding.tileOn") : t("onboarding.tileOff")) : t("onboarding.notInstalled")}
      </span>
    </>
  );
  const base = "relative flex aspect-square flex-col items-center justify-center gap-xs rounded-md border p-sm text-center";
  if (!switchable) {
    return (
      <div className={`${base} border-border`} data-onboarding-tile={`${agent.id}:unavailable`}>
        {body}
      </div>
    );
  }
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={agent.label}
      onClick={onToggle}
      className={`${base} cursor-pointer outline-none focus-visible:ring-2 focus-visible:ring-ring ${on ? "border-primary bg-card" : "border-border bg-transparent"}`}
      data-onboarding-tile={`${agent.id}:${on ? "on" : "off"}`}
    >
      <span
        className={`absolute right-xs top-xs flex size-md items-center justify-center rounded-full border ${on ? "border-primary bg-primary text-primary-foreground" : "border-border"}`}
        aria-hidden="true"
      >
        {on ? <CheckIcon className="size-(--size-icon-sm)" /> : null}
      </span>
      {body}
    </button>
  );
}
