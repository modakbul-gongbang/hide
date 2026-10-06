import { XIcon } from "lucide-react";
import { memo, useEffect, useRef, useState } from "react";
import type { Actions } from "./actions";
import { StatusMark } from "./components/status-mark";
import { markTone } from "./agentRow";
import { AgentMark } from "./AgentMark";
import { Button } from "./components/ui/button";
import { Input } from "./components/ui/input";
import { Keycap } from "./components/ui/keycap";
import { Hint, Tooltip, TooltipTrigger, TooltipContent } from "./components/ui/tooltip";
import type { AsyncOperation, Checkout, StripTab } from "./snapshot";
import { useShellStore } from "./store";
import { tabFit, type AreaTabInteraction } from "./AreaTree";
import { useInterfaceTranslation } from "./i18n/client";
import { statusText } from "./agentStatus";
import type { TabFit } from "./areaLayout";

// The sole Agent tab rendering unit, reused by every local and device area.
const NONE: AsyncOperation[] = [];

/** Phases of a close the core is still confirming with Herdr (`operations.rs`). */
const IN_FLIGHT = new Set(["transmitting", "awaiting_topology", "unknown"]);

/** "closing…" while the core is still confirming a close with Herdr. */
export function closingSuffix(targetId: string, kind: "tab.close" | "pane.close", operations: AsyncOperation[]): boolean {
  return operations.some((op) => op.kind === kind && op.target_id === targetId && IN_FLIGHT.has(op.phase));
}

export function AgentTab({ number, entry, checkout, interaction, actions, renaming, onCancelRename }: { number: number | null; entry: StripTab; checkout: Checkout; interaction: AreaTabInteraction; actions: Actions; renaming: boolean; onCancelRename: () => void }) {
  const { t } = useInterfaceTranslation();
  const operations = useShellStore((s) => s.rest?.status?.async_operations) ?? NONE;
  const agent = checkout.tabs.find((row) => row.id === entry.source_id)?.agent;
  const identity = `${agent ? t("panes.agent.tabIdentityAgent", { kind: agent.agent_kind, label: entry.label }) : t("panes.agent.tabIdentityTerminal", { label: entry.label })}${agent ? ` · ${statusText(t, agent.status_code)}` : ""}`;
  return <TabButton entry={entry} editor={renaming ? <TabRenameInput key={entry.source_id} entry={entry} actions={actions} onCancel={onCancelRename} /> : null} identity={identity} mark={<>{agent ? <StatusMark symbol={agent.symbol} className={markTone(agent)} data-tab-status={agent.status_code} /> : null}<AgentMark kind={agent?.agent_kind} /></>} number={number} active={interaction.selected} areaActive={interaction.areaActive} closing={closingSuffix(entry.source_id, "tab.close", operations)} closeLabel={t("shell.closeTab", { title: entry.label })} fit={interaction.fit} dragging={interaction.dragging} onSelect={interaction.select} onClose={() => actions.closeTab(entry.source_id)} onPointerDown={interaction.press} />;
}

