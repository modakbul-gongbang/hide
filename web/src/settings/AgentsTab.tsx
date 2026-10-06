import { useEffect, useState } from "react";
import type { Actions } from "../actions";
import { Button } from "../components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../components/ui/select";
import { Switch } from "../components/ui/switch";
import { Group, Note, Row, Status } from "../components/settings-rows";
import { useInterfaceTranslation } from "../i18n/client";
import {
  kitAgentGets,
  kitAgentLine,
  kitAgentMachines,
  kitAgentNeedsReinstall,
  kitAgentSwitch,
  offeredModels,
  providerLine,
} from "../settings";
import { useShellStore } from "../store";
import { useErrorSince } from "./useErrorSince";

export function AgentsTab({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const ai = useShellStore((s) => s.rest?.status?.background_ai);
  const hooks = useShellStore((s) => s.rest?.status?.agent_hooks);
  const devices = useShellStore((s) => s.rest?.navigator?.devices);
  const [changedAt, setChangedAt] = useState<number | null>(null);
  const aiError = useErrorSince(changedAt, ["ai_settings."]);
  const [pressedAt, setPressedAt] = useState<number | null>(null);
  const kitError = useErrorSince(pressedAt, ["kit."]);

  // The provider probe and the hook diagnosis run only while a page shows
  // this tab (B8). A hidden browser tab is not looking either; the daemon
  // releases this page's demand if the socket drops.
  // A reconnect is a new connection whose demand starts empty, so the
  // demand is declared again each time the page is live.
  const live = useShellStore((s) => s.connection === "live");
  useEffect(() => {
    if (!live) return;
    const report = () => actions.observeAgents(document.visibilityState === "visible");
    report();
    actions.checkKit();
    document.addEventListener("visibilitychange", report);
    return () => {
      document.removeEventListener("visibilitychange", report);
      actions.observeAgents(false);
    };
  }, [actions, live]);

  const selected = ai?.providers.find((provider) => provider.id === ai.provider) ?? null;
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
        title={t("settings.backgroundAi")}
        note={ai?.unavailable_reason ?? t("settings.backgroundAiDescription")}
      >
        <Row label={t("common.agent")} detail={aiError ? <Note tone="error" data-ai-error="true">{t("settings.notSaved", { reason: aiError })}</Note> : null}>
          <Select
            value={ai?.provider ?? undefined}
            disabled={!ai || ai.providers.length === 0}
            onValueChange={(value) => {
              setChangedAt(Date.now());
              actions.chooseAi(value);
            }}
          >
            <SelectTrigger aria-label={t("settings.backgroundAgent")} data-ai-provider="true">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {(ai?.providers ?? []).map((provider) => (
                <SelectItem key={provider.id} value={provider.id}>
                  {provider.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Status tone="muted">{ai?.chosen ? t("settings.chosen") : t("settings.defaultChoice")}</Status>
        </Row>
        <Row
          label={t("common.model")}
          detail={selected?.models_unavailable_reason ? <Note>{t("settings.modelsUnavailable", { reason: selected.models_unavailable_reason })}</Note> : null}
        >
          <Select
            value={selected?.model ?? undefined}
            disabled={!selected || offeredModels(selected).length < 2}
            onValueChange={(value) => {
              if (!selected) return;
              setChangedAt(Date.now());
              actions.chooseAi(selected.id, value);
            }}
          >
            <SelectTrigger aria-label={t("settings.backgroundModel")} data-ai-model="true">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {(selected ? offeredModels(selected) : []).map((model) => (
                <SelectItem key={model} value={model}>
                  {model}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Row>
        <Row label={t("settings.agentSummary")} detail={<Note>{t("settings.agentSummaryDescription")}</Note>}>
          <Switch
            checked={ai?.agent_summary ?? true}
            disabled={!ai}
            onCheckedChange={(checked) => {
              setChangedAt(Date.now());
              actions.setAgentSummary(checked);
            }}
            aria-label={t("settings.agentSummary")}
            data-ai-agent-summary={String(ai?.agent_summary ?? true)}
          />
        </Row>
        {selected && selected.state !== "ready" && selected.state !== "unread" ? (
          <Row label={<Note tone="warn" data-ai-degraded="true">{t("settings.backgroundDegraded", { agent: selected.label, status: selected.headline || selected.state })}</Note>} />
        ) : null}
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
    </>
  );
}
