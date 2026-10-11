// The first-run wizard (PRD first-run-onboarding D-01): 1 agents, 2
// notifications, 3 the first project, then the start panel. The step on
// screen is the core's (`ui_state.first_run`), so a reload or a restart
// reopens the step the operator was on, and nothing closes it but the steps'
// own buttons (B5, B6). A browser tab shows the first step alone (B26).
// Help > Set up Hide reopens the same steps from the first, as this page's
// own walk that closes with Escape and changes only what a step's button
// sends (B45, B46).
//
// The visual layout follows the approved Pen design (D-20); until it lands
// the steps keep to the shell's existing parts.

import { CheckIcon } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { create } from "zustand";
import type { Actions } from "./actions";
import { AddProjectViews } from "./AddProjectDialog";
import { agentLogo, monogram } from "./agentLogos";
import { appliedAgents, hideAiFirstRunKey, selection, tileSwitchable, type TileStart } from "./agentOnboardingRules";
import { Button } from "./components/ui/button";
import { Dialog, DialogBody, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "./components/ui/dialog";
import { failedAgents, firstRunStep, firstStartAgent, noAgentInstalled, notificationState, SUGGESTED_INSTALLS, tileStatus, whatChanges, type TileStatus, type WizardStep } from "./firstRun";
import { firstRunAgent } from "./hideAi";
import { hostBridge } from "./host";
import { useInterfaceTranslation } from "./i18n/client";
import type { MessageKey } from "./i18n/catalogs";
import { NotificationControls, usePermission } from "./NotificationControls";
import { installDocUrl, supportedAgents } from "./settings/agentRows";
import { useAgentsDemand } from "./settings/useAgentsDemand";
import type { KitAgent, WorkspaceRegistration } from "./snapshot";
import { useStartPanel } from "./startDraft";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";

/** Help > Set up Hide's walk: this page's own, gone with a close (B46). */
type SetupWalk = { step: WizardStep | null; open: () => void; go: (step: WizardStep) => void; close: () => void };

export const useSetupWalk = create<SetupWalk>((set) => ({
  step: null,
  open: () => set({ step: "agents" }),
  go: (step) => set({ step }),
  close: () => set({ step: null }),
}));

const STEPS: readonly WizardStep[] = ["agents", "notifications", "project"];
const NO_REGISTRATIONS: WorkspaceRegistration[] = [];

export function FirstRunWizard({ actions }: { actions: Actions }) {
  const desktop = hostBridge() !== null;
  const firstRun = useShellStore((s) => firstRunStep(s.rest, desktop));
  const setup = useSetupWalk((s) => s.step);
  const kit = useShellStore((s) => s.rest?.navigator?.devices?.find((device) => device.kind !== "remote")?.kit);
  // The first run owns the screen; Help's walk waits behind it.
  const step = firstRun ?? setup;
  if (step === null) return null;
  // The agent step draws this Mac's kit, which lists every supported agent once it has been read;
  // a kit that could not run stays on screen to say why.
  if (step === "agents" && !kit?.agents?.length && !kit?.unavailable) return null;
  return <Wizard actions={actions} step={step} reopened={firstRun === null} desktop={desktop} />;
}

function Wizard({ actions, step, reopened, desktop }: { actions: Actions; step: WizardStep; reopened: boolean; desktop: boolean }) {
  const { t } = useInterfaceTranslation();
  const primary = useRef<(() => void) | null>(null);
  // Each render's step names its own main button; a step without one (the project step) leaves Enter to its controls.
  primary.current = null;
  const close = () => useSetupWalk.getState().close();
  const number = STEPS.indexOf(step) + 1;
  // Enter on the step itself presses its main button; a control with its own Enter keeps it (B40).
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key !== "Enter" || event.nativeEvent.isComposing) return;
    const target = event.target as HTMLElement;
    if (target.closest("button, a, input, textarea, select, [role=switch], [role=combobox]")) return;
    event.preventDefault();
    primary.current?.();
  };
  return (
    <Dialog open onOpenChange={(open) => { if (!open && reopened) close(); }}>
      <DialogContent
        data-first-run={step}
        data-first-run-mode={reopened ? "setup" : "first_run"}
        showCloseButton={reopened}
        aria-describedby={undefined}
        className="w-(--size-onboarding-dialog-w)"
        onKeyDown={onKeyDown}
        // The first run closes only through its steps (B5).
        onEscapeKeyDown={(event) => { if (!reopened) event.preventDefault(); }}
        onPointerDownOutside={(event) => event.preventDefault()}
        onInteractOutside={(event) => event.preventDefault()}
      >
        {desktop ? (
          <div className="flex items-center gap-sm px-lg pt-lg" data-first-run-progress={number}>
            <div className="flex flex-1 gap-xxs" aria-hidden="true">
              {STEPS.map((row, index) => (
                <span key={row} className={`h-xxs flex-1 rounded-full ${index < number ? "bg-primary" : "bg-secondary"}`} />
              ))}
            </div>
            <span className="text-caption text-muted-foreground">{t("firstRun.progress", { step: number })}</span>
          </div>
        ) : null}
        {step === "agents" ? (
          <AgentsStep actions={actions} reopened={reopened} desktop={desktop} primary={primary} />
        ) : step === "notifications" ? (
          <NotificationsStep actions={actions} reopened={reopened} primary={primary} />
        ) : (
          <ProjectStep actions={actions} reopened={reopened} />
        )}
      </DialogContent>
    </Dialog>
  );
}

