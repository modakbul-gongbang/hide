// The stage-1 commands a Factory screen sends (docs/factory.md, The command
// contract). The core decodes them strictly and refuses any other verb, so a
// screen can only ask what a person may ask; the engine decides the rest.

export type VerificationChoice = { kind: "ci"; checks: string[] } | { kind: "commands"; commands: string[] } | { kind: "none" };

export type MergeMode = "auto" | "manual";

/** A Task as the engine resolves it on every Factory: `factory/T-3`. */
export function taskRef(factory: string, task: string): string {
  return `${factory}/${task}`;
}

export type FactoryCommand =
  | { verb: "init"; project: string; verification: VerificationChoice | null; merge_mode: MergeMode | null; confirm: boolean }
  | { verb: "answer"; task: string; question: string | null; choice: string | null; text: string | null; change?: boolean }
  | { verb: "priority"; task: string; priority: number }
  | { verb: "dep"; task: string; on: string; remove: true }
  | { verb: "pause"; task: string }
  | { verb: "resume"; task: string }
  | { verb: "retry"; task: string }
  | { verb: "pause_factory"; project: string | null }
  | { verb: "resume_factory"; project: string | null }
  | { verb: "ack_notices"; project: string | null }
  | { verb: "worker"; task: string; worker: number | null }
  | { verb: "merge"; task: string }
  | { verb: "request_changes"; task: string; comment: string }
  | { verb: "cancel"; task: string }
  | { verb: "revive"; task: string }
  | { verb: "config"; project: string | null; set: [string, string][] }
  | { verb: "close"; project: string | null }
  | { verb: "check"; project: string | null; at: "intake" | "after_done" | "periodic"; instruction: string };
