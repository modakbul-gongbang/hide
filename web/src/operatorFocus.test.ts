import { beforeEach, describe, expect, it } from "vitest";
import { appliedIn, caughtUp, isOperatorFocus, numbered, operatorFocusClientId } from "./operatorFocus";
import { useShellStore } from "./store";

const focus = (origin: string) => ({ schema_version: 2, kind: "focus_pane", payload: { pane_id: "p1", origin } });

function snapshotFrame(operator_focus: { client_id: string; sequence: number }[]) {
  return { type: "snapshot", payload: { revision: 1, rest: { focused: { pane_id: "p1", operator_focus } } } };
}

describe("operator focus numbering", () => {
  beforeEach(() => useShellStore.setState({ operatorFocusSent: 0, operatorFocusApplied: 0 }));

  it("numbers only this machine's operator focus", () => {
    expect(isOperatorFocus(focus("operator"))).toBe(true);
    expect(isOperatorFocus(focus("restore"))).toBe(false);
    expect(isOperatorFocus({ kind: "key", payload: { origin: "operator" } })).toBe(false);
    expect(numbered(focus("operator"), 7).payload).toMatchObject({ client_id: operatorFocusClientId, sequence: 7, origin: "operator" });
  });

  it("reads only this page's number from the snapshot", () => {
    const rest = snapshotFrame([{ client_id: "someone-else", sequence: 9 }, { client_id: operatorFocusClientId, sequence: 3 }]).payload.rest;
    expect(appliedIn(rest)).toBe(3);
    expect(appliedIn({ focused: { pane_id: "p1" } })).toBeNull();
  });

  it("holds DOM focus back until the snapshot includes the last number sent", () => {
    const { noteOperatorFocusSent, applyFrame } = useShellStore.getState();
    expect(caughtUp(useShellStore.getState())).toBe(true);
    noteOperatorFocusSent(2);
    expect(caughtUp(useShellStore.getState())).toBe(false);
    applyFrame(snapshotFrame([{ client_id: operatorFocusClientId, sequence: 1 }]));
    expect(caughtUp(useShellStore.getState())).toBe(false);
    applyFrame(snapshotFrame([{ client_id: operatorFocusClientId, sequence: 2 }]));
    expect(caughtUp(useShellStore.getState())).toBe(true);
    // An older answer arriving late never lowers what was applied.
    applyFrame(snapshotFrame([{ client_id: operatorFocusClientId, sequence: 1 }]));
    expect(caughtUp(useShellStore.getState())).toBe(true);
  });

  it("does not wait on a number a closed socket lost", () => {
    const { noteOperatorFocusSent, releaseOperatorFocus } = useShellStore.getState();
    noteOperatorFocusSent(4);
    releaseOperatorFocus();
    expect(caughtUp(useShellStore.getState())).toBe(true);
  });
});
