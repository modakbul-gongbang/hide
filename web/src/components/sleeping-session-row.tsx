import { Loader2Icon, MoonIcon } from "lucide-react";
import { AgentMark } from "../AgentMark";
import type { Actions } from "../actions";
import { useInterfaceTranslation } from "../i18n/client";
import type { SleepingSession } from "../snapshot";
import { sleepingCaption } from "../sleeping-session";
import { Button } from "./ui/button";
import { Hint } from "./ui/tooltip";

/** Reuses the sidebar's row tokens, with no fake pane, lineage or open action. */
export function SleepingSessionRow({ session, actions, inset = "var(--spacing-xs)" }: {
  session: SleepingSession;
  actions: Pick<Actions, "wakeSleepingSession" | "checkSleepingSession">;
  inset?: string;
}) {
  const { t } = useInterfaceTranslation();
  const uncertain = session.phase === "close_unknown" || session.phase === "wake_unknown";
  const caption = t(sleepingCaption(session));
  return (
    <li data-sleeping-session={session.sleep_id} className="flex min-w-0 items-center gap-xs rounded-sm py-xs pr-xs text-body" style={{ paddingLeft: inset }}>
      <Hint label={caption}>
        <span className="flex min-h-(--size-sidebar-line) shrink-0 items-center text-muted-foreground">
          {session.checking || (!session.wake_available && !uncertain)
            ? <Loader2Icon aria-hidden="true" className="size-(--size-status-mark) animate-spin" />
            : <MoonIcon aria-hidden="true" className="size-(--size-status-mark)" />}
        </span>
      </Hint>
      <AgentMark kind={session.kind} />
      <Hint label={[session.identity_label, session.reason, caption].filter(Boolean).join("\n")}>
        <span className="min-w-0 flex-1 truncate text-subtle-foreground">{session.identity_label}</span>
      </Hint>
      {session.wake_available ? (
        <Button variant="ghost" size="sm" onClick={() => actions.wakeSleepingSession(session.sleep_id)}>{t("panes.sleep.wake")}</Button>
      ) : uncertain ? (
        <Button variant="ghost" size="sm" disabled={session.checking} onClick={() => actions.checkSleepingSession(session.sleep_id)}>{session.checking ? t("agents.checking") : t("workspace.checkStatus")}</Button>
      ) : <span className="shrink-0 text-caption text-muted-foreground">{caption}</span>}
    </li>
  );
}
