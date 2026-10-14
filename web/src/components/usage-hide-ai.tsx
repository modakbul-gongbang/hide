// Hide's own background AI inside the Usage history: the one popover line under
// the day's summary and the names the full table uses for its rows.
import type { UsageDay } from "../generated/hided-ws";
import { HideMark } from "../AgentMark";
import { useInterfaceTranslation } from "../i18n/client";
import { requireInterfaceLanguage } from "../i18n/locale";
import { compactTokens, hideAiReading, hideAiShare } from "../usageHistory";

/** The closed feature groups the core counts under; one it does not know is Other. */
export const HIDE_AI_FEATURES = {
  labels: "usage.hideAi.feature.labels",
  memory: "usage.hideAi.feature.memory",
  factory: "usage.hideAi.feature.factory",
  issue: "usage.hideAi.feature.issue",
  other: "usage.hideAi.feature.other",
} as const;

/** `≥ 1.2M *` for a known amount that unknown calls make a lower bound, the plain number otherwise. */
export function BoundedTokens({ text, bounded }: { text: string; bounded: boolean }) {
  return <>{bounded ? `≥ ${text}` : text}{bounded ? <span className="text-muted-foreground"> *</span> : null}</>;
}

/** `Of this, Hide AI` and what it used on the day the summary above shows, as tokens and a share. */
export function HideAiLine({ day, since }: { day: UsageDay; since: string | null }) {
  const { t, i18n } = useInterfaceTranslation();
  const language = requireInterfaceLanguage(i18n.language);
  if (day.pending) return null;
  const reading = hideAiReading(day, since);
  const counted = reading.state === "counted";
  const tokens = counted ? compactTokens(reading.amount.new_tokens, language) : "";
  const share = counted ? hideAiShare(reading.basisPoints, language) : "";
  const bounded = counted && reading.unknown > 0;
  const label = counted ? t(bounded ? "usage.hideAi.lineLowerBound" : "usage.hideAi.lineLabel", { tokens, share }) : undefined;
  return <div className="usage-hide-line" data-usage-hide-ai={reading.state} role="group" aria-label={label}>
    <span className="inline-flex items-center gap-xs"><HideMark className="size-(--size-icon)" /><span>{t(counted ? "usage.hideAi.line" : "usage.hideAi.name")}</span></span>
    {counted
      ? <span className="usage-hide-numbers"><span data-usage-hide-tokens="true"><BoundedTokens text={tokens} bounded={bounded} /></span><span className="text-muted-foreground">{share}</span></span>
      : <span className="text-muted-foreground">{t(reading.state === "before" ? "usage.hideAi.before" : "usage.hideAi.none")}</span>}
  </div>;
}
