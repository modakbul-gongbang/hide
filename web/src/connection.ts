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

export function badgeText(state: ConnectionState, refused = false): string {
  switch (state) {
    case "connecting":
      return "connecting";
    case "live":
      return "";
    case "reconnecting":
      return "reconnecting";
    case "gone":
      return refused ? "연결 거부 - hide를 다시 실행하세요" : "gone - hide를 다시 실행하세요";
  }
}
