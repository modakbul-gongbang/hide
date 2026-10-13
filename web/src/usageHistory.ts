import type { UsageAmount, UsageRange, NodeUsage } from "./generated/hided-ws";
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
