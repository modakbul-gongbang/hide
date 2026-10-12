// Agent notifications in the desktop app (PRD first-run-onboarding D-09 to
// D-11, B31-B39). hided judges which root agents newly need the operator or
// finished, with the phone push's rules, and sends this window an
// `agent_notice` frame while it asks for them; this window words each one in
// the interface language and hands it to the desktop host, which drops it
// while a Hide window has the keyboard. A click opens that agent's pane, and
// an agent the operator has seen takes its notification back.

import { useEffect } from "react";
import type { Actions } from "./actions";
import { AGENT_NOTICE_PREFIX } from "./firstRun";
import type { AgentNotice } from "./generated/hided-ws";
import { hostBridge } from "./host";
import { useInterfaceTranslation } from "./i18n/client";
import { useShellStore } from "./store";

type Listener = (notice: AgentNotice) => void;
const listeners = new Set<Listener>();

/** One `agent_notice` frame from hided; nothing listening is no notification, which is the answer. */
export function receiveAgentNotice(payload: AgentNotice): void {
  for (const listener of listeners) listener(payload);
}

/** What the host may show at most (desktop `NOTIFY_LIMITS`); longer text is cut here, since the host refuses it. */
const TITLE_LIMIT = 120;
const BODY_LIMIT = 400;

function cut(text: string, limit: number): string {
  return text.length <= limit ? text : `${text.slice(0, limit - 1)}…`;
}

/** Shows agent notifications through the desktop host while the switch is on, and opens what a click names. */
export function useAgentNotifications(actions: Actions) {
  const { t } = useInterfaceTranslation();
  const bridge = hostBridge();
  const live = useShellStore((s) => s.connection === "live");
  const enabled = useShellStore((s) => s.rest?.ui_state?.desktop_notifications === true);
  const wanted = bridge !== null && live && enabled;
  // A reconnect is a new connection whose demand starts empty, so it is said again each time the page is live.
  useEffect(() => {
    if (!wanted) return;
    actions.wantAgentNotices(true);
    return () => actions.wantAgentNotices(false);
  }, [actions, wanted]);
  useEffect(() => {
    if (!bridge || !wanted) return;
    const listener: Listener = ({ notices, clear }) => {
      if (clear.length > 0) bridge.withdrawNotifications(clear.map((tag) => `${AGENT_NOTICE_PREFIX}${tag}`));
      for (const notice of notices) {
        const state = notice.state === "needs_you" ? t("firstRun.notice.needsYou") : t("firstRun.notice.done");
        const body = notice.place ? t("firstRun.notice.body", { state, project: notice.place }) : state;
        bridge.notify(`${AGENT_NOTICE_PREFIX}${notice.tag}`, cut(notice.title, TITLE_LIMIT), cut(body, BODY_LIMIT), { unlessFocused: true });
      }
    };
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  }, [bridge, wanted, t]);
  useEffect(() => {
    if (!bridge) return undefined;
    return bridge.onNotificationOpen((id) => {
      if (!id.startsWith(AGENT_NOTICE_PREFIX)) return;
      // The tag is `<device id>|<pane id>`; the pane id names the agent wherever it runs.
      const tag = id.slice(AGENT_NOTICE_PREFIX.length);
      const paneId = tag.slice(tag.indexOf("|") + 1);
      if (paneId) actions.openAgent(paneId);
    });
  }, [bridge, actions]);
}
