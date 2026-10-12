// What the screen says about the rows and checkouts Hide AI started (PRD
// hide-ai-origin-mark D-01 to D-09). The core decides who started what, from
// the coordination record of the spawn; this module only words it and joins
// it to the checkouts a project draws. Nothing here reads a branch name.
import type { TFunction } from "i18next";
import type { MessageKey } from "./i18n/catalogs";
import type { Checkout, HideAi, HideAiCheckout, HideAiTask, Workspace } from "./snapshot";

/** The task kinds the catalogs name; a kind of another build shows as its code (LOCALIZATION.md). */
const TASK_KEYS: Record<string, MessageKey> = {
  memory_promote: "hideAi.task.memory_promote",
};

/** `Memory 올리기 · m72`: the work and what it is about; null when the core named no task (B10). */
export function taskLine(t: TFunction<"translation">, task: HideAiTask | undefined): string | null {
  if (!task) return null;
  const key = TASK_KEYS[task.kind];
  const kind = key ? t(key) : task.kind;
  return task.target ? `${kind} · ${task.target}` : kind;
}

/** What a started thing is called on the screen: its task line, or `Hide AI` alone while the core names no task (B10). */
export function originLine(t: TFunction<"translation">, hideAi: HideAi): string {
  return taskLine(t, hideAi.task) ?? t("hideAi.name");
}

/** Hide AI's part of a row's accessible name (D-09): the name and the task, so a screen reader hears who started it. */
export function hideAiWords(t: TFunction<"translation">, hideAi: HideAi | undefined): string | null {
  if (!hideAi) return null;
  const task = taskLine(t, hideAi.task);
  return task ? `${t("hideAi.name")} · ${task}` : t("hideAi.name");
}

/** The place line of a row Hide AI started: the project and the task in the branch's place (`herdr-ide › Memory 올리기 · m72`). */
export function hideAiPlace(t: TFunction<"translation">, project: string, hideAi: HideAi): string {
  return `${project} › ${originLine(t, hideAi)}`;
}

/** The task Hide AI started this checkout for, by the device and path the core lists; absent for an operator's checkout. */
export function hideAiCheckoutOf(listed: readonly HideAiCheckout[] | undefined, workspace: Pick<Workspace, "device_id">, checkout: Pick<Checkout, "path">): HideAiCheckout | null {
  return listed?.find((row) => row.device_id === workspace.device_id && row.path === checkout.path) ?? null;
}

/**
 * A project's drawn checkouts split by who made them (D-03, D-06): the
 * operator's stay where they were, and Hide AI's gather behind them. The
 * group exists only while it has a checkout (D-08).
 */
export function splitByOrigin(listed: readonly HideAiCheckout[] | undefined, workspace: Pick<Workspace, "device_id">, checkouts: readonly Checkout[]): { operator: Checkout[]; hideAi: Checkout[] } {
  const operator: Checkout[] = [];
  const hideAi: Checkout[] = [];
  for (const checkout of checkouts) (hideAiCheckoutOf(listed, workspace, checkout) ? hideAi : operator).push(checkout);
  return { operator, hideAi };
}
