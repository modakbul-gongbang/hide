/**
 * The Factory's macOS notifications (PRD factory-observer D-50): the shell
 * names an item that newly became the operator's turn, or a main that broke,
 * and the host shows it while the app runs. Clicking it brings the window
 * forward and hands the item's id back to the shell, which opens it. Herdr's
 * own notification cannot carry where a click goes, which is why the host
 * shows these and the daemon does not.
 */

/** What the shell may ask for; longer text is refused rather than cut by the system. */
export const NOTIFY_LIMITS = { id: 200, title: 120, body: 400 } as const;

/** Notifications kept for their click; an older one still shows but its click is lost. */
export const NOTIFY_KEPT = 16;

export type NotifyRequest = { id: string; title: string; body: string };

/** The shell's request when it is well formed, or why it is refused. */
export function notifyRequest(value: unknown): NotifyRequest | { refused: "shape" | "length" } {
  if (typeof value !== "object" || value === null) return { refused: "shape" };
  const { id, title, body } = value as Record<string, unknown>;
  if (typeof id !== "string" || typeof title !== "string" || typeof body !== "string") return { refused: "shape" };
  if (id.length === 0 || title.trim().length === 0) return { refused: "shape" };
  if (id.length > NOTIFY_LIMITS.id || title.length > NOTIFY_LIMITS.title || body.length > NOTIFY_LIMITS.body) return { refused: "length" };
  return { id, title, body };
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

  /** Forgets `id` once its notification was clicked or closed. */
  forget(id: string): void {
    const index = this.kept.findIndex((entry) => entry.id === id);
    if (index >= 0) this.kept.splice(index, 1);
  }

  get size(): number {
    return this.kept.length;
  }
}
