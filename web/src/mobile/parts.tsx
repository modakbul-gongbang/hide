// The pieces a phone row and its detail share (PRD mobile-companion B20,
// B24): the status mark in the desktop row's tone, the provider mark, the
// task name, and the project · branch with the SSH device's chip.

import { ServerIcon } from "lucide-react";
import { AgentMark } from "../AgentMark";
import { StatusMark } from "../components/status-mark";
import type { PhoneAgent, Tone } from "./protocol";

export const TONE_TEXT: Record<Tone, string> = {
  error: "text-destructive",
  warning: "text-warning",
  working: "text-agent-working",
  success: "text-success",
  subtle: "text-muted-foreground",
};

export function AgentHead({ agent, large }: { agent: PhoneAgent; large: boolean }) {
  return (
    <span className="flex min-w-0 items-center gap-sm">
      <StatusMark symbol={agent.symbol} className={TONE_TEXT[agent.tone]} />
      <AgentMark kind={agent.agent_kind} />
      <span
        className={`min-w-0 truncate ${large ? "text-headline font-semibold" : "text-title font-medium"} ${
          agent.emphasized || large ? "text-foreground" : "text-subtle-foreground"
        }`}
      >
        {agent.title}
      </span>
    </span>
  );
}

export function Place({ agent }: { agent: PhoneAgent }) {
  if (!agent.place && !agent.device_label) return null;
  return (
    <span className="flex min-w-0 items-center gap-sm pl-(--size-mobile-row-inset) text-body text-muted-foreground">
      {agent.place ? <span className="min-w-0 truncate">{agent.place}</span> : null}
      {agent.device_label ? (
        <span className="inline-flex shrink-0 items-center gap-xs rounded-md bg-secondary px-sm text-body text-subtle-foreground" data-phone-device={agent.device_label}>
          <ServerIcon aria-hidden="true" className="size-(--size-icon-sm)" />
          {agent.device_label}
        </span>
      ) : null}
    </span>
  );
}
