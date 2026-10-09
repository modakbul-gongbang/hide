// What a 결정 필요 item offers and what each choice sends (PRD
// factory-human-loop D-33, B22, B23). A question offers its choices with what
// each leads to and a field for another answer; a to-do, a stop the recovery
// schedule gave up on and a Task whose pane was closed offer the engine's
// verbs or one button. The engine decides what an answer does; this only
// names the command a person's pick sends.

import { taskRef, type FactoryCommand } from "./commands";
import type { FactoryView, InboxItem } from "./model";

/** A choice in an item: a listed choice or verb with what it leads to, or the person's own words. */
export type Choice = { value: string; own: boolean; result: string | null };

/** The items whose choices are the engine's verbs rather than words it answers with. */
const VERB_ITEMS = new Set<InboxItem["kind"]>(["merge", "stopped", "paused"]);

/** A to-do, or a stop the schedule gave up on, has one thing to do: one button and no choices (B23). */
export function singleAction(item: InboxItem): boolean {
  return item.group === "todo" || item.kind === "stopped";
}

/** Whether the item takes words of the person's own: a question's other answer, or a merge's change request. */
export function takesOwnWords(item: InboxItem): boolean {
  return !singleAction(item) && item.kind !== "paused";
}

/** An item's choices in the engine's order, the suggestion first, each with the result its asker wrote. */
export function itemChoices(item: InboxItem): Choice[] {
  if (singleAction(item)) return [];
  const listed = [item.suggestion, ...item.choices, ...(item.default_action ? [item.default_action] : [])].filter((value) => value.trim() !== "");
  const unique = listed.filter((value, index) => listed.indexOf(value) === index);
  return unique.map((value) => ({ value, own: false, result: item.outcomes.find((outcome) => outcome.choice === value)?.result || null }));
}

/** The command a pick sends: a verb for a merge or paused item, else an answer to the item's question. */
export function choiceCommand(item: InboxItem, choice: Choice, text: string): FactoryCommand | null {
  if (item.task === null) return null;
  const task = taskRef(item.factory, item.task);
  if (VERB_ITEMS.has(item.kind)) {
    // A merge's own words are a change request to its worker.
    if (choice.own || choice.value === "request-changes") return text.trim() ? { verb: "request_changes", task, comment: text.trim() } : null;
    if (choice.value === "merge") return { verb: "merge", task };
    if (choice.value === "retry") return { verb: "retry", task };
    if (choice.value === "resume") return { verb: "resume", task };
    if (choice.value === "cancel") return { verb: "cancel", task };
    return null;
  }
  if (choice.own) return text.trim() ? { verb: "answer", task, question: item.question, choice: null, text: text.trim() } : null;
  return { verb: "answer", task, question: item.question, choice: choice.value === item.suggestion ? "suggestion" : choice.value, text: null };
}

/** The one button's command: a to-do's resolve, or starting a stopped Task again. */
export function singleCommand(item: InboxItem, view: FactoryView | null): FactoryCommand | null {
  if (item.resolve !== null) return { verb: "resolve", project: view?.project ?? null, item: item.resolve };
  if (item.kind === "stopped" && item.task !== null) return { verb: "retry", task: taskRef(item.factory, item.task) };
  return null;
}

/** An item's identity across summaries: a question by its id, a to-do by its button, a merge or stop by its Task. */
export function inboxKey(item: Pick<InboxItem, "factory" | "task" | "question" | "group"> & { resolve?: string | null }): string {
  return `${item.factory}/${item.task ?? "-"}/${item.question ?? item.resolve ?? item.group}`;
}
