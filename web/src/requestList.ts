import type { AgentScope } from "./agentScope";
// The request view (PRD overview-request-view) as pure functions over the
// snapshot: which rows it draws, in which group and order, the Requests tile,
// and the words each row's lines say. The verb is the core's (D-07); this
// file only groups, orders and shortens. `RequestView.tsx` draws it.

import { parser as markdownParser } from "@lezer/markdown";
import type { TFunction } from "i18next";
import type { MessageKey } from "./i18n/catalogs";
import type { DeviceAvailability } from "./navigation";
import type { LensAgent, Tile } from "./overviewLens";
import type { AgentPullRequest, AgentRequest, AgentRow, LabelEnd, RequestSender, RequestVerb, Task, Workspace } from "./snapshot";
import { parseToken, type LinkTarget } from "./terminalLinks";

// --- groups ------------------------------------------------------------------

/** The verbs that are the operator's to do, the groups the tile counts (D-06). */
export const TODO_VERBS: readonly RequestVerb[] = ["answer", "fix", "review", "stopped", "result"];

export const VERB_LABEL: Record<RequestVerb, MessageKey> = {
  answer: "requests.verb.answer",
  fix: "requests.verb.fix",
  review: "requests.verb.review",
  stopped: "requests.verb.stopped",
  result: "requests.verb.result",
  working: "requests.verb.working",
  waiting: "requests.verb.waiting",
  idle: "requests.verb.idle",
};


/** One row of the view: an agent with its verb, and the live descendants it speaks for (D-30). */
export type RequestRow = {
  lens: LensAgent;
  verb: RequestVerb;
  /** The descendants folded into this row, nearest first; empty for most rows. */
  children: AgentRow[];
};

export type RequestGroup = { verb: RequestVerb; rows: RequestRow[] };

/**
 * The rows the view draws: one per agent (D-01, D-46), except a delegated
 * child whose parent is in the same scope, which its parent's row speaks for
 * (D-30). A child whose parent is gone, or outside the scope, is a row of its
 * own. `all` is every agent of the scope's devices, for the descendants.
 */
export function requestRows(agents: readonly LensAgent[], all: readonly AgentRow[], scope: AgentScope): RequestRow[] {
  const byPane = new Map(all.map((agent) => [agent.pane_id, agent]));
  return scope.requests.rows.map((row) => {
    const lens = agents[row.member];
    if (!lens) throw new Error("Request scope references a missing member");
    const children = row.children.map((id) => {
      const child = byPane.get(id);
      if (!child) throw new Error(`Request scope references a missing descendant: ${id}`);
      return child;
    });
    return { lens, verb: lens.agent.state.verb, children };
  });
}

/**
 * The groups in the order D-06 gives, empty ones left out. A to-do group
 * puts the row that has waited longest first (D-40); the others put the most
 * recent activity first.
 */
export function requestGroups(rows: readonly RequestRow[], scope: AgentScope): RequestGroup[] {
  return scope.requests.groups.map((group) => ({ verb: group.verb, rows: group.rows.map((index) => {
    const row = rows[index];
    if (!row) throw new Error("Request group references a missing row");
    return row;
  }) }));
}

/** When a row's time counts from: the verb's start in a to-do group, else the last activity (D-40, B50). */
export function rowSince(row: RequestRow): number | null {
  return row.lens.agent.state.request_since;
}

/**
 * The Requests tile (B2): how many rows are the operator's to do as the big
 * number, the ones to answer as the yellow badge, and a bar of the to-do
 * verbs. Zero is drawn as zero; a device that has not answered has no count.
 */
export function requestsTile(scope: AgentScope, availability: DeviceAvailability, t: TFunction<"translation">): Tile {
  const known = availability.state === "ready";
  const count = (verb: RequestVerb) => scope.requests.counts[verb];
  const todo = scope.requests.todo;
  const answer = scope.requests.answer;
  return {
    id: "requests",
    label: t("requests.title"),
    value: known ? todo : null,
    unit: null,
    badge: known && answer > 0 ? { count: answer, label: t(VERB_LABEL.answer), parts: [] } : null,
    bar: known ? TODO_VERBS.map((verb) => ({ key: verb, label: t(VERB_LABEL[verb]), count: count(verb) })) : null,
    failure: availability.state === "unavailable" ? t("requests.unavailable", { reason: availability.text }) : null,
  };
}

// --- the request line (D-42) ---------------------------------------------------

/** Longer names keep their first twelve characters and their extension. */
const NAME_MAX = 24;
const NAME_HEAD = 12;
/** The share of the line the end of a long request keeps. */
export const TAIL_SHARE = 0.4;
/** Lines of the full request the expanded row shows before `Show all`. */
export const FULL_LINES = 20;

