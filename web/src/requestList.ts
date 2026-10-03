// The request view (PRD overview-request-view) as pure functions over the
// snapshot: which rows it draws, in which group and order, the `요청` tile,
// and the words each row's lines say. The verb is the core's (D-07); this
// file only groups, orders and shortens. `RequestView.tsx` draws it.

import type { DeviceAvailability } from "./navigation";
import type { LensAgent, Tile } from "./overviewLens";
import type { AgentPullRequest, AgentRow, RequestSender, RequestVerb, Task, Workspace } from "./snapshot";
import { parseToken, type LinkTarget } from "./terminalLinks";

// --- groups ------------------------------------------------------------------

/** The verbs that are the operator's to do, the groups the tile counts (D-06). */
export const TODO_VERBS: readonly RequestVerb[] = ["answer", "fix", "review", "stopped", "result"];

export const VERB_LABEL: Record<RequestVerb, string> = {
  answer: "답할 것",
  fix: "고칠 것",
  review: "리뷰·머지",
  stopped: "멈춤",
  result: "결과 볼 것",
  working: "일하는 중",
  waiting: "기다리는 중",
  idle: "쉬는 중",
};

const VERB_ORDER: readonly RequestVerb[] = ["answer", "fix", "review", "stopped", "result", "working", "waiting", "idle"];

/** One row of the view: an agent with its verb, and the live descendants it speaks for (D-30). */
export type RequestRow = {
  lens: LensAgent;
  verb: RequestVerb;
  /** The descendants folded into this row, nearest first; empty for most rows. */
  children: AgentRow[];
};

export type RequestGroup = { verb: RequestVerb; label: string; rows: RequestRow[] };

/** A row the core has not laid a block on yet reads from its group: working while it works, else resting. */
function verbOf(agent: AgentRow): RequestVerb {
  return agent.request?.verb ?? (agent.group === "working" ? "working" : "idle");
}

/**
 * The rows the view draws: one per agent (D-01, D-46), except a delegated
 * child whose parent is in the same scope, which its parent's row speaks for
 * (D-30). A child whose parent is gone, or outside the scope, is a row of its
 * own. `all` is every agent of the scope's devices, for the descendants.
 */
export function requestRows(agents: readonly LensAgent[], all: readonly AgentRow[]): RequestRow[] {
  const inScope = new Set(agents.map((value) => value.agent.pane_id));
  const byPane = new Map(all.map((agent) => [agent.pane_id, agent]));
  const rows: RequestRow[] = [];
  for (const lens of agents) {
    const { agent } = lens;
    if (agent.delegated && agent.lineage_parent_pane_id && inScope.has(agent.lineage_parent_pane_id)) continue;
    // Deepest first from the core; the expanded row reads nearest first.
    const children = (agent.close_descendant_pane_ids ?? [])
      .map((pane) => byPane.get(pane))
      .filter((child): child is AgentRow => child !== undefined)
      .reverse();
    rows.push({ lens, verb: verbOf(agent), children });
  }
  return rows;
}

/**
 * The groups in the order D-06 gives, empty ones left out. A to-do group
 * puts the row that has waited longest first (D-40); the others put the most
 * recent activity first.
 */
export function requestGroups(rows: readonly RequestRow[]): RequestGroup[] {
  const since = (row: RequestRow) => row.lens.agent.request?.verb_since_unix_ms ?? Number.MAX_SAFE_INTEGER;
  const activity = (row: RequestRow) => row.lens.agent.last_activity ?? "";
  return VERB_ORDER.map((verb) => {
    const members = rows.filter((row) => row.verb === verb);
    members.sort(TODO_VERBS.includes(verb) ? (a, b) => since(a) - since(b) : (a, b) => activity(b).localeCompare(activity(a)));
    return { verb, label: VERB_LABEL[verb], rows: members };
  }).filter((group) => group.rows.length > 0);
}

/** When a row's time counts from: the verb's start in a to-do group, else the last activity (D-40, B50). */
export function rowSince(row: RequestRow): number | null {
  if (TODO_VERBS.includes(row.verb)) return row.lens.agent.request?.verb_since_unix_ms ?? null;
  return row.lens.agent.changed_at_unix_ms;
}

/**
 * The `요청` tile (B2): how many rows are the operator's to do as the big
 * number, the ones to answer as the yellow badge, and a bar of the to-do
 * verbs. Zero is drawn as zero; a device that has not answered has no count.
 */
