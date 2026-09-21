import { useShellStore, type AgentRow } from "./store";
import type { DispatchFn } from "./ws";

function herdrRowLabel(state: string | null): string | null {
  if (state === "unconfigured" || state === "socket_missing") return "Herdr 소켓 없음";
  if (state === "not_connected" || state === "unreachable" || state === "stale") {
    return "Herdr 무응답";
  }
  return null;
}

export function Sidebar({ dispatch }: { dispatch: DispatchFn }) {
  const agents = useShellStore((s) => s.agents);
  const herdrState = useShellStore((s) => s.herdrState);
  const focusedPaneId = useShellStore((s) => s.focusedPaneId);
  const status = herdrRowLabel(herdrState);

  return (
    <nav className="flex h-full w-[var(--size-sidebar-ideal)] flex-col bg-sidebar text-primary">
      {status ? (
        <div className="border-b border-divider px-md py-sm text-caption text-muted">{status}</div>
      ) : null}
      <ul className="flex-1 overflow-auto">
        {agents.map((agent) => (
          <SidebarRow
            key={agent.id}
            agent={agent}
            selected={agent.pane_id === focusedPaneId}
            onSelect={() =>
              dispatch({
                schema_version: 2,
                kind: "focus_pane",
                payload: { pane_id: agent.pane_id, origin: "operator" },
              })
            }
          />
        ))}
      </ul>
    </nav>
  );
}

function SidebarRow({
  agent,
  selected,
  onSelect,
}: {
  agent: AgentRow;
  selected: boolean;
  onSelect: () => void;
}) {
  const attention = agent.group === "needs_you" || agent.unread;
  return (
    <li>
      <button
        type="button"
        onClick={onSelect}
        data-pane={agent.pane_id}
        data-attention={attention ? "true" : "false"}
        className={`flex w-full flex-col items-start px-md py-xs text-left ${
          selected ? "bg-elevated" : ""
        } ${agent.emphasized || attention ? "text-primary" : "text-secondary"}`}
      >
        <span className="flex w-full items-baseline gap-xs text-body">
          <span className="w-[var(--size-agent-mark)] font-mono text-caption">{agent.symbol}</span>
          <span className="flex-1 truncate">{agent.identity_label}</span>
          <span className="text-micro text-muted">{agent.elapsed}</span>
        </span>
        <span className="pl-[var(--size-agent-mark)] text-caption text-secondary">
          {agent.unknown ? "unknown" : `${agent.agent_kind}${agent.detail ? ` / ${agent.detail}` : ""}`}
        </span>
      </button>
    </li>
  );
}
