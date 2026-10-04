import { useSyncExternalStore, type HTMLAttributes } from "react";
import { useInterfaceTranslation } from "../i18n/client";
import { formatUnit } from "../i18n/format";
import { requireInterfaceLanguage, type InterfaceLanguage } from "../i18n/locale";

// How long ago an agent last changed state, counted on one clock for the
// whole window (PRD labels-in-hided D-07, B8). The core publishes the moment
// once; time passing never republishes the snapshot. Each label subscribes on
// its own and re-renders only when its text changes, so a second passing
// touches no row and a minute passing touches one span per row.

const listeners = new Set<() => void>();
let timer: number | null = null;
let now = Date.now();

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  if (timer === null) {
    now = Date.now();
    timer = window.setInterval(() => {
      now = Date.now();
      for (const notify of listeners) notify();
    }, 1000);
  }
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0 && timer !== null) {
      window.clearInterval(timer);
      timer = null;
    }
  };
}

/** `42s`, `3m`, `2h`, `1d` in English: the largest whole unit; a moment from the future is `0s`. */
export function formatElapsed(language: InterfaceLanguage, elapsedMs: number): string {
  const seconds = Math.floor(Math.max(0, elapsedMs) / 1000);
  if (seconds < 60) return formatUnit(language, seconds, "second");
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return formatUnit(language, minutes, "minute");
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return formatUnit(language, hours, "hour");
  return formatUnit(language, Math.floor(hours / 24), "day");
}

/** The elapsed text for `since` on the shared clock; `null` when the core never measured it. */
export function useElapsed(since: number | null | undefined): string | null {
  const language = requireInterfaceLanguage(useInterfaceTranslation().i18n.language);
  return useSyncExternalStore(subscribe, () => (since == null ? null : formatElapsed(language, (timer === null ? Date.now() : now) - since)));
}

/** A span holding the elapsed text; nothing at all when the core has no time for the agent. */
export function Elapsed({ since, ...props }: { since: number | null | undefined } & HTMLAttributes<HTMLSpanElement>) {
  const text = useElapsed(since);
  return text === null ? null : <span {...props}>{text}</span>;
}
