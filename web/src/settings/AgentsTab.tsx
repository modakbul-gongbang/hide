import { RefreshCwIcon } from "lucide-react";
import { useEffect, useState } from "react";
import type { Actions } from "../actions";
import { Disclosure, Group, Note, Row } from "../components/settings-rows";
import { Button } from "../components/ui/button";
import { ToggleGroup, ToggleGroupItem } from "../components/ui/toggle-group";
import { useInterfaceTranslation } from "../i18n/client";
import { useShellStore } from "../store";
import { agentMachines, type AgentMachine } from "./agentRows";
import { AgentRow, NotInstalledRow } from "./AgentRow";
import { IdleAgentsGroup } from "./IdleAgentsGroup";
import { StartingWorkGroup } from "./StartingWorkGroup";
import { useAgentsDemand } from "./useAgentsDemand";
import { useErrorSince } from "./useErrorSince";

/**
 * How long Check again reads as under way. The core publishes only what
 * changed, so a read that finds nothing new sends no frame to end it: the
 * button rests again after this bound, and a failed read shows its reason.
 */
const CHECKING_MS = 1500;

/**
 * Agents (PRD settings-cleanup D-06 to D-13): the seven supported agents of one
 * machine at a time, the machine chosen at the top when there is more than
 * one. Installed agents carry a switch and what Hide hears from their
 * sessions; the rest wait in a folded list with the link to install them.
 * Idle agents and Starting work keep their place below.
 */
export function AgentsTab({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const devices = useShellStore((s) => s.rest?.navigator?.devices);
  const [selected, setSelected] = useState("local");
  useAgentsDemand(actions, true);
  const machines = agentMachines(devices ?? []);
  const machine = machines.find((row) => row.device.id === selected) ?? machines[0];
  return (
    <>
      {machines.length > 1 ? (
        <ToggleGroup
          type="single"
          value={machine?.device.id}
          // Choosing the chosen machine again would leave the group with no value.
          onValueChange={(value) => value && setSelected(value)}
          aria-label={t("agents.machine")}
          className="mb-lg"
          data-agents-machines="true"
        >
          {machines.map((row) => (
            <ToggleGroupItem key={row.device.id} value={row.device.id} data-agents-machine={row.device.id}>
              {row.device.id === "local" ? t("common.thisMac") : row.device.label}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      ) : null}
      {machine ? <MachineAgents key={machine.device.id} machine={machine} actions={actions} /> : <Note>{t("agents.reading")}</Note>}
      <IdleAgentsGroup actions={actions} />
      <StartingWorkGroup actions={actions} />
    </>
  );
}

function MachineAgents({ machine, actions }: { machine: AgentMachine; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const { device } = machine;
  const [pressed, setPressed] = useState<{ at: number; check: boolean } | null>(null);
  const [checking, setChecking] = useState(false);
  const failure = useErrorSince(pressed?.at ?? null, ["kit."]);
  useEffect(() => {
    if (!checking) return;
    const timer = window.setTimeout(() => setChecking(false), CHECKING_MS);
    return () => window.clearTimeout(timer);
  }, [checking]);
  const act = () => setPressed({ at: Date.now(), check: false });

  // B11: a device that cannot be reached shows one line and a way to try again, not its last list.
  if (machine.blocked === "unreachable") {
    return (
      <Group>
        <Row label={<Note data-agents-unreachable={device.id}>{t("agents.unreachable", { device: device.label })}</Note>}>
          <Button variant="secondary" size="sm" onClick={() => actions.retryDevice(device.id)} data-agents-retry={device.id}>
            {t("agents.retry")}
          </Button>
        </Row>
      </Group>
    );
  }
  const reading = device.kit?.busy === true || checking;
  return (
    <div data-agents-machine-list={device.id}>
      {machine.blocked === "unavailable" && device.kit?.unavailable ? <Note data-agents-unavailable={device.id}>{device.kit.unavailable}</Note> : null}
      {machine.unread && machine.blocked === null ? <Note data-agents-unread={device.id}>{t("agents.reading")}</Note> : null}
      {machine.installed.length + machine.notInstalled.length > 0 ? (
        <>
          <Group
            title={t("agents.installedCount", { count: machine.installed.length })}
            action={
              <Button
                variant="ghost"
                size="sm"
                disabled={reading}
                aria-label={t("agents.checkAgainAria")}
                onClick={() => {
                  setPressed({ at: Date.now(), check: true });
                  setChecking(true);
                  actions.checkKit();
                }}
                data-agents-check={reading ? "checking" : "idle"}
              >
                <RefreshCwIcon aria-hidden="true" className={reading ? "animate-spin" : ""} />
                {reading ? t("agents.checking") : t("agents.checkAgain")}
              </Button>
            }
          >
            {machine.installed.length === 0 ? <Row label={<Note>{t("agents.noneInstalled")}</Note>} /> : null}
            {machine.installed.map((agent) => (
              <AgentRow key={agent.id} device={device} agent={agent} actions={actions} onAct={act} />
            ))}
          </Group>
          {failure ? (
            <Note tone="error" data-agents-error="true">
              {pressed?.check ? t("agents.checkFailed", { reason: failure }) : failure}
            </Note>
          ) : null}
          {machine.notInstalled.length > 0 ? (
            <Group>
              <Disclosure title={t("agents.notInstalledCount", { count: machine.notInstalled.length })} data-agents-not-installed={device.id}>
                {machine.notInstalled.map((agent) => (
                  <NotInstalledRow key={agent.id} agent={agent} />
                ))}
              </Disclosure>
            </Group>
          ) : null}
        </>
      ) : null}
    </div>
  );
}
