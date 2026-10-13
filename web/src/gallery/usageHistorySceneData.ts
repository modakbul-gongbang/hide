// Invented projection for visual states, not an accounting oracle. Reader,
// retention and arithmetic acceptance run against the real Rust worker.
import type { NodeUsage, UsageAmount, UsageContribution, UsageDay, UsageRange, UsageShare } from "../generated/hided-ws";

const NAMES = ["claude", "codex", "opencode", "pi", "omp", "grok"];
const SHARES = [6000, 2000, 1000, 500, 300, 200];
const amount = (n: number, state: string): UsageAmount => ({
  calls: n ? 1 : 0, new_tokens: n,
  tokens: { input: n * .3, output: n * .2, cache_write: n * .5, cache_read: n * 12, cache_write_1h: 0, reasoning: null },
  cost: { micro_usd: n * 2, partial: state === "unpriced", waiting: state === "offline", stale: state === "stale", incomplete: false, unpriced_models: state === "unpriced" ? ["model-with-no-public-price"] : [], other_unpriced_models: 0 },
});
export function ledgerScene(node: NodeUsage, state: string): NodeUsage {
  if (state === "empty") return { ...node, history: null, agents: node.agents.map((row) => ({ ...row, consumption: row.agent === "cursor" ? { state: "unsupported" } : { state: "unavailable", cause: "no_records", last_checked_at_ms: 1 } })) };
  const reading = state === "backfill";
  const days = state === "sparse" ? 9 : state === "year" ? 365 : 37;
  const first = Date.UTC(2026, 9, 12) - (days - 1) * 86400000;
  const offset = new Date(first).getUTCDay();
  const values: UsageDay[] = Array.from({ length: days + offset }, (_, index) => {
    const date = new Date(first + (index - offset) * 86400000).toISOString().slice(0, 10);
    const n = index < offset || index % 6 === 0 || (reading && index < 15) ? 0 : (index % 4 + 1) * 1_000_000;
    const agents: UsageContribution[] = n ? NAMES.map((agent, i) => ({ agent, new_tokens: n * SHARES[i]! / 10000, share_basis_points: SHARES[i]! })) : [];
    return { date, week: Math.floor(index / 7), weekday: index % 7, today: index === days + offset - 1, recent_week: index >= days + offset - 7, level: n ? index % 4 + 1 : 0, pending: reading && !n, amount: amount(n, state), agents, top_agents: agents.slice(0, 3), other_agents: n ? 3 : 0 };
  });
  const range = (selection: number | null, surface: "popover" | "year"): UsageRange => {
    const selected = values.slice(surface === "popover" ? Math.max(0, Math.floor((values.length - 134) / 7) * 7) : 0).map((day, index) => {
      const agents = selection === null ? day.agents : day.agents.filter((a) => a.agent === NAMES[selection]);
      const value = selection === null ? day.amount : amount(day.amount.new_tokens * SHARES[selection]! / 10000, state);
      return { ...day, week: Math.floor(index / 7), amount: value, agents, top_agents: agents.slice(0, 3), other_agents: Math.max(0, agents.length - 3) };
    });
    const total = selected.reduce((sum, day) => sum + day.amount.new_tokens, 0);
    const peak = selected.reduce((a, b) => a.amount.new_tokens > b.amount.new_tokens ? a : b);
    return { days: selected, amount: amount(total, state), since: selected.find((d) => d.amount.calls)?.date ?? null, active_days: selected.filter((d) => d.amount.calls).length, observed_days: selected.length, peak: { date: peak.date, new_tokens: peak.amount.new_tokens }, current_streak: 3, longest_streak: 5, reading, limited: state === "limited", files_done: 412, files_total: 1280 };
  };
  const shares: UsageShare[] = NAMES.map((agent, i) => ({ contribution: { agent, new_tokens: range(i, "year").amount.new_tokens, share_basis_points: SHARES[i]! }, since: values.find((d) => d.amount.calls)?.date ?? null, lifetime: range(i, "year").amount, providers: agent === "pi" ? ["openai-codex"] : agent === "omp" ? ["anthropic"] : [] }));
  return {
    ...node,
    history: { popover: { range: range(null, "popover"), shares }, year: { range: range(null, "year"), shares } },
    agents: node.agents.map((row) => {
      const index = NAMES.indexOf(row.agent);
      return index < 0 ? row : { ...row, consumption: { state: "reading", freshness: "fresh", observed_at_ms: 1, value: { popover: range(index, "popover"), year: range(index, "year"), retained: { since: shares[index]!.since, amount: shares[index]!.lifetime, providers: shares[index]!.providers } } } };
    }),
  };
}