function TabRenameInput({ entry, actions, onCancel }: { entry: StripTab; actions: Actions; onCancel: () => void }) {
  const { t } = useInterfaceTranslation();
  const [value, setValue] = useState(entry.label);
  const [requestId, setRequestId] = useState<string | null>(null);
  const sent = useRef(false);
  const input = useRef<HTMLInputElement>(null);
  const receipt = useShellStore((state) => state.rest?.status?.tab_rename);
  const answer = receipt?.request_id === requestId ? receipt : null;
  const failed = answer?.phase === "failed";
  const pending = requestId !== null && !failed;
  useEffect(() => {
    const frame = requestAnimationFrame(() => { input.current?.focus(); input.current?.select(); });
    return () => cancelAnimationFrame(frame);
  }, []);
  useEffect(() => {
    if (answer?.phase === "succeeded") onCancel();
    if (failed) sent.current = false;
  }, [answer?.phase, failed, onCancel]);
  const cancel = () => {
    const tab = input.current?.closest<HTMLElement>("[role=tab]");
    onCancel();
    tab?.focus();
  };
  return (
    <Tooltip open={failed}>
      <TooltipTrigger asChild>
        <Input
          ref={input}
          aria-label={t("panes.agent.tabName")}
          data-renaming="true"
          aria-invalid={failed || undefined}
          aria-describedby={failed ? "tab-rename-failure" : undefined}
          className="h-(--size-control-sm) flex-1 px-xs text-caption"
          value={value}
          readOnly={pending}
          onChange={(event) => setValue(event.target.value)}
          onPointerDown={(event) => event.stopPropagation()}
          onClick={(event) => event.stopPropagation()}
          onBlur={onCancel}
          onKeyDown={(event) => {
            event.stopPropagation();
            if (event.nativeEvent.isComposing || event.keyCode === 229) return;
            if (event.key === "Escape") { event.preventDefault(); cancel(); }
            if (event.key === "Enter" && !sent.current) {
              event.preventDefault();
              sent.current = true;
              const id = crypto.randomUUID();
              setRequestId(id);
              actions.renameTab(entry.source_id, value, id);
            }
          }}
        />
      </TooltipTrigger>
      <TooltipContent side="bottom" align="start" className="max-w-none whitespace-nowrap text-destructive" id="tab-rename-failure" role="status">{t("shell.renameFailed")}</TooltipContent>
    </Tooltip>
  );
}

const TabButton = memo(function TabButton({
  entry,
  identity,
  editor,
  mark,
  active,
  number,
  closing,
  closeLabel,
  fit: slotFit,
  dragging,
  areaActive,
  onSelect,
  onClose,
  onPointerDown,
}: {
  entry: StripTab;
  identity: string;
  editor: React.ReactNode;
  mark: React.ReactNode;
  active: boolean;
  /** The digit a ⌘ hold shows on this tab, or null while none shows. */
  number: number | null;
  closing: boolean;
  closeLabel: string;
  /** How the tab draws its contents in its slot (`AreaTabInteraction.fit`). */
  fit: TabFit;
  dragging: boolean;
  areaActive: boolean;
  onSelect: () => void;
  onClose: () => void;
  onPointerDown: (event: React.PointerEvent<HTMLElement>) => void;
}) {
  const fit = tabFit(active, slotFit);
  const { t } = useInterfaceTranslation();
  return (
    <Hint label={identity} reveals>
    <div
      role="tab"
      aria-selected={active}
      aria-label={identity}
      tabIndex={0}
      data-tab={entry.source_id}
      data-tab-kind={entry.kind}
      data-closing={closing ? "true" : "false"}
      className={`group relative flex min-w-0 flex-1 cursor-default select-none items-center gap-xs text-caption outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring ${fit.tab} ${
        active ? `text-foreground ${areaActive ? "bg-background" : "bg-secondary"}` : "text-subtle-foreground hover:bg-accent"
      } ${dragging ? "opacity-[var(--opacity-dimmed)]" : ""}`}
      onPointerDown={onPointerDown}
      onClick={onSelect}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onSelect();
        }
      }}
    >
      {mark}
      {editor ?? <span className={`min-w-0 flex-1 truncate ${fit.title}`}>
        {entry.label}
        {closing ? <span className="text-muted-foreground"> {t("panes.transport.closing")}</span> : null}
      </span>}
      <Hint label={closeLabel}>
        <Button
          variant="ghost"
          size="icon-sm"
          className={`shrink-0 hover:bg-popover hover:text-foreground ${fit.close} ${number !== null ? "invisible" : `focus-visible:visible group-hover:visible ${active ? "visible" : "invisible"}`}`}
          aria-label={closeLabel}
          onPointerDown={(event) => event.stopPropagation()}
          onClick={(event) => {
            event.stopPropagation();
            onClose();
          }}
        >
          <XIcon />
        </Button>
      </Hint>
      {active && areaActive ? <span className="absolute inset-x-0 bottom-0 h-[var(--size-tab-indicator)] bg-foreground" /> : null}
      {/* The digit takes the close control's corner for the length of the hold; the control keeps its space and comes back with the release. */}
      {number !== null ? <Keycap number={number} /> : null}

    </div>
    </Hint>
  );
});
