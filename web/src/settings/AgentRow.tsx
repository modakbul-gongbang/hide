import { ChevronRightIcon, ExternalLinkIcon } from "lucide-react";
import { useId, useState } from "react";
import type { Actions } from "../actions";
import { AgentMark } from "../components/agent-mark";
import { Note, Row, Status } from "../components/settings-rows";
import { Button } from "../components/ui/button";
import { Switch } from "../components/ui/switch";
import { useInterfaceTranslation } from "../i18n/client";
import { kitPartText } from "../settings";
import type { Device, KitAgent } from "../snapshot";
import { useUiStore } from "../ui";
import { agentLeftover, agentProblems, agentStatus, installDocUrl } from "./agentRows";
import { PartialChip } from "./PartialChip";

/** The link an agent row offers, as the row words it: a label and the arrow that says it leaves the app. */
function OutLink({ href, label, aria, className = "", ...data }: { href: string; label: string; aria: string; className?: string } & Record<`data-${string}`, string>) {
  return (
    <a
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      aria-label={aria}
      className={`inline-flex items-center gap-xs text-body text-primary outline-none focus-visible:ring-1 focus-visible:ring-ring ${className}`}
      {...data}
    >
      {label}
      <ExternalLinkIcon aria-hidden="true" className="size-(--size-icon-sm)" />
    </a>
  );
}

function Name({ agent }: { agent: KitAgent }) {
  return (
    <span className="flex min-w-0 items-center gap-sm">
      <AgentMark agent={agent.id} label={agent.label} />
      <span className="min-w-0 truncate font-semibold">{agent.label}</span>
    </span>
  );
}

/**
 * An agent the machine does not have (B8): its mark, its name and the link to
 * the vendor's installation guide. Nothing to switch, so no switch.
 */
export function NotInstalledRow({ agent }: { agent: KitAgent }) {
  const { t } = useInterfaceTranslation();
  const href = installDocUrl(agent.id);
  return (
    <Row label={<Name agent={agent} />} data-agent-row={`${agent.id}:not-installed`}>
      {href ? <OutLink href={href} label={t("agents.install")} aria={t("agents.installAria", { agent: agent.label })} data-agent-install={agent.id} /> : null}
    </Row>
  );
}

/**
 * One installed agent of one machine (D-08, D-09): its mark and name, the Partial
 * chip when Hide does only some things for it, the status of an agent that is
 * on, and the switch. A Docs link shows on hover or keyboard focus only (B12).
 * What needs the operator shows under the row, on this row alone: a part that
 * failed or was removed with its Reinstall (B13, B20), and the sessions that
 * run without Hide with a way to their panes (B17).
 */
