import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { english } from "../i18n/catalogs";
import { factoryCatalogs } from "../i18n/resources/factory";
import { itemSentence, stoppedText } from "./Decisions";
import { COLUMN_LABEL, CRITERION_LABEL, DECISION_KIND_LABEL, DECISION_SOURCE_LABEL, DIAGNOSIS_SOURCE_LABEL, DISCOVERY_LABEL, ENV_HOLD_LABEL, FOLLOW_UP_STATE_LABEL, GATE_LABEL, HOLDING_LABEL, KIND_LABEL, MODE_LABEL, ORIGIN_LABEL, OUTCOME_LABEL, PAUSE_REASON_LABEL, RECOVERY_ACTION_LABEL, RECOVERY_OUTCOME_LABEL, REFUSAL_LABEL, REFUSAL_REASONS, STAGE_LABEL, STATE_LABEL, STOP_LABEL, WAITING_LABEL, refusalText, waitingText } from "./labels";
import { type CardView, type FactoryView, type InboxItem, ATTEMPT_OUTCOMES, ATTEMPT_STAGES, COLUMNS, CRITERION_STATES, DECISION_BYS, DECISION_KINDS, DECISION_SOURCES, DIAGNOSIS_SOURCES, DISCOVERY_CLASSES, ENV_HOLDS, FOLLOW_UP_STATES, GATES, HOLDINGS, OBSERVER_MODES, PAUSE_REASONS, QUESTION_KINDS, QUESTION_ORIGINS, RECOVERY_ACTIONS, RECOVERY_OUTCOMES, RESULT_CODES, STOP_REASONS, TASK_STATES, WAITING_FOR } from "./model";

// The reading side of contracts/snapshot-wire-enums.json for the Factory
// sections: every value the core writes has a type member here and a label
// in the catalog, so a value the engine starts writing fails this test
// before a screen shows it unlabelled.
const CONTRACT: Record<string, string[]> = JSON.parse(fs.readFileSync(path.resolve(__dirname, "../../../contracts/snapshot-wire-enums.json"), "utf8"));

// `labels: null` is a value the screen reads but never words: a result code
// is told by the choice's own result, and who decided by the Task page's group.
const READERS: Record<string, { values: readonly string[]; labels: Record<string, string> | null }> = {
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
  factory_result_code: { values: RESULT_CODES, labels: null },
  factory_holding: { values: HOLDINGS, labels: HOLDING_LABEL },
  factory_decision_by: { values: DECISION_BYS, labels: { person: "factory.task.decisions.mine_one", ai: "factory.task.decisions.ai_one", worker: "factory.task.decisions.worker_one" } },
  factory_decision_source: { values: DECISION_SOURCES, labels: DECISION_SOURCE_LABEL },
  factory_follow_up_state: { values: FOLLOW_UP_STATES, labels: FOLLOW_UP_STATE_LABEL },
  factory_criterion_state: { values: CRITERION_STATES, labels: CRITERION_LABEL },
  factory_recovery_outcome: { values: RECOVERY_OUTCOMES, labels: RECOVERY_OUTCOME_LABEL },
  factory_recovery_action: { values: RECOVERY_ACTIONS, labels: RECOVERY_ACTION_LABEL },
  factory_decision_kind: { values: DECISION_KINDS, labels: DECISION_KIND_LABEL },
  factory_pause_reason: { values: PAUSE_REASONS, labels: PAUSE_REASON_LABEL },
  factory_observer_mode: { values: OBSERVER_MODES, labels: MODE_LABEL },
  factory_diagnosis_source: { values: DIAGNOSIS_SOURCES, labels: DIAGNOSIS_SOURCE_LABEL },
};

describe("the Factory wire enums", () => {
  it("reads every Factory enum the contract lists", () => {
    expect(Object.keys(CONTRACT).filter((key) => key.startsWith("factory_")).sort()).toEqual(Object.keys(READERS).sort());
  });

  for (const [key, reader] of Object.entries(READERS)) {
    it(`types and labels every ${key} value`, () => {
      expect([...reader.values].sort()).toEqual([...CONTRACT[key]!].sort());
      if (reader.labels) for (const value of CONTRACT[key]!) expect(english).toHaveProperty([reader.labels[value]!]);
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

  it("says a merge's gates, a stop's reason and a question's own words in one sentence (B22)", () => {
    const pr = { number: 561, url: "", head: "", by_factory: true, open: true };
    expect(itemSentence(item({ group: "merge", kind: "merge", question: null, gates: ["merge_refused"] }), null, { pr } as CardView, t)).toBe(t("factory.decide.mergeGated", { pr: "PR #561", gates: english["factory.gate.merge_refused"] }));
    expect(itemSentence(item({ group: "stopped", kind: "stopped", question: null, display_id: "#7", stop: "publish_refused" }), null, null, t)).toBe(t("factory.decide.stopped", { id: "#7", reason: english["factory.stop.publish_refused"] }));
    expect(itemSentence(item({ group: "todo", kind: "github", task: null, display_id: null }), { github_block: { forbidden: true, stage: "merge", since: 0 } } as FactoryView, null, t)).toBe(english["factory.decide.githubPermission"]);
    expect(itemSentence(item({}), null, null, t)).toBe("질문 원문");
  });

  it("says what an item holds up, from the asker's words or its code, with the Tasks waiting on it (B22)", () => {
    expect(stoppedText(item({ display_id: "#420", holding: "worker", stopped: null, unblocks: ["#421", "#422"] }), null, t)).toBe(t("factory.decide.waitingOn", { what: t("factory.holding.worker", { id: "#420" }), ids: "#421, #422" }));
    expect(stoppedText(item({ stopped: "#405 CI 읽기", holding: "github" }), null, t)).toBe("#405 CI 읽기");
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
