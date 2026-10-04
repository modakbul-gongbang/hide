import { FolderIcon, HomeIcon, ServerIcon } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { Actions } from "./actions";
import { rememberedSelection, modelToSend, type AgentSelection } from "./agentPicker";
import { AgentPicker } from "./components/agent-picker";
import { Button } from "./components/ui/button";
import { useEscapeLayer } from "./components/ui/layer";
import { Kbd } from "./components/ui/kbd";
import { Select, SelectContent, SelectGroup, SelectItem, SelectSeparator, SelectTrigger, SelectValue } from "./components/ui/select";
import { startAnswer, startRequestId } from "./startAnswer";
import { useStartPanel } from "./startDraft";
import { resolveTarget, startTargets, type StartTarget } from "./startTargets";
import { useShellStore } from "./store";
import { restoreFocus } from "./terminals";
import { useUiStore } from "./ui";

// The start panel (PRD home-device-rail D-17..D-21, B23-B31): ⌘K's position
// and width, no backdrop, the keyboard in the text box. What is written is the
// new agent's first prompt; Enter or 시작 sends one `agent_start_in_checkout`
// and the panel then follows only the answer that carries its request id.

/**
 * How long a start may go unanswered before the panel says so, so it never
 * waits for ever: longer than a device's Home sync (30 s) plus opening its
 * tab, so a slow start is not reported as lost while it still runs.
 */
const ANSWER_TIMEOUT_MS = 90_000;

/**
 * Follows the answer to the request the panel sent, whether or not the panel
 * is still open: a start that lands after Esc still spends the draft and
 * moves the center to its pane.
 */
function useStartAnswer() {
  const request = useStartPanel((s) => s.request);
  useEffect(() => {
    if (!request) return undefined;
    const settle = (): boolean => {
      const answer = startAnswer(useShellStore.getState().rest, request);
      const panel = useStartPanel.getState();
      if (answer.phase === "pending") return false;
      if (answer.phase === "refused" || answer.phase === "failed") {
        panel.fail(answer.message);
        return true;
      }
      const ui = useUiStore.getState();
      // A watched task reports the agent's own start after the panel is gone (the notice bar).
      if (answer.agentPhase) ui.setWatchedTask(answer.taskId);
      // The pane is opened as an agent row opens it, so a device's pane moves rail, sidebar and center together.
      if (answer.paneId) ui.setFocusWhenListed(answer.paneId);
      if (answer.agentPhase === "failed") panel.fail(answer.agentMessage ?? "에이전트가 시작되지 않았습니다.");
      else panel.finish(answer.agentPhase === "starting" ? answer.taskId : null);
      return true;
    };
    if (settle()) return undefined;
    const unsubscribe = useShellStore.subscribe(() => {
      if (settle()) unsubscribe();
    });
    const timer = window.setTimeout(() => {
      unsubscribe();
      useStartPanel.getState().fail("응답이 없습니다. 연결을 확인하고 다시 시작하세요.");
    }, ANSWER_TIMEOUT_MS);
    return () => {
      unsubscribe();
      window.clearTimeout(timer);
    };
  }, [request]);
}

/**
 * Follows a start whose tab opened while its agent was still starting: an
 * agent that then fails to start puts its text back in the draft with the
 * reason, so the next ⌘N shows both and nothing written is lost (B31).
 */
function useSpentStart() {
  const spent = useStartPanel((s) => s.spent);
  const operation = useShellStore((s) => s.rest?.task_operation);
  useEffect(() => {
    if (!spent) return;
    const panel = useStartPanel.getState();
    if (!operation || operation.id !== spent.taskId) return panel.settle();
    if (operation.agent_phase === "failed" || operation.agent_phase === "unknown") panel.restore(operation.agent_message ?? "에이전트가 시작되지 않았습니다.");
    else if (operation.agent_phase !== "starting") panel.settle();
  }, [spent, operation]);
}

export function StartPanelHost({ actions }: { actions: Actions }) {
  useStartAnswer();
  useSpentStart();
  const open = useStartPanel((s) => s.isOpen);
  // A palette or a dialog opening over the panel takes the place; the draft stays.
  const covered = useUiStore((s) => s.overlay !== "none" || s.workspaceDialog !== null);
  useEffect(() => {
    if (covered) useStartPanel.getState().close();
  }, [covered]);
  return open ? <StartPanel actions={actions} /> : null;
}

function TargetIcon({ target }: { target: StartTarget }) {
  if (target.kind === "checkout") return <FolderIcon aria-hidden="true" />;
  return target.deviceId === "local" ? <HomeIcon aria-hidden="true" /> : <ServerIcon aria-hidden="true" />;
}

