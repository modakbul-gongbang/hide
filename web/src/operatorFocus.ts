// Telling a snapshot older than the operator's last click from a real move.
//
// The page moves keyboard focus to the pane each snapshot names, and the
// operator's click moves it first. A snapshot already on its way when the
// click was sent still names the earlier pane, and following it would pull
// the keys off the pane just clicked until the snapshot that includes the
// click arrives. So the page numbers each operator `focus_pane` it sends, the
// core answers with the last number it applied (`focused.operator_focus` in
// the snapshot), and the page follows the snapshot's pane only once that
// answer has caught up with what it sent. After that, a different pane is a
// real move (the core followed Herdr, or refused the click and kept its own
// value) and is followed as before. No time window is involved.

import type { SnapshotRest } from "./snapshot";

export type FocusEvent = { schema_version: number; kind: string; payload: Record<string, unknown> };

/** This page load's name in the core's record; the number is only unique within it. */
export const operatorFocusClientId: string = Array.from(crypto.getRandomValues(new Uint8Array(8)), (byte) =>
  byte.toString(16).padStart(2, "0"),
).join("");

/** True for the one event the numbering covers: this machine's operator focus. */
export function isOperatorFocus(event: { kind: string; payload: Record<string, unknown> }): boolean {
  return event.kind === "focus_pane" && event.payload.origin === "operator";
}

/** The event with this page's name and the given number on it. */
export function numbered<T extends FocusEvent>(event: T, sequence: number): T {
  return { ...event, payload: { ...event.payload, client_id: operatorFocusClientId, sequence } };
}

/** The last number the core says it applied for this page, or null when it names none. */
export function appliedIn(rest: SnapshotRest): number | null {
  const entry = rest.focused?.operator_focus?.find((ack) => ack.client_id === operatorFocusClientId);
  return entry ? entry.sequence : null;
}

/** Whether the snapshot includes every operator focus this page has sent on the open connection. */
export function caughtUp(state: { operatorFocusSent: number; operatorFocusApplied: number }): boolean {
  return state.operatorFocusApplied >= state.operatorFocusSent;
}
