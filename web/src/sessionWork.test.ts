import { expect, it } from "vitest";
import { legacyAgentRow } from "../test/legacyAgentRow";
import type { SessionWorkSummary } from "./snapshot";
import { endedWork, recordTitle } from "./sessionWork";

it("moves one remote missed item to ended work on Resolve and back on revival", () => {
  const row = { ...legacyAgentRow({}), device_id: "remote", agent_kind: "claude", session_id: "same-native-id", resolved: null };
  const work: SessionWorkSummary = {
    session: { device_id: "remote", agent: "claude", session_id: "same-native-id" }, title: "Current goal", fallback_title: "Claude", native_title: null, manual_title: null,
    cwd: "/repo", ended_at_unix_ms: null, open_missed: 1, subtree_open_missed: 0,
    missed: [{ id: "missed-1", kind: "problem", text: "One missed check", status: "open", at_unix_ms: 1, turn_offset: 1, issue_repository: null, issue_number: null, creating: false }],
  };
  expect(endedWork([work], [row], null, null)).toHaveLength(0);
  expect(endedWork([work], [{ ...row, resolved: { at_unix_ms: 1, source: "operator" } }], null, null).flatMap(owner => owner.missed)).toHaveLength(1);
  expect(endedWork([work], [row], null, null)).toHaveLength(0);
  expect(endedWork([work], [{ ...row, device_id: "local" }], null, null)).toHaveLength(1);
  expect(recordTitle(work, false)).toBe("Claude");
});
