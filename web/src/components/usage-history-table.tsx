import { Fragment, useMemo, useState } from "react";
import { ChevronDownIcon, ChevronRightIcon } from "lucide-react";
import type { NodeUsage, UsageAmount } from "../generated/hided-ws";
import { AgentMark } from "../AgentMark";
import { agentProductName } from "../agentAdapters";
import { useInterfaceTranslation } from "../i18n/client";
import { requireInterfaceLanguage } from "../i18n/locale";
import { allTokens, compactTokens, selectedUsage, sumUsageDays, usageDate, usagePeriodDays } from "../usageHistory";
import { UsageCost, UsageStatus, UsageSummary } from "./usage-amount";
import { Button } from "./ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./ui/select";

const PAGE_DAYS = 30;
const HEADINGS = ["usage.history.dateAgent", "usage.history.costHeading", "usage.history.newTokens", "usage.history.inputHeading", "usage.history.outputHeading", "usage.history.cacheWriteHeading", "usage.history.cacheReadHeading", "usage.history.totalTokens"] as const;
const agentName = agentProductName;

export function UsageHistoryTable({ node }: { node: NodeUsage }) {
  const { t, i18n } = useInterfaceTranslation();
  const language = requireInterfaceLanguage(i18n.language);
  const [agent, setAgent] = useState("all");
  const [period, setPeriod] = useState("30");
  const [page, setPage] = useState(0);
  const [exact, setExact] = useState(false);
  const [folds, setFolds] = useState<Record<string, boolean>>({});
  const range = selectedUsage(node, "year", agent === "all" ? null : agent);
  const days = useMemo(() => {
    if (!range) return [];
    const available = Math.round((Date.parse(range.calendar_end) - Date.parse(range.calendar_start)) / 86_400_000) + 1;
    return usagePeriodDays(range, period === "year" ? available : Number(period)).filter((day) => day.amount.calls > 0).reverse();
  }, [range, period]);
  const total = useMemo(() => sumUsageDays(days), [days]);
  const byAgent = useMemo(() => node.agents.flatMap((row) => row.consumption.state === "reading" ? [{ agent: row.agent, days: new Map(row.consumption.value.year.days.map((day) => [day.date, day])) }] : []), [node.agents]);
  const pages = Math.max(1, Math.ceil(days.length / PAGE_DAYS));
  const current = Math.min(page, pages - 1);
  const shown = days.slice(current * PAGE_DAYS, (current + 1) * PAGE_DAYS);
  const numbers = new Intl.NumberFormat(language);
  const fmt = (value: number) => exact ? numbers.format(value) : compactTokens(value, language);
  const reset = () => { setPage(0); setFolds({}); };
  const cells = (amount: UsageAmount) => <><td className="usage-table-cost"><UsageCost amount={amount} /></td>{[amount.new_tokens, amount.tokens.input, amount.tokens.output, amount.tokens.cache_write, amount.tokens.cache_read, allTokens(amount)].map((value, index) => <td key={index} title={numbers.format(value)}>{fmt(value)}</td>)}</>;
  return <section className="usage-history usage-records flex min-w-0 flex-col gap-md" data-usage-history="year" data-surface="year">
    <div className="flex flex-wrap items-center justify-between gap-sm"><h2 className="text-subhead font-semibold">{t("usage.history.history")}</h2><div className="flex flex-wrap items-center gap-sm">
      <Select value={agent} onValueChange={(next) => { setAgent(next); reset(); }}><SelectTrigger size="sm" className="w-auto" aria-label={t("usage.history.agent")} data-usage-agent-select="true"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="all">{t("usage.history.allAgents")}</SelectItem>{byAgent.map((row) => <SelectItem key={row.agent} value={row.agent}><span className="inline-flex items-center gap-xs"><AgentMark kind={row.agent} />{agentName(row.agent)}</span></SelectItem>)}</SelectContent></Select>
      <Select value={period} onValueChange={(next) => { setPeriod(next); reset(); }}><SelectTrigger size="sm" className="w-auto" aria-label={t("usage.history.period")} data-usage-period-select="true"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="7">{t("usage.history.last7")}</SelectItem><SelectItem value="30">{t("usage.history.last30")}</SelectItem><SelectItem value="year">{t("usage.history.lastYear")}</SelectItem></SelectContent></Select>
      <Button variant={exact ? "secondary" : "ghost"} size="sm" aria-pressed={exact} onClick={() => setExact((value) => !value)} data-usage-exact="true">{t("usage.history.exact")}</Button>
    </div></div>
    <UsageSummary amount={total} exact={exact} />
    {range ? <UsageStatus range={range} /> : null}
    <div className="usage-table-scroll" tabIndex={0} role="region" aria-label={t("usage.history.history")}>
      <table className="usage-table" data-usage-table="true"><thead><tr>{HEADINGS.map((key) => <th key={key} scope="col">{t(key)}</th>)}</tr></thead><tbody>
        {shown.map((day) => {
          const expanded = folds[day.date] ?? day.today;
          const rows = byAgent.flatMap((row) => { const detail = row.days.get(day.date); return detail?.amount.calls && (agent === "all" || agent === row.agent) ? [{ agent: row.agent, amount: detail.amount }] : []; });
          return <Fragment key={day.date}><tr data-usage-date-row={day.date}><th scope="row"><button type="button" className="usage-date-toggle" aria-expanded={expanded} aria-label={t("usage.history.expandDay", { date: usageDate(day.date, language, { year: "numeric", month: "short", day: "numeric" }) })} onClick={() => setFolds((before) => ({ ...Object.fromEntries(shown.filter((item) => item.date in before).map((item) => [item.date, before[item.date]!])), [day.date]: !expanded }))}>{expanded ? <ChevronDownIcon /> : <ChevronRightIcon />}<span>{usageDate(day.date, language, { ...(period === "year" ? { year: "numeric" as const } : {}), month: "short", day: "numeric", weekday: "short" })}</span>{day.today ? <span className="text-micro text-muted-foreground">{t("usage.history.today")}</span> : null}</button></th>{cells(day.amount)}</tr>
            {expanded ? rows.map((row) => <tr key={row.agent} data-usage-agent-row={row.agent} className="usage-agent-row"><th scope="row"><span className="inline-flex items-center gap-xs"><AgentMark kind={row.agent} />{agentName(row.agent)}</span></th>{cells(row.amount)}</tr>) : null}
          </Fragment>;
        })}
        {!shown.length ? <tr><td colSpan={8} className="usage-empty">{t(range?.reading ? "usage.history.pending" : "usage.history.noMatches")}</td></tr> : null}
      </tbody>{shown.length ? <tfoot><tr><th scope="row">{t("usage.history.periodTotal")}</th>{cells(total)}</tr></tfoot> : null}</table>
    </div>
    <div className="flex flex-wrap items-center justify-between gap-sm text-micro text-muted-foreground"><span>{t("usage.history.apiEquivalent")}</span><div className="flex items-center gap-sm"><Button variant="ghost" size="sm" disabled={current === 0} onClick={() => { setPage(current - 1); setFolds({}); }}>{t("usage.history.previous")}</Button><span>{t("usage.history.page", { page: current + 1, pages })}</span><Button variant="ghost" size="sm" disabled={current === pages - 1} onClick={() => { setPage(current + 1); setFolds({}); }}>{t("usage.history.next")}</Button></div></div>
  </section>;
}
