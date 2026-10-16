import type { Actions } from "../actions";
import { AgentRowMark } from "../AgentRowMark";
import { agentProductName } from "../agentAdapters";
import { useInterfaceTranslation } from "../i18n/client";
import type { SleepingSession } from "../snapshot";
import { sleepingCaption } from "../sleeping-session";
import { Button } from "./ui/button";
import { StatusMark } from "./status-mark";
import { Hint } from "./ui/tooltip";

/** Reuses the sidebar's row tokens, with no fake pane, lineage or open action. */
export function SleepingSessionRow({ session, actions, inset = "var(--spacing-xs)" }: {
  session: SleepingSession;
  actions: Pick<Actions, "wakeSleepingSession" | "checkSleepingSession">;
  inset?: string;
}) {
  const { t } = useInterfaceTranslation();
  const uncertain = session.phase === "close_unknown" || session.phase === "wake_unknown";
  const pending = session.phase !== "sleeping" && session.phase !== "failed" && !uncertain;
  const caption = t(sleepingCaption(session));
  return (
    <li data-sleeping-session={session.sleep_id} className="flex min-w-0 items-start gap-xs rounded-sm py-xs pr-xs text-body" style={{ paddingLeft: inset }}>
      {/* A row with no mark, dimmed whole, its state on its line (status-marks D-05, B16); the action stays at full strength. */}
      <span className="flex min-w-0 flex-1 items-start gap-xs opacity-(--opacity-dimmed)">
        <span className="flex min-h-(--size-sidebar-line) shrink-0 items-center gap-xs">
          <StatusMark mark="quiet" />
          <AgentRowMark kind={session.kind} />
        </span>
        <Hint label={[session.identity_label, agentProductName(session.kind), session.reason, caption].filter(Boolean).join("\n")}>
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="min-h-(--size-sidebar-line) truncate leading-(--size-sidebar-line) text-subtle-foreground">{session.identity_label}<span className="sr-only">, {agentProductName(session.kind)}, </span></span>
            <span className="truncate text-caption leading-(--size-sidebar-line-detail) text-subtle-foreground" data-sleeping-line={session.phase} aria-busy={session.checking || pending || undefined}>{caption}</span>
          </span>
        </Hint>
      </span>
      {session.wake_available ? (
        <Button variant="ghost" size="sm" onClick={() => actions.wakeSleepingSession(session.sleep_id)}>{t("panes.sleep.wake")}</Button>
      ) : uncertain ? (
        <Button variant="ghost" size="sm" disabled={session.checking} onClick={() => actions.checkSleepingSession(session.sleep_id)}>{session.checking ? t("agents.checking") : t("workspace.checkStatus")}</Button>
      ) : null}
    </li>
  );
}
