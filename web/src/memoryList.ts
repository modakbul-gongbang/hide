import { formatRelativeTime } from "./i18n/format";
import type { InterfaceLanguage } from "./i18n/locale";
import type { MemoryRow, ProjectMemory } from "./snapshot";

/** How many cards the list shows before `그 외 N` (design v7). */
export const MEMORY_LIST_HEAD = 5;

/** A card's short name, as agents and the CLI write it. */
export function cardName(number: number): string {
  return `m${number}`;
}

/**
 * The list under `카드`: the search's answer in its order while one runs,
 * else every card a question does not already show. A question's card is
 * drawn once, in `물어볼 것`.
 */
export function memoryCards(project: ProjectMemory): MemoryRow[] {
  const asked = new Set(project.questions.map((question) => question.number));
  if (project.search) {
    const byNumber = new Map(project.rows.map((row) => [row.number, row]));
    return project.search.numbers.flatMap((number) => byNumber.get(number) ?? []);
  }
  return project.rows.filter((row) => !asked.has(row.number) && (row.number === project.expanded?.number || !["running", "unconfirmed", "pr_open"].includes(row.promotion?.state ?? "")));
}

/**
 * The cards on screen and how many are folded under `그 외 N`: the first
 * few, plus the card shown in full and every card on its way into the
 * repository wherever they sit, so neither a card opened from a
 * conversation nor one just promoted is hidden; a search shows every match.
 */
export function visibleCards(cards: MemoryRow[], expanded: number | null, all: boolean): { shown: MemoryRow[]; folded: number } {
  if (all || cards.length <= MEMORY_LIST_HEAD) return { shown: cards, folded: 0 };
  const moving = (row: MemoryRow) => row.promotion !== null && row.promotion.state !== "merged";
  const shown = cards.filter((row, index) => index < MEMORY_LIST_HEAD || row.number === expanded || moving(row));
  return { shown, folded: cards.length - shown.length };
}

/** A row's time: minutes or hours ago today, yesterday, then the date. */
export function memoryTime(unixMs: number, language: InterfaceLanguage, now = Date.now()): string {
  const elapsed = Math.max(0, now - unixMs);
  if (elapsed < 60 * 60_000) return formatRelativeTime(language, -Math.floor(elapsed / 60_000), "minute");
  const then = new Date(unixMs);
  const today = new Date(now);
  const days = Math.round((startOfDay(today) - startOfDay(then)) / 86_400_000);
  if (days === 0) return formatRelativeTime(language, -Math.floor(elapsed / 3_600_000), "hour");
  if (days === 1) return formatRelativeTime(language, -1, "day");
  const day = `${pad(then.getMonth() + 1)}-${pad(then.getDate())}`;
  return then.getFullYear() === today.getFullYear() ? day : `${then.getFullYear()}-${day}`;
}

/** A source's time: the clock today, yesterday with the clock, then the date with the clock. */
export function sourceTime(unixMs: number, language: InterfaceLanguage, now = Date.now()): string {
  const then = new Date(unixMs);
  const today = new Date(now);
  const clock = `${pad(then.getHours())}:${pad(then.getMinutes())}`;
  const days = Math.round((startOfDay(today) - startOfDay(then)) / 86_400_000);
  if (days === 0) return clock;
  if (days === 1) return `${formatRelativeTime(language, -1, "day")} ${clock}`;
  const day = `${pad(then.getMonth() + 1)}-${pad(then.getDate())}`;
  return `${then.getFullYear() === today.getFullYear() ? day : `${then.getFullYear()}-${day}`} ${clock}`;
}

function startOfDay(date: Date): number {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
}

function pad(value: number): string {
  return String(value).padStart(2, "0");
}

/** PR grouping is derived only from confirmed promotion records. */
export function memoryPullRequests(project: ProjectMemory): { number: number; cards: number[] }[] {
  const groups = new Map<number, number[]>();
  for (const row of project.rows) {
    if (row.promotion?.state !== "pr_open" || row.promotion.pr_number == null) continue;
    const cards = groups.get(row.promotion.pr_number) ?? [];
    cards.push(row.number);
    groups.set(row.promotion.pr_number, cards);
  }
  return [...groups].map(([number, cards]) => ({ number, cards: cards.sort((a, b) => a - b) }));
}

/** The existing durable lifecycle, including attempts started before upgrade. */
export function memoryInProgress(project: ProjectMemory): MemoryRow[] {
  return project.rows.filter((row) => row.promotion?.state === "running" || row.promotion?.state === "unconfirmed");
}

export function selectableMemoryQuestions(project: ProjectMemory): Set<number> {
  return new Set(project.questions.filter((question) => question.kind !== "docs_disagree" || question.document.authority === "card_newer").map((question) => question.number));
}
