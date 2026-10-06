import type { Actions } from "../actions";
import { Switch } from "../components/ui/switch";
import { Note, Status } from "../components/settings-rows";
import { useInterfaceTranslation } from "../i18n/client";
import { kitAgentLine, kitAgentSwitch, kitConsentTerms, kitPartLine, kitPartSwitch } from "../settings";
import type { Device } from "../snapshot";

export function KitTerms({ helperRoot, cliDir }: { helperRoot: string | null; cliDir: string | null }) {
  const { t } = useInterfaceTranslation();
  return (
    <ul className="list-disc space-y-xs pl-md text-body text-subtle-foreground" data-kit-terms="true">
      {kitConsentTerms(helperRoot, cliDir, t).map((term) => (
        <li key={term}>{term}</li>
      ))}
    </ul>
  );
}

/**
 * Each part of Hide's kit on one machine, in the same form for This Mac and
 * every device (PRD device-parity B7): a mark, the part, and where it is or
 * why it is not. A machine whose kit does not run says why instead.
 */
export function MachineKit({ device, actions }: { device: Device; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const kit = device.kit;
  if (!kit) return null;
  if (kit.unavailable) {
    return (
      <div className="mt-xs" data-machine-kit={`${device.id}:unavailable`}>
        <Note>{kit.unavailable}</Note>
      </div>
    );
  }
  if (kit.components.length === 0) {
    return (
      <div className="mt-xs" data-machine-kit={`${device.id}:${kit.busy ? "busy" : "unread"}`}>
        <Status tone="pending">{kit.busy ? t("devices.installingKit") : t("devices.kitOnConnection")}</Status>
      </div>
    );
  }
  return (
    <div className="mt-xs grid grid-cols-[auto_auto_minmax(0,1fr)_auto] gap-x-xs gap-y-xxs" data-machine-kit={`${device.id}:${kit.busy ? "busy" : "read"}`}>
      {kit.components.map((part) => {
        const line = kitPartLine(part, t);
        const switched = kitPartSwitch(part);
        const mark = part.state === "installed" ? "✓" : part.state === "absent" ? "–" : part.state === "off" ? "○" : part.state === "failed" ? "✕" : "!";
        const markTone = line.tone === "ok" ? "text-success" : line.tone === "muted" ? "text-muted-foreground" : line.tone === "error" ? "text-destructive" : "text-warning";
        return (
          <div key={part.id} className="col-span-4 grid grid-cols-subgrid text-caption" data-kit-part={`${device.id}:${part.id}:${part.state}`}>
            <span className={markTone} aria-hidden="true">
              {mark}
            </span>
            <span className="whitespace-nowrap text-foreground">{part.label}</span>
            {part.state === "installed" ? <span className="sr-only">{line.text}</span> : null}
            <span className="min-w-0 break-words text-subtle-foreground">
              {part.state === "installed" ? <span className="break-all font-mono">{part.location}</span> : `${line.text}${part.reason ? `: ${part.reason}` : ""}`}
              {/* An installed part can still carry a reason, such as a setting that applies to newly opened sessions. */}
              {part.state === "installed" && part.reason ? <span className="block text-muted-foreground" data-kit-part-note="">{part.reason}</span> : null}
            </span>
            {switched ? (
              <Switch
                checked={switched.on}
                disabled={kit.busy}
                onCheckedChange={(checked) => actions.setKitComponent(device.id, part.id, checked)}
                aria-label={t(switched.on ? "devices.kitSwitchOff" : "devices.kitSwitchOn", { part: part.label })}
                data-kit-part-switch={`${device.id}:${part.id}:${switched.on ? "on" : "off"}`}
              />
            ) : (
              <span aria-hidden="true" />
            )}
          </div>
        );
      })}
      {kit.agents
        .filter((agent) => kitAgentSwitch(agent) !== null)
        .map((agent) => {
          const line = kitAgentLine(agent, t);
          const switched = kitAgentSwitch(agent);
          const mark = !agent.enabled ? "○" : line.tone === "ok" ? "✓" : line.tone === "error" ? "✕" : "!";
          const markTone = line.tone === "ok" ? "text-success" : line.tone === "muted" ? "text-muted-foreground" : line.tone === "error" ? "text-destructive" : "text-warning";
          return (
            <div key={agent.id} className="col-span-4 grid grid-cols-subgrid text-caption" data-kit-agent={`${device.id}:${agent.id}:${agent.enabled ? "on" : "off"}`}>
              <span className={markTone} aria-hidden="true">
                {mark}
              </span>
              <span className="whitespace-nowrap text-foreground">{agent.label}</span>
              <span className="min-w-0 break-words text-subtle-foreground">{`${line.text}${line.reason ? `: ${line.reason}` : ""}`}</span>
              {switched ? (
                <Switch
                  checked={switched.on}
                  disabled={kit.busy}
                  onCheckedChange={(checked) => actions.setKitAgent(device.id, agent.id, checked)}
                  aria-label={t(switched.on ? "devices.kitSwitchOff" : "devices.kitSwitchOn", { part: agent.label })}
                  data-kit-agent-switch={`${device.id}:${agent.id}:${switched.on ? "on" : "off"}`}
                />
              ) : (
                <span aria-hidden="true" />
              )}
            </div>
          );
        })}
    </div>
  );
}
