import { formatRelativeTime } from "./i18n/format";
import type { InterfaceLanguage } from "./i18n/locale";
import type { AgentRow, MemoryRow, ProjectMemory, PullRequest } from "./snapshot";
import { githubPullRequestIdentity, sameGithubPullRequest } from "./pullRequestIdentity";
import type { MessageKey } from "./i18n/catalogs";

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
  return project.rows.filter((row) => !asked.has(row.number) && !["running", "unconfirmed", "pr_open"].includes(row.promotion?.state ?? ""));
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

export type MemoryProgress = { key: string; pr: number | null; url: string | null; cards: [MemoryRow, ...MemoryRow[]] };

/** A confirmed PR or a single ledger-bound agent attempt, never a guessed branch/name. */
export function memoryProgress(project: ProjectMemory): MemoryProgress[] {
  const groups = new Map<string, MemoryProgress>();
  for (const row of project.rows) {
    const promotion = row.promotion;
    if (!promotion || !["running", "unconfirmed", "pr_open"].includes(promotion.state)) continue;
    const pr = promotion.state === "pr_open" ? promotion.pr_number : null;
    const identity = pr !== null ? githubPullRequestIdentity(promotion.pr_url) : null;
    const url = identity !== null ? promotion.pr_url! : null;
    const key = identity !== null ? `pr:${identity}` : promotion.state === "running" && promotion.agent?.pane_id ? `agent:${promotion.agent.pane_id}:${promotion.started_at_unix_ms ?? row.number}` : `card:${row.number}`;
    const group = groups.get(key);
    if (group) group.cards.push(row);
    else groups.set(key, { key, pr, url, cards: [row] });
  }
  return [...groups.values()].map((group) => ({ ...group, cards: group.cards.sort((a, b) => a.number - b.number) }));
}

/** After PR creation only a Hide AI promotion origin and its live, repository-qualified PR prove the agent. */
export function memoryProgressAgent(group: MemoryProgress, pull: PullRequest | undefined, agents: AgentRow[]): AgentRow | undefined {
  if (group.pr === null) {
    const pane = group.cards[0].promotion?.agent?.pane_id;
    return pane ? agents.find((agent) => agent.pane_id === pane) : undefined;
  }
  if (!pull?.url || !sameGithubPullRequest(pull.url, group.url)) return undefined;
  return agents.find((agent) => {
    const task = agent.hide_ai?.task;
    return task?.kind === "memory_promote"
      && agent.request?.pull_requests.some((row) => row.number === pull.number && sameGithubPullRequest(row.url, pull.url) && row.live && (row.created || row.duty));
  });
}

export function memoryProgressStatus(group: MemoryProgress, pull: PullRequest | undefined, agent: AgentRow | undefined): { key: MessageKey; tone: string; agentLink: boolean } {
  if (group.pr !== null && !group.url) return { key: "memory.unconfirmed", tone: "text-muted-foreground", agentLink: false };
  pull = sameGithubPullRequest(pull?.url, group.url) ? pull : undefined;
  if (pull?.state === "merged" || pull?.state === "closed") return { key: pull.state === "merged" ? "agentSessions.pr.merged" : "agentSessions.pr.closed", tone: "text-muted-foreground", agentLink: false };
  if (pull?.checks === "failed") return { key: "agentSessions.pr.failed", tone: "text-destructive", agentLink: true };
  if (pull?.review === "changes_requested") return { key: "board.review.changes", tone: "text-warning", agentLink: true };
  if (agent?.state.asking) return { key: "memory.progress.asking", tone: "text-warning", agentLink: true };
  if (group.cards[0].promotion?.state === "unconfirmed") return { key: "memory.unconfirmed", tone: "text-muted-foreground", agentLink: false };
  if (group.cards[0].promotion?.state === "running") return { key: "memory.progress.writing", tone: "text-muted-foreground", agentLink: true };
  if (pull?.checks === "pending") return { key: "card.checkPending", tone: "text-muted-foreground", agentLink: false };
  if (pull?.is_draft) return { key: "agentSessions.pr.draft", tone: "text-muted-foreground", agentLink: false };
  if (pull?.review === "approved") return { key: "board.review.approved", tone: "text-muted-foreground", agentLink: false };
  return { key: pull ? "memory.prOpen" : "memory.progress.inPr", tone: "text-muted-foreground", agentLink: false };
}

export function selectableMemoryQuestions(project: ProjectMemory): Set<number> {
  return new Set(project.questions.filter((question) => question.kind !== "docs_disagree" || question.document.authority === "card_newer").map((question) => question.number));
}
