import type { UsageAmount, UsageRange } from "../generated/hided-ws";
import { useInterfaceTranslation } from "../i18n/client";
import { requireInterfaceLanguage } from "../i18n/locale";
import { compactTokens, dollarAmount } from "../usageHistory";
import { Hint } from "./ui/tooltip";

export function UsageCost({ amount }: { amount: UsageAmount }) {
  const { t, i18n } = useInterfaceTranslation();
  const language = requireInterfaceLanguage(i18n.language);
  if (!amount.calls) return <span>{t("usage.history.noData")}</span>;
  const cost = amount.cost;
  const unavailable = !cost.micro_usd && (cost.waiting || cost.partial);
  const value = unavailable ? t(cost.waiting ? "usage.history.priceLoading" : "usage.history.unpriced") : `${cost.partial || cost.waiting || cost.incomplete ? "≥" : "≈"} ${dollarAmount(amount, language)}`;
  const reasons = [
    t("usage.history.apiEquivalent"),
    ...cost.unpriced_models.map((model) => t("usage.history.noPrice", { model })),
    cost.other_unpriced_models ? t("usage.history.otherPrices") : null,
    cost.incomplete ? t("usage.history.incomplete") : null,
    cost.stale ? t("usage.history.stalePrice") : null,
    cost.waiting ? t("usage.history.waitingPrice") : null,
  ].filter(Boolean).join(" · ");
  return <Hint label={`${value} · ${reasons}`} contentClassName="[overflow-wrap:anywhere]"><span tabIndex={0} data-usage-cost="true" data-cost-unavailable={unavailable} className="usage-cost">{value}{cost.stale || cost.incomplete || cost.partial && !unavailable ? <span className="text-micro text-muted-foreground"> *</span> : null}</span></Hint>;
}

export function UsageSummary({ amount, pending = false, exact = false, announce = false }: { amount: UsageAmount; pending?: boolean; exact?: boolean; announce?: boolean }) {
  const { t, i18n } = useInterfaceTranslation();
  const language = requireInterfaceLanguage(i18n.language);
  return <div className="usage-summary" aria-live={announce ? "polite" : undefined} aria-atomic={announce || undefined}>
    <div><div className="text-micro text-muted-foreground">{t("usage.history.costHeading")}</div><div className="usage-summary-value">{pending ? t("usage.history.pending") : <UsageCost amount={amount} />}</div></div>
    <div><div className="text-micro text-muted-foreground">{t("usage.history.newTokens")}</div><div className="usage-summary-value" data-usage-total="true">{pending ? t("usage.history.pending") : amount.calls ? exact ? new Intl.NumberFormat(language).format(amount.new_tokens) : compactTokens(amount.new_tokens, language) : t("usage.history.noData")}</div></div>
  </div>;
}

export function UsageStatus({ range }: { range: UsageRange }) {
  const { t } = useInterfaceTranslation();
  return <>
    {range.reading ? <div className="flex flex-col gap-xs text-micro text-muted-foreground" role="status"><span>{t("usage.history.reading", { done: range.files_done, total: range.files_total })} · {t("usage.history.soFar")}</span><progress className="h-xs w-full accent-primary" value={range.files_done} max={Math.max(1, range.files_total)} /></div> : null}
    {range.limited ? <span className="text-micro text-muted-foreground" data-usage-limited="true">{t("usage.history.limited")}</span> : null}
  </>;
}