export function requestsTile(rows: readonly RequestRow[], availability: DeviceAvailability): Tile {
  const known = availability.state === "ready";
  const count = (verb: RequestVerb) => rows.filter((row) => row.verb === verb).length;
  const todo = TODO_VERBS.reduce((sum, verb) => sum + count(verb), 0);
  const answer = count("answer");
  return {
    id: "requests",
    label: "요청",
    value: known ? todo : null,
    unit: null,
    badge: known && answer > 0 ? { count: answer, label: VERB_LABEL.answer, parts: [] } : null,
    bar: known ? TODO_VERBS.map((verb) => ({ key: verb, label: VERB_LABEL[verb], count: count(verb) })) : null,
    failure: availability.state === "unavailable" ? `에이전트를 읽지 못함 · ${availability.text}` : null,
  };
}

// --- the request line (D-42) ---------------------------------------------------

/** Longer names keep their first twelve characters and their extension. */
const NAME_MAX = 24;
const NAME_HEAD = 12;
/** The share of the line the end of a long request keeps. */
export const TAIL_SHARE = 0.4;
/** Lines of the full request the expanded row shows before `전부 보기`. */
export const FULL_LINES = 20;

/** Where Claude Code leaves an attached image in the prompt's text. */
const IMAGE_MARK = /\[Image #\d+\]/gu;

function shortened(name: string): string {
  const chars = [...name];
  if (chars.length <= NAME_MAX) return name;
  const dot = name.lastIndexOf(".");
  const extension = dot > 0 && name.length - dot <= 8 ? name.slice(dot) : "";
  return `${chars.slice(0, NAME_HEAD).join("")}…${extension}`;
}

/** A URL or a path by its last name: `#N` for a GitHub pull request or issue, else the last part of its path. */
function shortTarget(target: LinkTarget): string {
  if (target.kind === "url") {
    const url = new URL(target.url);
    const github = /^\/[^/]+\/[^/]+\/(?:pull|issues)\/(\d+)/u.exec(url.pathname);
    if (url.hostname === "github.com" && github) return `#${github[1]}`;
    const parts = url.pathname.split("/").filter(Boolean);
    return shortened(decodeURIComponent(parts.at(-1) ?? url.hostname));
  }
  const parts = target.path.split("/").filter(Boolean);
  return shortened(parts.at(-1) ?? target.path);
}

/**
 * Whether a path-shaped word is a path for the line's purpose: rooted, or
 * deep, or a file name. `PR/이슈` in a sentence is a pair of words, not a
 * place, and keeps both.
 */
function plainlyPath(path: string): boolean {
  if (/^(?:\/|~\/|\.\.?\/)/u.test(path)) return true;
  if ((path.match(/\//gu)?.length ?? 0) >= 2) return true;
  return /\.[\p{L}\d]{1,8}$/u.test(path.split("/").at(-1) ?? "");
}

function shortToken(token: string): string {
  const parsed = parseToken(token);
  if (!parsed || (parsed.target.kind === "path" && !plainlyPath(parsed.target.path))) return token;
  return `${token.slice(0, parsed.lead)}${shortTarget(parsed.target)}${token.slice(token.length - parsed.trail)}`;
}

/**
 * A request as the folded row's one line (D-42, B52): its lines joined by
 * ` · ` with blank lines and runs of spaces gone, each URL and path by its
 * last name (so a home folder never shows), and the attached images as
 * `이미지 N` at the end; an image alone is that mark alone.
 */
export function requestLine(text: string, images: number): string {
  const lines = text
    .replace(IMAGE_MARK, " ")
    .split(/\r?\n/u)
    .map((line) => line.split(/\s+/u).filter(Boolean).map(shortToken).join(" "))
    .filter((line) => line.length > 0);
  if (images > 0) lines.push(`이미지 ${images}`);
  return lines.join(" · ");
}

/**
 * Where a line that does not fit is cut (D-42): the end keeps the longest run
 * of whole words that `fitsTail` accepts (its share of the width), and the
 * front is the rest, which the row ellipsizes in the width left. A line that
 * fits whole, or whose last word alone is too wide, is all front.
 */
export function splitTail(line: string, fits: (text: string) => boolean, fitsTail: (text: string) => boolean): { head: string; tail: string } {
  if (fits(line)) return { head: line, tail: "" };
  const words = line.split(" ");
  let tail = "";
  for (let index = words.length - 1; index > 0; index -= 1) {
    const candidate = words.slice(index).join(" ");
    if (!fitsTail(candidate)) break;
    tail = candidate;
  }
  if (tail === "") return { head: line, tail: "" };
  return { head: line.slice(0, line.length - tail.length).trimEnd(), tail };
}

/** `나 ›`, `<보낸 이> ›`, `에이전트 ›` (B26). */
export function senderWords(sender: RequestSender): string {
  return sender.kind === "operator" ? "나" : sender.kind === "named" ? sender.name : "에이전트";
}

// --- the result line -----------------------------------------------------------

function lastLine(text: string): string {
  const lines = text.split(/\r?\n/u).map((line) => line.trim()).filter(Boolean);
  return lines.at(-1) ?? "";
}

/**
 * The row's third line (D-12, B5, B10): the agent's label line when it has
 * one, else its own words, all of them on one line while it works and the
 * last line once it has finished.
 */
export function resultLine(row: RequestRow): string {
  const { agent } = row.lens;
  const label = agent.detail?.trim();
  if (label) return label;
  const reply = agent.request?.reply?.text ?? "";
  if (row.verb === "working") return reply.split(/\s+/u).filter(Boolean).join(" ");
  return lastLine(reply);
}

/** The row's name for assistive technology (B8): title, agent kind, verb, result line. */
export function requestAccessibleName(row: RequestRow, result: string): string {
  return [row.lens.agent.identity_label, row.lens.agent.agent_kind, VERB_LABEL[row.verb], result].filter(Boolean).join(", ");
}

// --- descendants (D-30, B13) -----------------------------------------------------

/** `자식 N · 일하는 중 M` and the warning `질문 K`, or null with no live descendant. */
export function childrenSummary(row: RequestRow): { text: string; asking: number } | null {
  const total = row.children.length;
  if (total === 0) return null;
  const counts = row.lens.agent.descendant_counts;
  const working = counts?.working ?? 0;
  const asking = (counts?.question ?? 0) + (counts?.approval ?? 0);
  return { text: [`자식 ${total}`, working > 0 ? `일하는 중 ${working}` : null].filter(Boolean).join(" · "), asking };
}

// --- pull requests and issues (D-43, D-46, D-47) ---------------------------------

/** The row's chip and how many other live pull requests its `+N` counts; the core put the chip first. */
export function pullRequestChip(pulls: readonly AgentPullRequest[]): { chip: AgentPullRequest; more: number } | null {
  const live = pulls.filter((pull) => pull.live);
  const chip = live[0];
  return chip ? { chip, more: live.length - 1 } : null;
}

/**
 * The row's issues (D-46, D-47): those the chip's pull request closes, then
 * the checkout's own, then those the other pull requests close, each once
 * and only when the project's source has it, so the chip draws what the
 * Issues board draws.
 */
export function rowIssues(row: RequestRow, project: Workspace): Task[] {
  const tasks = new Map((project.tasks?.tasks ?? []).map((task) => [task.key, task]));
  const pulls = row.lens.agent.request?.pull_requests ?? [];
  const chip = pullRequestChip(pulls)?.chip;
  const keys: string[] = [];
  const closing = (pull: AgentPullRequest) => pull.closing_issues.map((reference) => `github:${reference.repository}#${reference.number}`);
  if (chip) keys.push(...closing(chip));
  if (row.lens.task) keys.push(row.lens.task.key);
  for (const pull of pulls) if (pull !== chip) keys.push(...closing(pull));
  const seen = new Set<string>();
  const issues: Task[] = [];
  for (const key of keys) {
    if (seen.has(key)) continue;
    seen.add(key);
    const task = key === row.lens.task?.key ? row.lens.task : tasks.get(key);
    if (task) issues.push(task);
  }
  return issues;
}

// --- open targets (D-39) -----------------------------------------------------------

/** How many open chips the folded row shows; the rest are in the expanded row. */
export const OPEN_CHIPS = 3;

/** Something in the agent's last words that can be opened, by its short name. */
export type OpenCandidate = { key: string; label: string; target: LinkTarget };

/**
 * The URLs and path-shaped words of the agent's last words, in order and
 * each once, without the row's own pull requests (B49). A path becomes a
 * chip only once the host says it exists; a device's row offers URLs only,
 * since its paths are not this Mac's.
 */
export function openCandidates(reply: string, exclude: readonly string[], paths: boolean): OpenCandidate[] {
  const skip = new Set(exclude);
  const seen = new Set<string>();
  const result: OpenCandidate[] = [];
  for (const word of reply.split(/\s+/u)) {
    // Markdown wraps links in brackets and backticks the parser does not strip.
    const token = word.replace(/^[`*_[(<]+|[`*_\])>]+$/gu, "");
    const parsed = token ? parseToken(token) : null;
    if (!parsed) continue;
    const { target } = parsed;
    if (target.kind === "path" && !paths) continue;
    const key = target.kind === "url" ? target.url : target.path;
    if (seen.has(key) || skip.has(key)) continue;
    seen.add(key);
    result.push({ key, label: shortTarget(target), target });
  }
  return result;
}

/** The first lines of the full request, and whether more follow (B6). */
export function fullRequest(text: string, all: boolean): { text: string; more: boolean } {
  const lines = text.split(/\r?\n/u);
  if (all || lines.length <= FULL_LINES) return { text, more: false };
  return { text: lines.slice(0, FULL_LINES).join("\n"), more: true };
}