type StepProps = { actions: Actions; reopened: boolean; primary: { current: (() => void) | null } };

function AgentsStep({ actions, reopened, desktop, primary }: StepProps & { desktop: boolean }) {
  const { t } = useInterfaceTranslation();
  const kit = useShellStore((s) => s.rest?.navigator?.devices?.find((device) => device.kind !== "remote")?.kit ?? null);
  const agents = useMemo(() => supportedAgents(kit?.agents), [kit?.agents]);
  const providers = useShellStore((s) => s.rest?.status?.background_ai?.providers);
  const ai = useShellStore((s) => s.rest?.status?.background_ai);
  const start: TileStart = reopened ? "enabled" : "installed";
  // Only the operator's flips are state; what is on is read from the live kit each render.
  const [flipped, setFlipped] = useState<ReadonlySet<string>>(new Set());
  const [showChanges, setShowChanges] = useState(false);
  // The agents the last Continue sent: a failure names those (B12).
  const [sent, setSent] = useState<string[] | null>(null);
  // The kit is busy from the snapshot that takes Continue until its pass lands
  // (B10); the press itself holds the button only until that next snapshot.
  // A second press meanwhile is the same intent, which the core ignores.
  const busy = kit?.busy ?? false;
  const revision = useShellStore((s) => s.revision);
  const [pressedAt, setPressedAt] = useState<number | null>(null);
  const pressed = pressedAt !== null && pressedAt === revision;
  const on = selection(agents, flipped, start);
  // The sign-in probe runs only while a page asks for it.
  useAgentsDemand(actions, false);
  const hideAi = firstRunAgent(ai, on);
  const changes = whatChanges(agents, on, kit);
  const applying = pressed || busy;
  const failures = sent && !applying ? failedAgents(agents, sent) : [];
  // A Continue whose pass could not run here: the core keeps the step and reads the agents again.
  const unavailable = sent && !applying ? (kit?.unavailable ?? null) : null;
  const toggle = (id: string) =>
    setFlipped((current) => {
      const next = new Set(current);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  const proceed = () => {
    if (applying || agents.length === 0) return;
    const chosen = appliedAgents(agents, flipped, start);
    setSent(chosen);
    if (reopened) {
      // Help's walk switches only what changed, the way Settings > Agents does, and moves on at once.
      actions.applyAgentOnboarding(chosen);
      useSetupWalk.getState().go("notifications");
      return;
    }
    setPressedAt(revision);
    // A browser tab's wizard is this step alone, so it ends with it (B26).
    actions.applyAgentOnboarding(chosen, !desktop);
  };
  primary.current = proceed;
  return (
    <>
      <DialogHeader>
        <DialogTitle className="text-headline">{t("firstRun.agents.title")}</DialogTitle>
        <DialogDescription>{t("firstRun.agents.consent")}</DialogDescription>
      </DialogHeader>
      <DialogBody data-agent-onboarding="true">
        <div role="group" aria-label={t("onboarding.grid")} className="grid grid-cols-[repeat(auto-fill,minmax(var(--size-onboarding-tile),1fr))] gap-sm">
          {agents.map((agent) => (
            <AgentTile key={agent.id} agent={agent} on={on.has(agent.id)} status={tileStatus(agent, providers)} disabled={applying} onToggle={() => toggle(agent.id)} />
          ))}
        </div>
        {noAgentInstalled(agents) ? (
          <p className="mt-md text-body text-muted-foreground" data-first-run-no-agent="true">
            {t("firstRun.agents.noneInstalled")}{" "}
            {SUGGESTED_INSTALLS.map((id) => (
              <a key={id} href={installDocUrl(id) ?? undefined} target="_blank" rel="noreferrer" className="mr-sm underline" data-first-run-install={id}>
                {agents.find((agent) => agent.id === id)?.label ?? id} ↗
              </a>
            ))}
          </p>
        ) : null}
        <div className="mt-md">
          <button type="button" aria-expanded={showChanges} onClick={() => setShowChanges((open) => !open)} className="text-body text-muted-foreground underline" data-first-run-what-changes="true">
            {t("firstRun.agents.whatChanges")} {showChanges ? "▾" : "▸"}
          </button>
          {showChanges ? (
            <ul className="mt-xs flex flex-col gap-xs font-mono text-caption text-muted-foreground" data-first-run-changes="true">
              {changes.agents.map((row) => (
                <li key={row.id} data-first-run-change={row.id}>
                  <span className="font-sans font-medium text-foreground">{row.label}</span>
                  {row.locations.map((location) => (
                    <div key={location}>{location}</div>
                  ))}
                </li>
              ))}
              {changes.agents.length === 0 ? <li className="font-sans">{t("firstRun.agents.nothingChanges")}</li> : null}
              {changes.cli ? <li data-first-run-change="cli">{changes.cli}</li> : null}
            </ul>
          ) : null}
        </div>
        {unavailable ? (
          <p role="alert" className="mt-sm text-body text-destructive" data-first-run-unavailable="true">
            {t("firstRun.agents.unavailable", { reason: unavailable })}
          </p>
        ) : null}
        {failures.map((failure) => (
          <p key={failure.id} role="alert" className="mt-sm text-body text-destructive" data-first-run-failed={failure.id}>
            {failure.reason ? t("firstRun.agents.failed", { agent: failure.label, reason: failure.reason }) : t("firstRun.agents.failedNoReason", { agent: failure.label })}
          </p>
        ))}
        <p className="mt-md text-caption text-muted-foreground">{t("onboarding.devices")}</p>
      </DialogBody>
      <DialogFooter className="flex-col items-end">
        {/* With no agent read yet there is no choice to send; the next read brings the tiles back. */}
        <Button onClick={proceed} disabled={applying || agents.length === 0} aria-busy={applying} data-onboarding-apply="true">
          {applying ? t("firstRun.agents.applying") : failures.length > 0 || unavailable ? t("firstRun.agents.retry") : t("firstRun.agents.continue")}
        </Button>
        {/* The agent Hide AI will run on under its first-run rule; no line when none of the chosen agents is signed in (B13). */}
        {hideAi ? (
          <p className="text-caption text-muted-foreground" data-onboarding-hide-ai={hideAi.id}>
            {t(hideAiFirstRunKey(hideAi.agent), { agent: hideAi.label })}
          </p>
        ) : null}
      </DialogFooter>
    </>
  );
}

const TILE_STATUS: Record<TileStatus, MessageKey> = {
  checking: "firstRun.tile.checking",
  ready: "firstRun.tile.ready",
  not_signed_in: "firstRun.tile.notSignedIn",
  installed: "firstRun.tile.installed",
  not_installed: "firstRun.tile.notInstalled",
};

/**
 * One agent: its mark, its name and what Hide knows of it (B8). An agent
 * installed here is a switch; one that is not is dimmed and links its
 * official install page instead (B9).
 */
function AgentTile({ agent, on, status, disabled, onToggle }: { agent: KitAgent; on: boolean; status: TileStatus; disabled: boolean; onToggle: () => void }) {
  const { t } = useInterfaceTranslation();
  const switchable = tileSwitchable(agent);
  const logo = agentLogo(agent.id);
  const install = switchable ? null : installDocUrl(agent.id);
  // The mark and the name dim; the line that says why stays legible.
  const dim = switchable ? "" : "opacity-(--opacity-dimmed)";
  const body: ReactNode = (
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
      <span className="text-caption text-muted-foreground" data-onboarding-status={status}>
        {t(TILE_STATUS[status])}
      </span>
    </>
  );
  const base = "relative flex aspect-square flex-col items-center justify-center gap-xs rounded-md border p-sm text-center";
  if (!switchable) {
    return (
      <div className={`${base} border-border`} data-onboarding-tile={`${agent.id}:unavailable`}>
        {body}
        {install ? (
          <a href={install} target="_blank" rel="noreferrer" className="text-caption underline" data-onboarding-install={agent.id}>
            {t("firstRun.tile.install")}
          </a>
        ) : null}
      </div>
    );
  }
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={agent.label}
      disabled={disabled}
      onClick={onToggle}
      className={`${base} cursor-pointer outline-none focus-visible:ring-2 focus-visible:ring-ring ${on ? "border-primary bg-card" : "border-border bg-transparent"}`}
      data-onboarding-tile={`${agent.id}:${on ? "on" : "off"}`}
    >
      <span className={`absolute right-xs top-xs flex size-md items-center justify-center rounded-full border ${on ? "border-primary bg-primary text-primary-foreground" : "border-border"}`} aria-hidden="true">
        {on ? <CheckIcon className="size-(--size-icon-sm)" /> : null}
      </span>
      {body}
    </button>
  );
}

function NotificationsStep({ actions, reopened, primary }: StepProps) {
  const { t } = useInterfaceTranslation();
  const enabled = useShellStore((s) => s.rest?.ui_state?.desktop_notifications === true);
  const permission = usePermission();
  // On a Mac that already allows Hide the step opens on, and Continue is what turns the switch on (B16).
  const allowedOff = !enabled && notificationState(enabled, permission, "wizard") === "on";
  const next = () => {
    if (reopened) {
      if (allowedOff) actions.setDesktopNotifications(true);
      useSetupWalk.getState().go("project");
    } else actions.advanceFirstRun("notifications", allowedOff);
  };
  primary.current = next;
  return (
    <>
      <DialogHeader>
        <DialogTitle className="text-headline">{t("firstRun.notifications.title")}</DialogTitle>
        <DialogDescription>{t("firstRun.notifications.description")}</DialogDescription>
      </DialogHeader>
      <DialogBody className="flex flex-col gap-sm">
        <NotificationControls actions={actions} place="wizard" permission={permission} />
        <button type="button" onClick={() => useUiStore.getState().openSettings("mobile")} className="self-start text-body text-muted-foreground underline" data-first-run-phone="true">
          {t("firstRun.notifications.phone")}
        </button>
      </DialogBody>
      <DialogFooter>
        {/* Not now leaves the switch off; once it is on, the step goes on with Continue (B21). */}
        <Button variant={enabled || allowedOff ? "default" : "secondary"} onClick={next} data-first-run-next="notifications">
          {enabled || allowedOff ? t("firstRun.agents.continue") : t("firstRun.notifications.notNow")}
        </Button>
      </DialogFooter>
    </>
  );
}

function ProjectStep({ actions, reopened }: { actions: Actions; reopened: boolean }) {
  const { t } = useInterfaceTranslation();
  const registrations = useShellStore((s) => s.rest?.ui_state?.workspace_registrations ?? NO_REGISTRATIONS);
  // The projects that were there when the step opened; the first one after them is the operator's (B23).
  const [known] = useState(() => new Set(registrations.map((row) => row.id)));
  // Only on the host the step adds to: a device's own registrations arriving meanwhile are not the operator's pick.
  const [host, setHost] = useState<string | null>(null);
  const added = registrations.find((row) => !row.home && !known.has(row.id) && row.device_id === host) ?? null;
  const finish = () => {
    if (reopened) useSetupWalk.getState().close();
    else actions.advanceFirstRun("project");
  };
  useEffect(() => {
    if (!added) return;
    finish();
    // The start panel opens once, on this project, with the first agent D-15 picks (B23, B24).
    const rest = useShellStore.getState().rest;
    const agents = rest?.navigator?.devices?.find((device) => device.kind !== "remote")?.kit?.agents ?? [];
    useStartPanel.getState().open(false, { projectId: added.id, kind: firstStartAgent(supportedAgents(agents), rest?.status?.background_ai?.providers) });
    // `finish` reads only the store; the project appearing is the one trigger.
  }, [added?.id]);
  return (
    <>
      <AddProjectViews actions={actions} title={(translate) => translate("firstRun.project.title")} onHost={setHost} />
      <DialogFooter>
        <Button variant="secondary" onClick={finish} data-first-run-skip="true">
          {t("firstRun.project.skip")}
        </Button>
      </DialogFooter>
    </>
  );
}
