import type { TFunction } from "i18next";

export type ConnectionState = "connecting" | "live" | "reconnecting" | "gone";

export const BACKOFF_START_MS = 500;
export const BACKOFF_MAX_MS = 30_000;
export const HEALTH_FAILS_TO_GONE = 3;

export function nextBackoff(currentMs: number): number {
  return Math.min(BACKOFF_MAX_MS, Math.max(BACKOFF_START_MS, currentMs * 2));
}

export function connectionAfterHealthFails(fails: number): ConnectionState {
  return fails >= HEALTH_FAILS_TO_GONE ? "gone" : "reconnecting";
}

export function badgeText(state: ConnectionState, refused: boolean, t: TFunction<"translation">): string {
  switch (state) {
    case "connecting":
      return t("shell.connection.connecting");
    case "live":
      return "";
    case "reconnecting":
      return t("shell.connection.reconnecting");
    case "gone":
      return refused ? t("shell.connection.refused") : t("shell.connection.gone");
  }
}
