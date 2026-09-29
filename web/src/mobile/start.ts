// The phone's start sheet (PRD home-device-rail D-24, B42-B46): what it
// preselects from the catalog hided sends, why a start cannot leave yet, and
// the words for each refusal. Pure, so every rule is tested without a socket.

import type { StartCatalog, StartKind, StartKindEntry, StartTarget } from "./protocol";

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

/** The sheet's target label: the device, then the place on it. */
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
 * other answer is final, and tapping 시작 again is a new start with a new id.
 */
export function mayHaveStarted(reason: string | null): boolean {
  return reason === "timeout" || reason === "in_flight" || reason === "offline";
}

/** The line inside the sheet for a refused or failed start. */
export function startFailure(reason: string | null): string {
  switch (reason) {
    case "in_flight":
      return "같은 시작을 아직 처리하는 중이에요. 잠시 뒤 목록을 확인하세요.";
    case "timeout":
      return "맥이 시간 안에 답하지 않았어요. 목록에 에이전트가 생겼는지 확인하고, 없으면 다시 시작하세요.";
    case "offline":
      return "맥의 hide에 닿지 않아 시작하지 못했어요. 연결되면 다시 시작하세요.";
    case "empty":
      return "할 일을 적어 주세요.";
    case "too_long":
      return `할 일은 ${MAX_START_CHARS.toLocaleString("ko-KR")}자까지 적을 수 있어요.`;
    case "control_characters":
    case "agent_start.invalid_prompt":
      return "할 일에 보낼 수 없는 문자가 있어요.";
    case "unknown_target":
    case "overview.unknown_checkout":
      return "이 대상은 더 이상 목록에 없어요. 다른 대상을 고르세요.";
    case "unknown_kind":
    case "agent_start.unknown_provider":
      return "이 종류는 시작할 수 없어요. 다른 종류를 고르세요.";
    case "unknown_model":
    case "agent_start.unknown_model":
      return "이 모델은 쓸 수 없어요. 다른 모델을 고르세요.";
    case "task_operation.busy":
      return "다른 시작이 진행 중이에요. 잠시 뒤 다시 시작하세요.";
    case "home.conflict":
      return "이 기기의 ~/hide 폴더가 hide의 것이 아니에요. 데스크톱에서 확인하세요.";
    case "revoked":
    case "mobile_off":
      return "이 폰의 연결이 끊겼어요. 맥에서 다시 연결하세요.";
    case "unavailable":
      return "맥의 hide가 시작 요청을 받지 못했어요. 다시 시작하세요.";
    case "agent_failed":
      return "에이전트를 띄웠지만 실행하지 못했어요. 데스크톱에서 확인하세요.";
    default:
      return "시작하지 못했어요. 다시 시작하세요.";
  }
}
