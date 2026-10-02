// The right half of ⌘K (PRD cmdk-navigation B15, B21): what the highlighted row
// is, said with the facts the snapshot carries and no others, and the
// relations it belongs to drawn the way the empty list draws them. A property
// with no value has no line (design principle 10); nothing here is computed
// that the core did not produce.

import { relationRows, relationsOf, type RelationTarget, type Relations } from "./relations";
import type { EntryStatus, SearchEntry } from "./search";
import { lastReadWords } from "./searchGithub";
import { searchDevices } from "./search";
import type { PullRequest, SnapshotRest } from "./snapshot";

export type Detail = {
  /** `Pull request`, `Agent · Claude`: the kind the title is of. */
  kind: string;
  title: string;
  pills: EntryStatus[];
  /** Plain pills: a device, a branch, a number. */
  tags: string[];
  facts: [string, string][];
  /** The relations of this row, when it has any beyond itself. */
  relations: Relations | null;
  /** What ↵ does. */
  action: string;
};

const REVIEW: Record<NonNullable<PullRequest["review"]>, string> = {
  approved: "승인됨",
  changes_requested: "변경 요청",
  review_required: "리뷰 필요",
};

function provider(kind: string | undefined): string {
  if (!kind) return "Agent";
  return `Agent · ${kind.charAt(0).toUpperCase()}${kind.slice(1)}`;
}

/** The relations target a row stands for: its own kind of thing, or null for one with no relations to draw. */
function targetOf(entry: SearchEntry): RelationTarget | null {
  if (entry.external) return null;
  if (entry.kind === "agent" && entry.paneId) return { kind: "agent", paneId: entry.paneId };
  if (entry.kind === "checkout" && entry.checkoutId) return { kind: "checkout", checkoutId: entry.checkoutId };
  if (entry.kind === "pr" && entry.workspaceId && entry.number !== undefined) return { kind: "pr", workspaceId: entry.workspaceId, number: entry.number };
  if (entry.kind === "issue" && entry.taskKey) return { kind: "issue", taskKey: entry.taskKey };
  return null;
}

function relationsFor(rest: SnapshotRest | null, entry: SearchEntry): Relations | null {
  const target = targetOf(entry);
  const relations = target ? relationsOf(rest, target) : null;
  // A relation of one row is no relation: the row itself is all it holds.
  return relations && relationRows(relations).length > 1 ? relations : null;
}

function present(facts: [string, string | null | undefined | false][]): [string, string][] {
  return facts.filter((row): row is [string, string] => typeof row[1] === "string" && row[1].length > 0);
}

/** The first line the agent last said, which is what its row's second line shows. */
function lastWords(entry: SearchEntry): string | null {
  const text = entry.agent?.detail || entry.agent?.message;
  return text ? (text.split("\n")[0] ?? null) : null;
}

export function detailOf(rest: SnapshotRest | null, entry: SearchEntry, now: number): Detail {
  const relations = relationsFor(rest, entry);
  const device = entry.chip?.label ?? searchDevices(rest ?? ({} as SnapshotRest)).find((scope) => scope.device.id === entry.deviceId)?.device.label;
  const base = { relations, tags: [] as string[], pills: [] as EntryStatus[] };
  switch (entry.kind) {
    case "agent": {
      return {
        ...base,
        kind: provider(entry.agentKind),
        title: entry.title,
        pills: entry.status ? [entry.status] : [],
        tags: device ? [device] : [],
        facts: present([["checkout", entry.place], ["마지막 말", lastWords(entry)]]),
        action: "에이전트로 이동",
      };
    }
    case "checkout": {
      // A Recent row of a device that is not connected: the names the record kept and nothing else, and ↵ does nothing, so no action is offered.
      if (entry.dimmed) return { ...base, kind: "Checkout", title: `${entry.subtitle} › ${entry.title}`, tags: [entry.title, device].filter((text): text is string => Boolean(text)), facts: [], action: "" };
      const checkout = entry.checkout;
      const files = checkout?.changed_file_count ?? 0;
      const ahead = checkout?.ahead ?? 0;
      return {
        ...base,
        kind: "Checkout",
        title: `${entry.workspace?.label ?? ""} › ${checkout?.label ?? entry.title}`,
        tags: [checkout?.branch, device].filter((text): text is string => Boolean(text)),
        facts: present([
          ["변경", [ahead > 0 ? `base 이후 커밋 ${ahead}` : null, files > 0 ? `변경 파일 ${files}` : null].filter(Boolean).join(" · ")],
          ["PR", checkout?.pull_request ? `#${checkout.pull_request.number}` : null],
          ["이슈", checkout?.task_key ? (entry.workspace?.tasks?.tasks.find((task) => task.key === checkout.task_key)?.id ?? null) : null],
        ]),
        action: "checkout 열기",
      };
    }
    case "pr": {
      if (entry.external) {
        return { ...base, kind: "Pull request · GitHub", title: entry.title.replace(/^#\d+ /, ""), pills: entry.status ? [entry.status] : [], tags: [`#${entry.number}`, entry.repository ?? ""].filter(Boolean), facts: [], action: "GitHub에서 열기" };
      }
      const pr = entry.pr;
      const closes = (pr?.closing_issues ?? []).map((issue) => `#${issue.number}`).join(", ");
      return {
        ...base,
        kind: "Pull request",
        title: pr?.title ?? entry.title,
        pills: [entry.status, pr?.is_draft ? { tone: "muted" as const, label: "Draft" } : null, entry.ci].filter((row): row is EntryStatus => Boolean(row)),
        tags: [`#${entry.number}`],
        facts: present([
          ["Review", pr?.review ? REVIEW[pr.review] : null],
          ["브랜치", pr?.head_branch],
          ["닫는 이슈", closes],
          ["읽음", entry.workspace ? lastReadWords(entry.workspace, now) : null],
        ]),
        action: "PRs 보기에서 열기",
      };
    }
    case "issue": {
      if (entry.external) {
        return { ...base, kind: "Issue · GitHub", title: entry.title.replace(/^#\d+ /, ""), pills: entry.status ? [entry.status] : [], tags: [`#${entry.number}`, entry.repository ?? ""].filter(Boolean), facts: [], action: "GitHub에서 열기" };
      }
      const owners = (entry.workspace?.checkouts ?? []).filter((checkout) => checkout.task_key === entry.taskKey);
      const closing = (entry.workspace?.checkouts ?? []).filter((checkout) => checkout.pull_request && (checkout.closes_task_keys ?? []).includes(entry.taskKey ?? ""));
      return {
        ...base,
        kind: "Issue",
        title: entry.task?.title ?? entry.title,
        pills: entry.status ? [entry.status] : [],
        tags: entry.task?.id ? [entry.task.id] : [],
        facts: present([
          ["프로젝트", entry.workspace?.label],
          ["맡은 곳", owners.map((checkout) => checkout.branch ?? checkout.label).join(", ")],
          ["닫는 PR", closing.map((checkout) => `#${checkout.pull_request?.number}`).join(", ")],
          ["읽음", entry.workspace ? lastReadWords(entry.workspace, now) : null],
        ]),
        action: "이슈 열기",
      };
    }
    case "project":
      return { ...base, kind: "Project", title: entry.title, tags: device ? [device] : [], facts: present([["경로", entry.workspace?.path]]), action: "프로젝트 열기" };
    case "device":
      return { ...base, kind: "Device", title: entry.title, facts: present([["종류", entry.subtitle]]), action: "기기로 이동" };
    case "command":
      return { ...base, kind: "Command", title: entry.title, facts: [], action: "시작 패널 열기" };
  }
}
