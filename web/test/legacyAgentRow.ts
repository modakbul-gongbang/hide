// Frozen pre-refactor fixture adapter (main 9f144877).
// Unit tests keep their old input vocabulary and independent assertions while
// exercising renderers of the new wire shape. Never import this in live code.
import type { AgentRow, AgentState, StatusTone } from "../src/snapshot";

type Input = Omit<Partial<AgentRow>, "state"> & { state?: AgentState };
export function legacyAgentRow(agent: Input): AgentRow {
  const asking = ["question", "approval", "error"].includes(agent.demand ?? "none");
  const needs = agent.group === "needs_you";
  const attention = needs || Boolean(agent.unread);
  const chip: StatusTone = {
    kind: agent.demand === "error" ? "error" : asking ? "warning" : agent.activity === "working" ? "working" : agent.activity === "stopped" && agent.emphasized ? "success" : "subtle",
    read: asking && !agent.emphasized,
  };
  const bucket = needs || agent.group === "done" ? "turn" : agent.waiting_on_descendants ? "delegating" : agent.group === "working" ? "working" : "resting";
  const counts = agent.descendant_counts;
  const waits = Boolean(agent.waiting_on_descendants || (counts && counts.working + counts.question + counts.approval + counts.error > 0));
  const working = agent.group === "working" || Boolean(agent.waiting_on_descendants);
  const text = agent.detail?.trim();
  const mode = asking ? "request" : agent.unread ? "news" : "quiet";
  const verb = agent.request?.verb ?? (agent.group === "working" ? "working" : "idle");
  const todo = ["answer", "fix", "review", "stopped", "result"].includes(verb);
  return { ...agent, state: {
    attention, needs_you: needs, root: !agent.delegated,
    title_emphasized: !(agent.delegated && !attention) && (attention || Boolean(agent.emphasized)),
    selection_emphasizes_title: !agent.delegated || attention,
    asking, working, waits_on_children: waits,
    chip_tone: chip, mark_tone: agent.waiting_on_descendants ? { kind: "working", read: false } : chip,
    line: text ? { text, mode, tone: mode === "request" ? chip : { kind: mode === "news" ? "news" : "subtle", read: false } } : null,
    branch_badge: agent.delegated ? agent.lineage_worktree_badge?.trim() || null : null,
    bucket,
    attention_rank: needs ? agent.demand === "error" ? 0 : 1 : agent.group === "done" ? 2 : agent.group === "working" ? 3 : 4,
    graph_rank: asking || bucket === "turn" ? 0 : bucket === "working" ? 1 : waits ? 2 : 3,
    edge: asking ? "ask" : agent.activity === "working" ? "flow" : waits ? "wait" : "rest",
    search_tone: chip.kind === "error" ? "failed" : chip.kind === "warning" ? "attention" : chip.kind === "working" ? "working" : chip.kind === "success" ? "done" : "muted",
    subtree: agent.requires_close_status_check ? "unknown" : agent.demand && agent.demand !== "none" ? "waiting" : agent.activity === "working" ? "working" : agent.unread && agent.symbol === "✓" ? "unread" : "quiet",
    link: asking ? "question" : working ? "working" : "idle",
    verb, request_todo: todo, request_since: todo ? agent.request?.verb_since_unix_ms ?? null : agent.changed_at_unix_ms ?? null,
  } } as AgentRow;
}
