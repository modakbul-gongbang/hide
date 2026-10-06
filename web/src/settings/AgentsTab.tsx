import { useState } from "react";
import type { Actions } from "../actions";
import { Button } from "../components/ui/button";
import { Switch } from "../components/ui/switch";
import { Group, Note, Row, Status } from "../components/settings-rows";
import { useInterfaceTranslation } from "../i18n/client";
import {
  kitAgentGets,
  kitAgentLine,
  kitAgentMachines,
  kitAgentNeedsReinstall,
  kitAgentSwitch,
  providerLine,
} from "../settings";
import { useShellStore } from "../store";
import { IdleAgentsGroup } from "./IdleAgentsGroup";
import { StartingWorkGroup } from "./StartingWorkGroup";
import { useAgentsDemand } from "./useAgentsDemand";
import { useErrorSince } from "./useErrorSince";

export function AgentsTab({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const ai = useShellStore((s) => s.rest?.status?.background_ai);
  const hooks = useShellStore((s) => s.rest?.status?.agent_hooks);
  const devices = useShellStore((s) => s.rest?.navigator?.devices);
  const [pressedAt, setPressedAt] = useState<number | null>(null);
  const kitError = useErrorSince(pressedAt, ["kit."]);

  useAgentsDemand(actions, true);

  const machines = kitAgentMachines(devices ?? []);

  return (
    <>
      <Group title={t("settings.agentClis")} note={t("settings.agentClisDescription")}>
        {(ai?.providers ?? []).length === 0 ? <Row label={<Note>{t("settings.notRead")}</Note>} /> : null}
        {(ai?.providers ?? []).map((provider) => {
          const line = providerLine(provider);
          return (
            <Row key={provider.id} label={<span className="font-semibold">{provider.label}</span>}>
              <Status tone={line.tone} data-cli-state={`${provider.id}:${provider.state}`}>
                {line.text}
              </Status>
            </Row>
          );
        })}
      </Group>
      <Group
        title={t("settings.agentHooks")}
        note={t("settings.agentHooksDescription")}
        data-agent-hooks="true"
      >
        {machines.map(({ device, listed, others, unavailable }) => (
          <div key={device.id} data-hook-machine={device.id}>
            <Row
              label={<span className="font-semibold">{device.id === "local" ? t("common.thisMac") : device.label}</span>}
              detail={unavailable ? <Note data-hook-unavailable={device.id}>{unavailable}</Note> : device.kit?.components.length === 0 ? <Note>{t("settings.notChecked")}</Note> : null}
            />
            {listed.map((agent) => {
              const line = kitAgentLine(agent, t);
              const switched = kitAgentSwitch(agent);
              return (
                <Row
                  key={agent.id}
                  label={
                    <span className="flex min-w-0 flex-col pl-md">
                      <span>{agent.label}</span>
                      <span className="text-caption text-muted-foreground">{kitAgentGets(agent, t)}</span>
                    </span>
                  }
                  detail={line.reason ? <Note tone={line.tone === "error" ? "error" : "muted"}>{line.reason}</Note> : null}
                >
                  <Status tone={line.tone} data-agent-state={`${device.id}:${agent.id}:${agent.enabled ? "on" : "off"}:${line.tone}`}>
                    {line.text}
                  </Status>
                  {kitAgentNeedsReinstall(agent) ? (
                    <Button
                      variant="secondary"
                      disabled={device.kit?.busy === true}
                      onClick={() => {
                        setPressedAt(Date.now());
                        actions.reinstallKit(device.id, [], [agent.id]);
                      }}
                      data-hook-reinstall={`${device.id}:${agent.id}`}
                    >
                      {device.kit?.busy ? t("settings.reinstalling") : t("settings.reinstall")}
                    </Button>
                  ) : null}
                  {switched ? (
                    <Switch
                      checked={switched.on}
                      disabled={device.kit?.busy === true}
                      onCheckedChange={(checked) => {
                        setPressedAt(Date.now());
                        actions.setKitAgent(device.id, agent.id, checked);
                      }}
                      aria-label={t(switched.on ? "devices.kitSwitchOff" : "devices.kitSwitchOn", { part: agent.label })}
                      data-agent-switch={`${device.id}:${agent.id}:${switched.on ? "on" : "off"}`}
                    />
                  ) : null}
                </Row>
              );
            })}
            {others.length > 0 ? (
              <Row label={<Note data-agents-not-installed={device.id}>{t("settings.agentsNotInstalled", { agents: others.join(", ") })}</Note>} />
            ) : null}
          </div>
        ))}
        {kitError ? <Row label={<Note tone="error" data-hook-error="true">{kitError}</Note>} /> : null}
        {hooks?.last_report_failure ? <Row label={<Note tone="error">{hooks.last_report_failure}</Note>} /> : null}
        {(hooks?.sessions_predating_install ?? []).map((pane) => (
          <Row key={pane.pane_id} label={<Note tone="warn">{`${pane.label} (${pane.pane_id}): ${pane.message}`}</Note>} />
        ))}
      </Group>
      <IdleAgentsGroup actions={actions} />
      <StartingWorkGroup actions={actions} />
    </>
  );
}
