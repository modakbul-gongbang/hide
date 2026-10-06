import { Status, type Tone } from "../components/settings-rows";
import { providerReason, resetTime } from "../hideAi";
import { useInterfaceTranslation } from "../i18n/client";
import { requireInterfaceLanguage } from "../i18n/locale";
import type { AiProvider, AiRefusal } from "../snapshot";

/** How the Hide AI tab words an agent's state and a refusal; one place so the Runs on row, the fallback rows and the Add menu agree. */
export function useHideAiWords() {
  const { t, i18n } = useInterfaceTranslation();
  const language = requireInterfaceLanguage(i18n.language);
  const time = (unixMs: number) => resetTime(language, unixMs);
  /** One short state: "Signed in", "Out of usage until 3:10 PM". */
  const state = (provider: AiProvider): { tone: Tone; text: string } => {
    switch (providerReason(provider)) {
      case "ready":
        // An agent with no sign-in check is ready only because its program was found, so it does not claim to be signed in.
        return provider.login_checked === false ? { tone: "muted", text: t("hideAi.state.loginUnchecked") } : { tone: "ok", text: t("hideAi.state.ready") };
      case "needs_login":
        return { tone: "warn", text: t("hideAi.state.needsLogin") };
      case "usage_limited":
        return {
          tone: "warn",
          text: provider.retry_at_ms ? t("hideAi.state.usageLimitedUntil", { time: time(provider.retry_at_ms) }) : t("hideAi.state.usageLimited"),
        };
      case "not_installed":
        return { tone: "warn", text: t("hideAi.state.notInstalled") };
      case "read_only":
        return { tone: "warn", text: t("hideAi.state.readOnly") };
      case "unsupported":
        return { tone: "warn", text: t("hideAi.state.unsupported") };
      case "unread":
        return { tone: "pending", text: t("hideAi.state.unread") };
      default:
        return { tone: "warn", text: t("hideAi.state.unavailable") };
    }
  };
  /** Why Runs on is not answering, as a sentence about the agent: "Claude Code is out of usage until 3:10 PM". */
  const refusal = (agent: string, why: AiRefusal): string => {
    switch (why.reason) {
      case "usage_limited":
        return why.retry_at_ms
          ? t("hideAi.reason.usageLimitedUntil", { agent, time: time(why.retry_at_ms) })
          : t("hideAi.reason.usageLimited", { agent });
      case "needs_login":
        return t("hideAi.reason.needsLogin", { agent });
      case "not_installed":
        return t("hideAi.reason.notInstalled", { agent });
      case "unsupported":
        return t("hideAi.reason.unsupported", { agent });
      default:
        return t("hideAi.reason.unavailable", { agent });
    }
  };
  return { state, refusal };
}

/** An agent's state as symbol plus words, never colour alone (design 7). */
export function ProviderState({ provider }: { provider: AiProvider }) {
  const { state } = useHideAiWords();
  const { tone, text } = state(provider);
  return (
    <Status tone={tone} data-ai-state={`${provider.id}:${provider.state}`}>
      {text}
    </Status>
  );
}
