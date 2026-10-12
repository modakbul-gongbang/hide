/**
 * The shell's system notifications: the Factory's (PRD factory-observer
 * D-50), an item that newly became the operator's turn or a main that broke,
 * and an agent's (PRD first-run-onboarding D-10), a root agent that newly
 * needs the operator or finished; the host shows them while the app runs.
 * Clicking one brings the window forward and hands its id back to the shell,
 * which opens what it names. Herdr's own notification cannot carry where a
 * click goes, which is why the host shows these and the daemon does not.
 */

/** What the shell may ask for; longer text is refused rather than cut by the system. */
export const NOTIFY_LIMITS = { id: 200, title: 120, body: 400 } as const;

/** Notifications kept for their click; an older one still shows but its click is lost. */
export const NOTIFY_KEPT = 16;

/** Ids one withdrawal may name; an agent judgment clears far fewer. */
export const WITHDRAW_LIMIT = 64;

/**
 * `unlessFocused`: an agent's notification (PRD first-run-onboarding D-10)
 * is not shown while a Hide window has the keyboard, since the operator is
 * already looking; the Factory's and the test notification always show.
 */
export type NotifyRequest = { id: string; title: string; body: string; unlessFocused: boolean };

/** The shell's request when it is well formed, or why it is refused. */
export function notifyRequest(value: unknown): NotifyRequest | { refused: "shape" | "length" } {
  if (typeof value !== "object" || value === null) return { refused: "shape" };
  const { id, title, body, unlessFocused = false } = value as Record<string, unknown>;
  if (typeof id !== "string" || typeof title !== "string" || typeof body !== "string" || typeof unlessFocused !== "boolean") return { refused: "shape" };
  if (id.length === 0 || title.trim().length === 0) return { refused: "shape" };
  if (id.length > NOTIFY_LIMITS.id || title.length > NOTIFY_LIMITS.title || body.length > NOTIFY_LIMITS.body) return { refused: "length" };
  return { id, title, body, unlessFocused };
}

/** The notification ids the shell withdraws (an agent the operator has seen, B35), or why the request is refused. */
export function withdrawRequest(value: unknown): string[] | { refused: "shape" | "length" } {
  if (!Array.isArray(value) || !value.every((id) => typeof id === "string" && id.length > 0)) return { refused: "shape" };
  if (value.length > WITHDRAW_LIMIT || value.some((id: string) => id.length > NOTIFY_LIMITS.id)) return { refused: "length" };
  return value as string[];
}

/** The shown notifications a click can still reach, newest last, at most `NOTIFY_KEPT`. */
export class KeptNotifications<T> {
  private readonly kept: { id: string; value: T }[] = [];

  /** Keeps `value` for `id`, replacing an earlier one for the same id; answers the one that dropped out, if any. */
  keep(id: string, value: T): T | null {
    const earlier = this.kept.findIndex((entry) => entry.id === id);
    const replaced = earlier >= 0 ? this.kept.splice(earlier, 1)[0]!.value : null;
    this.kept.push({ id, value });
    if (replaced !== null) return replaced;
    return this.kept.length > NOTIFY_KEPT ? this.kept.shift()!.value : null;
  }

  /** Takes `id` out, answering what was kept for it: a newer notification for the same id replaces it on screen (B35). */
  take(id: string): T | null {
    const index = this.kept.findIndex((entry) => entry.id === id);
    return index >= 0 ? this.kept.splice(index, 1)[0]!.value : null;
  }

  /** Forgets `id` once its notification was clicked or closed. */
  forget(id: string): void {
    const index = this.kept.findIndex((entry) => entry.id === id);
    if (index >= 0) this.kept.splice(index, 1);
  }

  get size(): number {
    return this.kept.length;
  }
}
