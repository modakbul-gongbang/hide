/**
 * Keeps a failure that repeats on the same subject to one log line per
 * window. A machine that stays unreachable once logged the same
 * `collect_failed` line every few seconds (2265 in five days); the first
 * failure and every change of cause still log at once, a repeat only adds to
 * a count the next line carries, and recovery reports how many were folded.
 * A success does not end the window: a machine that fails, answers once and
 * fails again with the same cause is still one episode.
 */
export interface FailureLog {
  /** Writes a line for a new or changed failure, or once per window while it repeats; returns whether it wrote. */
  failed(subject: string, cause: string, fields: Record<string, unknown>): boolean;
  /** Writes one line if repeats were folded and resets that count; the window itself keeps running. Returns whether it wrote. */
  recovered(subject: string, fields?: Record<string, unknown>): boolean;
}

interface Episode { cause: string; loggedAt: number; folded: number }

export function createFailureLog(event: { failed: string; recovered: string }, write: (line: string) => void, windowMs: number, now: () => number = Date.now): FailureLog {
  const episodes = new Map<string, Episode>();
  const line = (name: string, fields: Record<string, unknown>): void => write(`${JSON.stringify({ event: name, at: new Date(now()).toISOString(), ...fields })}\n`);
  return {
    failed(subject, cause, fields) {
      const episode = episodes.get(subject);
      if (episode !== undefined && episode.cause === cause && now() - episode.loggedAt < windowMs) { episode.folded += 1; return false; }
      line(event.failed, { ...fields, ...(episode !== undefined && episode.folded > 0 ? { repeated: episode.folded } : {}) });
      episodes.set(subject, { cause, loggedAt: now(), folded: 0 });
      return true;
    },
    recovered(subject, fields = {}) {
      const episode = episodes.get(subject);
      if (episode === undefined || episode.folded === 0) return false;
      line(event.recovered, { ...fields, repeated: episode.folded });
      episode.folded = 0;
      return true;
    },
  };
}