/** Where Claude Code leaves an attached image in the prompt's text. */
const IMAGE_MARK = /\[Image #\d+\]/gu;

/** Characters that draw nothing but can reorder or disguise a name (`%E2%80%AE` turns `fdp.exe` around). */
const IGNORABLE = /[\p{Default_Ignorable_Code_Point}\p{Cc}]/gu;

function shortened(raw: string): string {
  const name = raw.replace(IGNORABLE, "");
  const chars = [...name];
  if (chars.length <= NAME_MAX) return name;
  const dot = name.lastIndexOf(".");
  const extension = dot > 0 && name.length - dot <= 8 ? name.slice(dot) : "";
  return `${chars.slice(0, NAME_HEAD).join("")}…${extension}`;
}

/** A percent-encoded name as written when it is not valid encoding (`50%`): text an agent wrote never throws. */
function decoded(name: string): string {
  try {
    return decodeURIComponent(name);
  } catch {
    return name;
  }
}

/** A URL or a path by its last name: `#N` for a GitHub pull request or issue, else the last part of its path. */
function shortTarget(target: LinkTarget): string {
  if (target.kind === "url") {
    let url: URL;
    try {
      url = new URL(target.url);
    } catch {
      return shortened(target.url);
    }
    const github = /^\/[^/]+\/[^/]+\/(?:pull|issues)\/(\d+)/u.exec(url.pathname);
    if (url.hostname === "github.com" && github) return `#${github[1]}`;
    const parts = url.pathname.split("/").filter(Boolean);
    return shortened(decoded(parts.at(-1) ?? url.hostname));
  }
  const parts = target.path.split("/").filter(Boolean);
  return shortened(parts.at(-1) ?? target.path);
}

/**
 * Whether a path-shaped word is a path for the line's purpose: rooted, or
 * deep, or a file name. `PR/issue` in a sentence is a pair of words, not a
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
 * `N images` at the end; an image alone is that mark alone.
 */
export function requestLine(text: string, images: number, t: TFunction<"translation">): string {
  const lines = text
    .replace(IMAGE_MARK, " ")
    .split(/\r?\n/u)
    .map((line) => line.split(/\s+/u).filter(Boolean).map(shortToken).join(" "))
    .filter((line) => line.length > 0);
  if (images > 0) lines.push(t("requests.images", { count: images }));
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

/** `Me ›`, `<sender> ›`, `Agent ›` (B26). */
export function senderWords(sender: RequestSender, t: TFunction<"translation">): string {
  return sender.kind === "operator" ? t("requests.sender.operator") : sender.kind === "named" ? sender.name : t("requests.sender.agent");
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
  const label = agent.request?.line?.trim();
  if (label) return label;
  const reply = agent.request?.reply?.text ?? "";
  if (row.verb === "working") return reply.split(/\s+/u).filter(Boolean).join(" ");
  return lastLine(reply);
}

const END_LABEL: Record<LabelEnd, MessageKey> = {
  working: "requests.end.working",
  question: "requests.end.question",
  done: "requests.end.done",
  waiting: "requests.end.waiting",
  unfinished: "requests.end.unfinished",
};

/**
 * The label's reading of the turn, shown in the expanded row (B6, D-28):
 * how the turn ended and the line it wrote. Null without a label, which is
 * also every row while summaries are off.
 */
export function verdictLine(block: AgentRequest | undefined, t: TFunction<"translation">): string | null {
  if (!block?.end) return null;
  const line = block.line?.trim();
  const end = t(END_LABEL[block.end]);
  return line ? t("requests.verdictLine", { end, line }) : t("requests.verdict", { end });
}

/** The row's name for assistive technology (B8): title, agent kind, verb, result line. */
export function requestAccessibleName(row: RequestRow, result: string, t: TFunction<"translation">): string {
  return [row.lens.agent.identity_label, row.lens.agent.agent_kind, t(VERB_LABEL[row.verb]), result].filter(Boolean).join(", ");
}

// --- descendants (D-30, B13) -----------------------------------------------------

/** `N descendants · Working M` and the warning `Questions K`, or null with no live descendant. */
export function childrenSummary(row: RequestRow, t: TFunction<"translation">): { text: string; asking: number } | null {
  const total = row.children.length;
  if (total === 0) return null;
  const counts = row.lens.agent.descendant_counts;
  const working = counts?.working ?? 0;
  const asking = (counts?.question ?? 0) + (counts?.approval ?? 0);
  return { text: [t("requests.children", { count: total }), working > 0 ? t("requests.workingChildren", { count: working }) : null].filter(Boolean).join(" · "), asking };
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

/** Folded chips keep open issues and issues closed after this request (D-47, D-43).
 * A later edit to an already closed issue does not make it current work.
 * `rowIssues` keeps the complete history for the expanded row (B56).
 */
export function rowIssueChips(row: RequestRow, project: Workspace): Task[] {
  const requested = row.lens.agent.request?.request?.at_unix_ms ?? 0;
  return rowIssues(row, project).filter((issue) => issue.open || (issue.closed_at_unix_ms != null && issue.closed_at_unix_ms > requested));
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
  const words: string[] = [];
  let from = 0;
  // Use the editor's existing Markdown grammar for balanced destinations
  // and labels containing spaces. Do not treat words in a link's label as
  // filesystem candidates. The existing validator still owns every target.
  markdownParser.parse(reply).iterate({ enter(node) {
    if (node.name !== "Link" && node.name !== "Image") return;
    const destination = node.node.getChild("URL");
    if (!destination) return;
    words.push(...reply.slice(from, node.from).split(/\s+/u), reply.slice(destination.from, destination.to));
    from = node.to;
    return false;
  } });
  words.push(...reply.slice(from).split(/\s+/u));
  for (const word of words) {
    // Markdown wraps links in brackets and backticks the parser does not strip.
    const token = word.replace(/^[`*_]+|[`*_]+$/gu, "");
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
