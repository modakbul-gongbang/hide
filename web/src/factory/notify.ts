// 내 차례 macOS notifications (PRD factory-observer D-50, B42): while the
// desktop app runs, an item that newly counts in 내 차례, or a main that newly
// broke, in a Factory with the setting on becomes one macOS notification; a
// notice never does. The first summary is the baseline, so opening the app
// does not announce what was already waiting. Clicking a notification opens
// its item, or the Factory whose main broke.

import { useEffect, useRef } from "react";
import type { Actions } from "../actions";
import { hostBridge } from "../host";
import { useInterfaceTranslation } from "../i18n/client";
import { useShellStore } from "../store";
import { itemWhy } from "./labels";
import type { FactorySummary, InboxItem } from "./model";
import { inboxKey } from "./view";

/** What the host shows at most; longer text is cut here, since the host refuses it (desktop `NOTIFY_LIMITS`). */
const TITLE_LIMIT = 120;
const BODY_LIMIT = 400;

/** What this window has already seen: the items counted in 내 차례 and the Factories whose main was broken. */
export type NotifySeen = { items: Set<string>; broken: Set<string> };

export type FactoryNotice =
  | { kind: "item"; id: string; item: InboxItem; project: string }
  | { kind: "main"; id: string; factory: string; project: string };

const ITEM_PREFIX = "item:";
const MAIN_PREFIX = "main:";

/** What became new since `seen`, in the Factories that asked for notifications, and what is seen now. */
export function newFactoryNotices(summary: FactorySummary, seen: NotifySeen | null): { notices: FactoryNotice[]; seen: NotifySeen } {
  const next: NotifySeen = { items: new Set(), broken: new Set() };
  const notices: FactoryNotice[] = [];
  const views = new Map(summary.factories.map((view) => [view.id, view]));
  for (const item of summary.inbox) {
    if (item.group === "notice") continue;
    const key = inboxKey(item);
    next.items.add(key);
    const view = views.get(item.factory);
    if (seen && !seen.items.has(key) && view?.macos_notifications && !view.closed) notices.push({ kind: "item", id: `${ITEM_PREFIX}${key}`, item, project: view.project_name });
  }
  for (const view of summary.factories) {
    if (!view.main_broken) continue;
    next.broken.add(view.id);
    if (seen && !seen.broken.has(view.id) && view.macos_notifications && !view.closed) notices.push({ kind: "main", id: `${MAIN_PREFIX}${view.id}`, factory: view.id, project: view.project_name });
  }
  return { notices, seen: next };
}

function cut(text: string, limit: number): string {
  return text.length <= limit ? text : `${text.slice(0, limit - 1)}…`;
}

/** Shows the Factory's notifications through the desktop host and opens what a click names. */
export function useFactoryNotifications(actions: Actions) {
  const { t } = useInterfaceTranslation();
  const summary = useShellStore((s) => s.factory?.summary ?? null);
  const seen = useRef<NotifySeen | null>(null);
  const bridge = hostBridge();
  useEffect(() => {
    if (!summary || !bridge) return;
    const { notices, seen: now } = newFactoryNotices(summary, seen.current);
    seen.current = now;
    for (const notice of notices) {
      if (notice.kind === "main") {
        bridge.notify(notice.id, cut(t("factory.notify.mainBroken", { project: notice.project }), TITLE_LIMIT), cut(t("factory.notify.mainBrokenBody"), BODY_LIMIT));
        continue;
      }
      const item = notice.item;
      const title = item.group === "merge" ? "factory.notify.merge" : item.group === "stopped" ? "factory.notify.stopped" : "factory.notify.answer";
      bridge.notify(notice.id, cut(t(title, { project: notice.project }), TITLE_LIMIT), cut(`${item.display_id} ${item.title} · ${itemWhy(item, t)}`, BODY_LIMIT));
    }
  }, [summary, bridge, t]);
  useEffect(() => {
    if (!bridge) return undefined;
    return bridge.onNotificationOpen((id) => {
      if (id.startsWith(MAIN_PREFIX)) return actions.openFactory({ factory: id.slice(MAIN_PREFIX.length) });
      if (!id.startsWith(ITEM_PREFIX)) return;
      const key = id.slice(ITEM_PREFIX.length);
      const item = useShellStore.getState().factory?.summary?.inbox.find((row) => inboxKey(row) === key);
      // An item answered meanwhile opens its Task instead, which still says what happened.
      if (item) actions.openFactory({ tab: "turn", factory: null, focus: key });
      else {
        const [factory, task] = key.split("/");
        if (factory && task) actions.openFactory({ task: { factory, task } });
      }
    });
  }, [bridge, actions]);
}