export function AgentRow({ device, agent, actions, onAct }: { device: Device; agent: KitAgent; actions: Actions; onAct: () => void }) {
  const { t } = useInterfaceTranslation();
  const busy = device.kit?.busy === true;
  const status = agentStatus(agent);
  const problems = agentProblems(agent);
  const leftover = agentLeftover(agent);
  const [open, setOpen] = useState(false);
  const listId = useId();
  const sessions = agent.sessions;
  const hidden = sessions?.not_connected_hidden ?? 0;
  const worst = problems.some(({ piece }) => piece.state === "failed") ? "error" : "warn";
  return (
    <Row
      className="group/agent"
      label={
        <span className="flex min-w-0 flex-wrap items-center gap-x-sm gap-y-xs">
          <Name agent={agent} />
          <OutLink
            href={agent.doc_url}
            label={t("agents.docs")}
            aria={t("agents.docsAria", { agent: agent.label })}
            className="opacity-0 transition-opacity focus-visible:opacity-100 group-focus-within/agent:opacity-100 group-hover/agent:opacity-100 [@media(hover:none)]:opacity-100"
            data-agent-docs={agent.id}
          />
        </span>
      }
      data-agent-row={`${device.id}:${agent.id}:${agent.enabled ? "on" : "off"}`}
      detail={
        <>
          {problems.length > 0 ? (
            <div className="flex flex-wrap items-center gap-x-sm gap-y-xs" data-agent-problem={`${device.id}:${agent.id}`}>
              <Status tone={worst}>
                {problems.map(({ part, piece }) => t("agents.problem", { part: t(`agents.part.${part}`), state: kitPartText(piece, t) })).join("; ")}
              </Status>
              <Button
                variant="secondary"
                size="sm"
                disabled={busy}
                onClick={() => {
                  onAct();
                  actions.reinstallKit(device.id, [], [agent.id]);
                }}
                data-hook-reinstall={`${device.id}:${agent.id}`}
              >
                {busy ? t("settings.reinstalling") : t("settings.reinstall")}
              </Button>
            </div>
          ) : null}
          {leftover ? (
            <Note tone="error" data-agent-leftover={`${device.id}:${agent.id}`}>
              {leftover.reason ? t("agents.leftover", { reason: leftover.reason }) : t("agents.leftoverNoReason")}
            </Note>
          ) : null}
          {open && sessions && status.kind === "sessions" ? (
            <div id={listId} role="group" aria-label={t("agents.notConnectedAria", { agent: agent.label })} className="mt-xs divide-y divide-border rounded-sm border border-border" data-agent-sessions={`${device.id}:${agent.id}`}>
              <p className="px-sm py-xs text-caption text-muted-foreground">{t("agents.notConnectedHelp")}</p>
              {sessions.not_connected.map((session) => (
                <div key={session.pane_id} className="flex flex-wrap items-center gap-x-md gap-y-xs px-sm py-xs" data-agent-session={session.pane_id}>
                  <span className="flex min-w-[min(100%,var(--size-settings-control-w))] flex-1 flex-col">
                    <span className="break-words text-body text-foreground">{session.title}</span>
                    <span className="break-words text-caption text-muted-foreground">
                      {session.project} · {session.pane_id}
                    </span>
                  </span>
                  <Button
                    variant="secondary"
                    size="sm"
                    aria-label={t("agents.goToPaneAria", { pane: session.pane_id })}
                    onClick={() => {
                      // The pane shows behind the sheet, so the sheet gets out of its way.
                      actions.focusPane(session.pane_id);
                      useUiStore.getState().closeOverlay("settings");
                    }}
                    data-agent-go-to-pane={session.pane_id}
                  >
                    {t("agents.goToPane")}
                  </Button>
                </div>
              ))}
              {hidden > 0 ? (
                <p className="px-sm py-xs text-caption text-muted-foreground" data-agent-sessions-more={String(hidden)}>
                  {t("agents.moreSessions", { count: hidden })}
                </p>
              ) : null}
            </div>
          ) : null}
        </>
      }
    >
      {agent.partial ? <PartialChip agent={agent} /> : null}
      {status.kind === "ready" ? (
        <Status tone="ok" data-agent-status={`${device.id}:${agent.id}:ready`}>
          {t("agents.status.ready")}
        </Status>
      ) : null}
      {status.kind === "sessions" ? (
        <>
          {status.connected > 0 ? (
            <Status tone="ok" data-agent-status={`${device.id}:${agent.id}:connected`}>
              {t("agents.status.connected", { count: status.connected })}
            </Status>
          ) : null}
          {status.notConnected > 0 ? (
            <button
              type="button"
              aria-expanded={open}
              aria-controls={open ? listId : undefined}
              onClick={() => setOpen((current) => !current)}
              className="inline-flex cursor-pointer items-center gap-xxs rounded-xs outline-none focus-visible:ring-1 focus-visible:ring-ring"
              data-agent-status={`${device.id}:${agent.id}:not-connected`}
            >
              <Status tone="warn">{t("agents.status.notConnected", { count: status.notConnected })}</Status>
              <ChevronRightIcon aria-hidden="true" className={`size-(--size-icon) text-muted-foreground transition-transform ${open ? "rotate-90" : ""}`} />
            </button>
          ) : null}
        </>
      ) : null}
      <Switch
        checked={agent.enabled}
        disabled={busy}
        onCheckedChange={(checked) => {
          onAct();
          actions.setKitAgent(device.id, agent.id, checked);
        }}
        aria-label={t(agent.enabled ? "agents.switchOff" : "agents.switchOn", { agent: agent.label })}
        data-agent-switch={`${device.id}:${agent.id}:${agent.enabled ? "on" : "off"}`}
      />
    </Row>
  );
}