function StartPanel({ actions }: { actions: Actions }) {
  const text = useStartPanel((s) => s.text);
  const chosen = useStartPanel((s) => s.target);
  const request = useStartPanel((s) => s.request);
  const failure = useStartPanel((s) => s.failure);
  const rest = useShellStore((s) => s.rest);
  const screen = useUiStore((s) => s.screen);
  const overSettings = useStartPanel((s) => s.overSettings);
  // What is in front is read again on every open; the panel itself lives only while open.
  const overviewProjectId = useUiStore((s) => s.overviewOpen || s.screen?.kind === "main" ? s.overviewProjectId : null);
  const targets = useMemo(() => startTargets(rest, screen, overSettings, overviewProjectId), [rest, screen, overSettings, overviewProjectId]);
  const target = resolveTarget(targets, chosen);
  const [selection, setSelection] = useState<AgentSelection>(() => rememberedSelection(useShellStore.getState().rest?.ui_state?.agent_start));
  const surface = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLInputElement>(null);
  const previous = useRef<HTMLElement | null>(document.activeElement instanceof HTMLElement ? document.activeElement : null);
  const working = request !== null;

  const dismiss = () => {
    useStartPanel.getState().close();
    restoreFocus(previous.current);
  };
  useEscapeLayer(true, dismiss);
  useEffect(() => {
    field.current?.focus();
  }, []);
  // A press outside closes, except inside the menus the panel opened: they render in their own portal.
  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      const at = event.target;
      if (!(at instanceof Element)) return;
      if (surface.current?.contains(at) || at.closest("[data-radix-popper-content-wrapper]")) return;
      useStartPanel.getState().close();
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => document.removeEventListener("pointerdown", onPointerDown, true);
  }, []);

  const canStart = target !== null && selection.kind !== "terminal" && !working;
  const start = () => {
    if (!canStart || !target || selection.kind === "terminal") return;
    const id = startRequestId();
    useStartPanel.getState().begin(id);
    actions.startAgent({
      target: target.kind === "home" ? { home: true } : { checkoutPath: target.checkoutPath },
      deviceId: target.deviceId,
      provider: selection.kind,
      model: modelToSend(selection),
      prompt: text.trim() || null,
      requestId: id,
    });
  };

  return (
    <div
      ref={surface}
      role="dialog"
      aria-label="에이전트 시작"
      data-start-panel="true"
      className="fixed left-1/2 top-(--size-settings-sheet-window-inset) z-50 flex w-(--size-search-sheet-w) max-w-[calc(100%-var(--spacing-xxl))] -translate-x-1/2 flex-col overflow-hidden rounded-lg border border-border bg-popover text-body text-popover-foreground shadow-lg"
    >
      <input
        ref={field}
        type="text"
        value={text}
        autoComplete="off"
        spellCheck={false}
        aria-label="첫 지시"
        placeholder="무엇을 시킬까요?"
        data-start-text="true"
        className="h-(--size-control-lg) w-full bg-transparent px-md pt-md pb-md text-body text-foreground outline-none placeholder:text-muted-foreground"
        onChange={(event) => useStartPanel.getState().setText(event.target.value)}
        onKeyDown={(event) => {
          if (event.key !== "Enter" || event.nativeEvent.isComposing) return;
          event.preventDefault();
          start();
        }}
      />
      {failure ? (
        <p role="alert" data-start-failure="true" className="whitespace-pre-line break-words px-md pb-xs text-caption text-destructive">
          {failure}
        </p>
      ) : null}
      <div className="flex items-center gap-xs border-t border-border px-sm py-sm">
        <Select value={target?.key ?? ""} disabled={working || targets.groups.length === 0} onValueChange={(key) => useStartPanel.getState().setTarget(key)}>
          <SelectTrigger aria-label="대상" className="min-w-0 flex-1" data-start-target={target?.key ?? ""}>
            <SelectValue placeholder="대상 없음" />
          </SelectTrigger>
          <SelectContent>
            {targets.groups.map((group, index) => (
              <SelectGroup key={group.deviceId} data-start-target-group={group.deviceId}>
                {index > 0 ? <SelectSeparator className="bg-secondary" /> : null}
                {group.items.map((item) => (
                  <SelectItem key={item.key} value={item.key} disabled={item.disabled !== null} data-start-target-option={item.key}>
                    <TargetIcon target={item} />
                    <span className="flex min-w-0 flex-col">
                      <span className="truncate">{item.label}</span>
                      {item.disabled ? <span className="text-caption text-muted-foreground">{item.disabled}</span> : null}
                    </span>
                  </SelectItem>
                ))}
              </SelectGroup>
            ))}
          </SelectContent>
        </Select>
        <AgentPicker actions={actions} value={selection} onChange={setSelection} disabled={working} />
        <Kbd aria-hidden="true">⏎</Kbd>
        <Button size="default" disabled={!canStart} onClick={start} data-start-submit="true">
          {working ? "시작하는 중…" : "시작"}
        </Button>
      </div>
    </div>
  );
}
