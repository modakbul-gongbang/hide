import type { MessageKey } from "./i18n/catalogs";
import type { SleepingSession } from "./snapshot";

export function sleepingCaption(session: Pick<SleepingSession, "phase" | "checking">): MessageKey {
  if (session.checking) return "agents.checking";
  switch (session.phase) {
    case "close_unknown":
    case "wake_unknown": return "common.unknown";
    case "sleeping": return "panes.sleep.sleeping";
    case "failed": return "panes.sleep.captionFailed";
    case "saving_close":
    case "saving_close_ready":
    case "closing": return "panes.transport.closing";
    default: return "panes.sleep.waking";
  }
}
