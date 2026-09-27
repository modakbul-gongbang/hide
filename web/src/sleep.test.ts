import { describe, expect, it } from "vitest";
import { paneMenuItems } from "./PaneRelations";
import { sleepAfterChoice, sleepingCount } from "./settings";
import { sleepCaption, wakingLine } from "./sleep";
import type { AgentRow, PaneRow } from "./snapshot";

const HOUR = 60 * 60 * 1000;
const now = 100 * HOUR;

describe("a sleeping pane's words", () => {
  it("says how long the agent has slept, then waking, then that it could not resume", () => {
    expect(sleepCaption({ state: "sleeping", since_unix_ms: now - 22 * HOUR }, now)).toEqual({ text: "sleeping · 22h", moon: true });
    expect(sleepCaption({ state: "sleeping", since_unix_ms: now - 1000 }, now)).toEqual({ text: "sleeping", moon: true });
    expect(sleepCaption({ state: "waking", since_unix_ms: now - 22 * HOUR }, now)).toEqual({ text: "waking…", moon: true });
    expect(sleepCaption({ state: "failed", since_unix_ms: now, reason: "The conversation couldn’t be resumed." }, now)).toEqual({
      text: "could not resume",
      moon: false,
    });
  });

  it("names the age of the conversation a wake resumes", () => {
    expect(wakingLine({ state: "waking", since_unix_ms: now - 3 * 24 * HOUR }, now)).toBe("Resuming the conversation from 3d ago");
    expect(wakingLine({ state: "waking", since_unix_ms: now }, now)).toBe("Resuming the conversation");
  });
});

describe("the Sleep idle agents setting and the pane menu", () => {
  it("reads a stored choice and reads anything it does not offer as Never", () => {
    expect([null, undefined, 12, 24, 72, 5, "24"].map(sleepAfterChoice)).toEqual(["never", "never", "12", "24", "72", "never", "never"]);
  });

  it("counts only this machine's sleeping agents", () => {
    const row = (pane_id: string, sleeping: boolean) =>
      ({ pane_id, ...(sleeping ? { sleep: { state: "sleeping", since_unix_ms: 0 } } : {}) }) as AgentRow;
    expect(sleepingCount([row("w1:p1", true), row("w1:p2", false), row("remote:mini:pane:w1:p1", true)])).toBe(1);
  });

  it("offers Sleep agent on an awake agent pane, disabled with the core's reason when it cannot sleep", () => {
    const pane = (sleep_action?: PaneRow["sleep_action"]) =>
      ({ id: "w1:p1", sleep_action, children: null, lineage_path: [] }) as unknown as PaneRow;
    const sleepItem = (row: PaneRow) => paneMenuItems(row, "reviewer").find((item) => item.id === "sleep_agent");
    expect(sleepItem(pane({ available: true }))).toMatchObject({ label: "Sleep agent", unavailable: null });
    expect(sleepItem(pane({ available: false, reason: "This agent is working" }))).toMatchObject({ unavailable: "This agent is working" });
    expect(sleepItem(pane())).toBeUndefined();
  });
});
