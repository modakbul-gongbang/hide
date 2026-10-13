import type { UsageAmount, UsageDay, UsageRange, NodeUsage } from "./generated/hided-ws";
import type { InterfaceLanguage } from "./i18n/locale";

/** Native provider ids are provenance, never inferred from a model name. */
export function usageProvider(provider: string): { account: string } | { provider: string } | null {
  if (provider === "openai-codex") return { account: "Codex" };
  if (provider === "claude-code") return { account: "Claude Code" };
  const names: Record<string, string> = { anthropic: "Anthropic", openai: "OpenAI", google: "Google", "google-vertex": "Google Vertex AI", "google-gemini-cli": "Google Gemini CLI", xai: "xAI", groq: "Groq", mistral: "Mistral", openrouter: "OpenRouter", "amazon-bedrock": "Amazon Bedrock", azure: "Azure", deepseek: "DeepSeek", cerebras: "Cerebras", together: "Together AI" };
  return names[provider] ? { provider: names[provider] } : null;
}

/** Format a core civil date without moving it into the browser's timezone. */
export function usageDate(date: string, language: InterfaceLanguage, options: Intl.DateTimeFormatOptions = { month: "short", day: "numeric" }): string {
  return new Intl.DateTimeFormat(language, { ...options, timeZone: "UTC" }).format(new Date(`${date}T12:00:00Z`));
}
export function compactTokens(value: number, language: InterfaceLanguage): string {
  return new Intl.NumberFormat(language, { notation: "compact", maximumFractionDigits: 1 }).format(value);
}
export function dollarAmount(value: UsageAmount, language: InterfaceLanguage): string {
  return new Intl.NumberFormat(language, { style: "currency", currency: "USD", currencyDisplay: "narrowSymbol", maximumFractionDigits: 2 }).format(value.cost.micro_usd / 1_000_000);
}
export function selectedUsage(node: NodeUsage, surface: "popover" | "year", agent: string | null): UsageRange | null {
  if (!agent) return node.history?.[surface].range ?? null;
  const cell = node.agents.find((row) => row.agent === agent)?.consumption;
  return cell?.state === "reading" ? cell.value[surface] : null;
}

/** Fill display cells only; recorded amounts, levels and range totals stay core-owned. */
export function usageDays(range: UsageRange): UsageDay[] {
  const start = Date.parse(`${range.calendar_start}T00:00:00Z`);
  const end = Date.parse(`${range.calendar_end}T00:00:00Z`);
  const count = (end - start) / 86_400_000 + 1;
  if (!Number.isInteger(count) || count < 1 || count > 371) throw new Error("Invalid usage calendar bounds");
  const recorded = new Map(range.days.map((day) => [day.date, day]));
  return Array.from({ length: count }, (_, index) => {
    const civil = new Date(start + index * 86_400_000);
    const date = civil.toISOString().slice(0, 10);
    return recorded.get(date) ?? {
      date, week: Math.floor(index / 7), weekday: civil.getUTCDay(),
      today: index === count - 1, recent_week: index >= count - 7,
      level: 0, pending: range.reading,
      amount: {
        calls: 0, new_tokens: 0,
        tokens: { input: 0, output: 0, cache_read: 0, cache_write: 0, cache_write_1h: 0, reasoning: null },
        cost: { micro_usd: 0, partial: false, waiting: false, stale: false, incomplete: false, unpriced_models: [], other_unpriced_models: 0 },
      },
      agents: [], top_agents: [], other_agents: 0,
    };
  });
}
