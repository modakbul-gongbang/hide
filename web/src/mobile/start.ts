// The phone's start sheet (PRD home-device-rail D-24, B42-B46): what it
// preselects from the catalog hided sends, why a start cannot leave yet, and
// the words for each refusal, named by catalog key. Pure, so every rule is
// tested without a socket.

import type { Notice, StartCatalog, StartKind, StartKindEntry, StartTarget } from "./protocol";

/** The longest instruction hided starts an agent with (hided/src/mobile/start.rs MAX_PROMPT_CHARS). */
export const MAX_START_CHARS = 4000;

export const START_KINDS: readonly { id: StartKind; label: string }[] = [
  { id: "claude", label: "Claude" },
  { id: "codex", label: "Codex" },
];

/** What the operator changed in the sheet; anything not changed follows the remembered choice. */
export type StartChoice = { target: string | null; kind: StartKind | null; model: string | undefined };

export const NO_CHOICE: StartChoice = { target: null, kind: null, model: undefined };

export type StartSelection = {
  target: StartTarget | null;
  kind: StartKind;
  /** The catalog id sent, or "" for the CLI's own default. */
  model: string;
  /** The catalog for `kind`; null until hided has sent one. */
  entry: StartKindEntry | null;
};

/** Why an instruction cannot be sent yet, checked before it leaves the phone. */
export function startProblem(text: string): "empty" | "too_long" | "control_characters" | null {
  if (text.trim().length === 0) return "empty";
  if ([...text].length > MAX_START_CHARS) return "too_long";
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u0008\u000b-\u001f\u007f-\u009f]/.test(text)) return "control_characters";
  return null;
}

/** The sheet's target label: the device, then the place on it. Both names are the Mac's data. */
export function targetText(target: StartTarget): string {
  return `${target.device_label} · ${target.label}`;
}

/** What the sheet shows selected: This Mac's Home until the operator picks, the remembered kind and model until then. */
export function selectionOf(catalog: StartCatalog | null, choice: StartChoice): StartSelection {
  const targets = catalog?.targets ?? [];
  const target = targets.find((candidate) => candidate.id === choice.target) ?? targets[0] ?? null;
  const kind = choice.kind ?? catalog?.remembered.kind ?? "claude";
  const entry = catalog?.kinds.find((candidate) => candidate.id === kind) ?? null;
  const wanted = choice.model ?? catalog?.remembered.models[kind] ?? "";
  // A remembered model the catalog does not list is still the one sent: the CLI decides, never a quiet swap (B32).
  return { target, kind, model: wanted, entry };
}

/** The model menu is disabled while the catalog has none; it then shows the remembered model or the default. */
export function modelsOf(selection: StartSelection): string[] {
  return selection.entry?.models ?? [];
}

/**
 * Whether a start that did not answer ok may still have started its agent:
 * then a resend keeps its request id and hided answers it once (B43). Any
 * other answer is final, and tapping Start again is a new start with a new id.
 */
export function mayHaveStarted(reason: string | null): boolean {
  return reason === "timeout" || reason === "in_flight" || reason === "offline";
}

/** The line inside the sheet for a refused or failed start. */
export function startFailure(reason: string | null): Notice {
  switch (reason) {
    case "in_flight":
      return { key: "mobile.start.inFlight" };
    case "timeout":
      return { key: "mobile.start.timeout" };
    case "offline":
      return { key: "mobile.start.offline" };
    case "empty":
      return { key: "mobile.start.empty" };
    case "too_long":
      return { key: "mobile.start.tooLong", limit: MAX_START_CHARS };
    case "control_characters":
    case "agent_start.invalid_prompt":
      return { key: "mobile.start.controlCharacters" };
    case "unknown_target":
    case "overview.unknown_checkout":
      return { key: "mobile.start.unknownTarget" };
    case "unknown_kind":
    case "agent_start.unknown_provider":
      return { key: "mobile.start.unknownKind" };
    case "unknown_model":
    case "agent_start.unknown_model":
      return { key: "mobile.start.unknownModel" };
    case "task_operation.busy":
      return { key: "mobile.start.busy" };
    case "home.conflict":
      return { key: "mobile.start.homeConflict" };
    case "revoked":
    case "mobile_off":
      return { key: "mobile.start.revoked" };
    case "unavailable":
      return { key: "mobile.start.unavailable" };
    case "agent_failed":
      return { key: "mobile.start.agentFailed" };
    default:
      return { key: "mobile.start.failed" };
  }
}
