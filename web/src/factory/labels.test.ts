import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { english } from "../i18n/catalogs";
import { factoryCatalogs } from "../i18n/resources/factory";
import { COLUMN_LABEL, DISCOVERY_LABEL, ENV_HOLD_LABEL, GATE_LABEL, KIND_LABEL, ORIGIN_LABEL, OUTCOME_LABEL, REFUSAL_LABEL, REFUSAL_REASONS, RESULT_LABEL, STAGE_LABEL, STATE_LABEL, STOP_LABEL, WAITING_LABEL, itemWhy, refusalText, resultText, waitingText } from "./labels";
import { type CardView, type InboxItem, ATTEMPT_OUTCOMES, ATTEMPT_STAGES, COLUMNS, DISCOVERY_CLASSES, ENV_HOLDS, GATES, QUESTION_KINDS, QUESTION_ORIGINS, RESULT_CODES, STOP_REASONS, TASK_STATES, WAITING_FOR } from "./model";

// The reading side of contracts/snapshot-wire-enums.json for the Factory
// sections: every value the core writes has a type member here and a label
// in the catalog, so a value the engine starts writing fails this test
// before a screen shows it unlabelled.
const CONTRACT: Record<string, string[]> = JSON.parse(fs.readFileSync(path.resolve(__dirname, "../../../contracts/snapshot-wire-enums.json"), "utf8"));

const READERS: Record<string, { values: readonly string[]; labels: Record<string, string> }> = {
  factory_task_state: { values: TASK_STATES, labels: STATE_LABEL },
  factory_column: { values: COLUMNS, labels: COLUMN_LABEL },
  factory_question_kind: { values: QUESTION_KINDS, labels: KIND_LABEL },
  factory_question_origin: { values: QUESTION_ORIGINS, labels: ORIGIN_LABEL },
  factory_discovery_class: { values: DISCOVERY_CLASSES, labels: DISCOVERY_LABEL },
  factory_attempt_stage: { values: ATTEMPT_STAGES, labels: STAGE_LABEL },
  factory_attempt_outcome: { values: ATTEMPT_OUTCOMES, labels: OUTCOME_LABEL },
  // `predecessors` and `environment` are said with their ids and hold reason.
  factory_waiting_for: { values: WAITING_FOR, labels: { ...WAITING_LABEL, predecessors: "factory.waiting.predecessors", environment: "factory.waiting.environment" } },
  factory_env_hold: { values: ENV_HOLDS, labels: ENV_HOLD_LABEL },
  factory_stop_reason: { values: STOP_REASONS, labels: STOP_LABEL },
  factory_gate: { values: GATES, labels: GATE_LABEL },
  factory_result_code: { values: RESULT_CODES, labels: RESULT_LABEL },
};

describe("the Factory wire enums", () => {
  it("reads every Factory enum the contract lists", () => {
    expect(Object.keys(CONTRACT).filter((key) => key.startsWith("factory_")).sort()).toEqual(Object.keys(READERS).sort());
  });

  for (const [key, reader] of Object.entries(READERS)) {
    it(`types and labels every ${key} value`, () => {
      expect([...reader.values].sort()).toEqual([...CONTRACT[key]!].sort());
      for (const value of CONTRACT[key]!) expect(english).toHaveProperty([reader.labels[value]!]);
    });
  }
});

/** Every reason a Rust source refuses a command with, read from `refuse("…"` and `Refusal::new("…"`. */
function refusalReasons(): string[] {
  const reasons = new Set<string>();
  for (const crate of ["hide-factory/src", "herdr-core/src"]) {
    const root = path.resolve(__dirname, "../../..", crate);
    for (const file of fs.readdirSync(root, { recursive: true, encoding: "utf8" })) {
      if (!file.endsWith(".rs")) continue;
      const text = fs.readFileSync(path.join(root, file), "utf8");
      for (const match of text.matchAll(/(?:\brefuse|Refusal::new)\(\s*"([a-z_]+)"/g)) reasons.add(match[1]!);
    }
  }
  return [...reasons].sort();
}

describe("the Factory refusals", () => {
  // The engine keeps no enum of its refusals, so the screen's list is held to
  // the sources: a refusal the engine starts answering with fails here.
  it("labels every reason the engine and the core refuse with", () => {
    const reasons = refusalReasons();
    expect(reasons.length).toBeGreaterThan(40);
    expect([...REFUSAL_REASONS].sort()).toEqual(reasons);
    for (const reason of REFUSAL_REASONS) expect(english).toHaveProperty([REFUSAL_LABEL[reason]]);
  });
});

describe("the Factory's code sentences", () => {
  const t = (key: string, options: Record<string, unknown> = {}) => (english as Record<string, string>)[key]!.replace(/\{\{(\w+)\}\}/g, (_, name: string) => String(options[name]));
  const item = (patch: Partial<InboxItem>) => ({ group: "answer", kind: "default", question: "q-1", text: "질문 원문", gates: [], stop: null, result_code: "wake_worker", unblocks: [], ...patch }) as InboxItem;

  it("says a merge's gates, a stop's reason and a question's own words (B9)", () => {
    expect(itemWhy(item({ group: "merge", kind: "merge", question: null, gates: ["merge_refused"] }), t)).toBe("Waiting to merge: Merge refused");
    expect(itemWhy(item({ group: "stopped", kind: "stopped", question: null, stop: "publish_refused" }), t)).toBe("Stopped: Push refused");
    expect(itemWhy(item({}), t)).toBe("질문 원문");
  });

  it("says what the suggestion does and what it frees (B9)", () => {
    expect(resultText(item({ unblocks: ["#421"] }), t)).toBe("Wakes the worker to continue; then #421 can start");
    expect(resultText(item({ result_code: "merge" }), t)).toBe("Merges");
  });

  it("says what a card waits for (B15)", () => {
    const card = (patch: Partial<CardView>) => ({ waiting_code: null, waiting_on: [], env_hold: null, ...patch }) as CardView;
    expect(waitingText(card({ waiting_code: "predecessors", waiting_on: ["#420", "T-3"] }), t)).toBe("Waiting for #420, T-3");
    expect(waitingText(card({ waiting_code: "environment", env_hold: "disk_floor" }), t)).toBe("Starts held: free disk below the floor");
    expect(waitingText(card({}), t)).toBeNull();
  });

  it("names a refusal it has no words for rather than hiding it (B10)", () => {
    expect(refusalText("lineage_unknown", t)).toBe(english["factory.refusal.lineage_unknown"]);
    expect(refusalText("a_new_reason", t)).toBe("The Factory did not take this (a_new_reason)");
  });
});

describe("Korean Factory strings", () => {
  it("never leave a particle to a fallback such as (으)로 or 을(를), which reads wrong after a value", () => {
    const fallback = /\((으|을|를|이|가|은|는|와|과)\)|[가-힣]\((을|를|이|가|은|는|와|과)\)/;
    const offending = Object.entries(factoryCatalogs.ko).filter(([, text]) => fallback.test(text));
    expect(offending).toEqual([]);
  });
});
