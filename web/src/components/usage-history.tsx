// Core-prepared values only: the shell formats dates and draws levels/shares.
import { useMemo, useState, type CSSProperties, type KeyboardEvent } from "react";
import { ArrowUpRightIcon } from "lucide-react";
import type { NodeUsage, UsageAmount, UsageContribution, UsageDay, UsageRange, UsageShare } from "../generated/hided-ws";
import { AgentMark } from "../AgentMark";
import { agentAdapter } from "../agentAdapters";
import { useInterfaceTranslation } from "../i18n/client";
import { requireInterfaceLanguage } from "../i18n/locale";
import { compactTokens, dollarAmount, selectedUsage, usageDate } from "../usageHistory";
import { cn } from "../lib/utils";
import { Button } from "./ui/button";
import { Hint } from "./ui/tooltip";
import "./usage-history.css";

export type UsageOutline = "five_hour" | "weekly" | null;
const agentName = (name: string) => agentAdapter(name)?.label ?? name;

export function UsageCost({ amount }: { amount: UsageAmount }) {
  const { t, i18n } = useInterfaceTranslation();
  const language = requireInterfaceLanguage(i18n.language);
  if (!amount.calls) return <span>{t("usage.history.noData")}</span>;
  const cost = amount.cost;
  const reasons = [
    ...cost.unpriced_models.map((model) => t("usage.history.noPrice", { model })),
    cost.other_unpriced_models ? t("usage.history.otherPrices", { count: cost.other_unpriced_models }) : null,
    cost.incomplete ? t("usage.history.incomplete") : null,
    cost.stale ? t("usage.history.stalePrice") : null,
    cost.waiting ? t("usage.history.waitingPrice") : null,
  ].filter(Boolean);
  return <span data-usage-cost="true">{cost.waiting && !cost.micro_usd ? null : t("usage.history.cost", { value: `${cost.partial || cost.waiting ? "≥" : "≈"} ${dollarAmount(amount, language)}` })}{reasons.length ? `${cost.waiting && !cost.micro_usd ? "" : " · "}${reasons.join(" · ")}` : null}</span>;
}

function DayDetail({ day }: { day: UsageDay }) {
  const { t, i18n } = useInterfaceTranslation();
  const language = requireInterfaceLanguage(i18n.language);
  const fmt = (n: number) => compactTokens(n, language);
  return <div className="usage-detail-copy text-caption text-muted-foreground">
    <div className="flex flex-wrap justify-between gap-xs text-foreground"><span>{usageDate(day.date, language, { weekday: "short", month: "short", day: "numeric" })}</span><strong className="font-mono">{day.pending ? t("usage.history.pending") : day.amount.calls ? fmt(day.amount.new_tokens) : t("usage.history.noData")}</strong></div>
    <div>{[t("usage.history.input", { value: fmt(day.amount.tokens.input) }), t("usage.history.output", { value: fmt(day.amount.tokens.output) }), t("usage.history.cacheWrite", { value: fmt(day.amount.tokens.cache_write) })].join(" · ")}</div>
    <div>{t("usage.history.cacheRead", { value: fmt(day.amount.tokens.cache_read) })}</div>
    <div><UsageCost amount={day.amount} /></div>
    <div className="flex flex-wrap items-center gap-sm">{day.top_agents.map((agent) => <span key={agent.agent} className="inline-flex items-center gap-xxs"><AgentMark kind={agent.agent} />{fmt(agent.new_tokens)}</span>)}{day.other_agents ? <span>+{day.other_agents}</span> : null}</div>
  </div>;
}

function ShareDetail({ share }: { share: UsageShare }) {
  const { t, i18n } = useInterfaceTranslation();
  const language = requireInterfaceLanguage(i18n.language);
  return <div className="usage-detail-copy text-caption"><span>{agentName(share.contribution.agent)}{share.since ? ` · ${t("usage.history.since", { date: usageDate(share.since, language) })}` : null}</span><span className="text-muted-foreground"><UsageCost amount={share.lifetime} /></span></div>;
}

function Calendar({ range, outline, onDay }: { range: UsageRange; outline: UsageOutline; onDay: (date: string | null) => void }) {
  const { t, i18n } = useInterfaceTranslation();
  const language = requireInterfaceLanguage(i18n.language);
  const months = range.days.filter((day, index, days) => day.weekday === 0 && (index === 0 || day.date.slice(0, 7) !== days[Math.max(0, index - 7)]?.date.slice(0, 7)));
  const key = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const offsets: Record<string, number> = { ArrowUp: -1, ArrowDown: 1, ArrowLeft: -7, ArrowRight: 7 };
    const next = event.key === "Home" ? 0 : event.key === "End" ? range.days.length - 1 : offsets[event.key] !== undefined ? index + (offsets[event.key] ?? 0) : null;
    if (next === null) return;
    event.preventDefault();
    const date = range.days[Math.max(0, Math.min(next, range.days.length - 1))]?.date;
    event.currentTarget.parentElement?.querySelector<HTMLButtonElement>(`[data-usage-day="${date}"]`)?.focus();
  };
  return <div className="usage-calendar text-micro text-muted-foreground" role="group" aria-label={t("usage.history.calendar")} onMouseLeave={() => onDay(null)}>
    <div className="usage-calendar-labels" aria-hidden="true">{[1, 3, 5].map((day) => <span key={day} style={{ gridRow: day + 2 }}>{usageDate(`2026-01-0${day + 4}`, language, { weekday: "short" })}</span>)}</div>
    <div className="usage-calendar-days" style={{ "--usage-weeks": Math.max(1, (range.days.at(-1)?.week ?? 0) + 1) } as CSSProperties}>
      {months.map((day) => <span className="usage-calendar-month" key={day.date} style={{ gridRow: 1, gridColumn: day.week + 1 }}>{usageDate(day.date, language, { month: "short" })}</span>)}
      {range.days.map((day, index) => <button type="button" key={day.date} className="usage-cell" data-usage-day={day.date} data-level={day.level} data-pending={day.pending} data-today={day.today} data-outline={outline === "weekly" ? day.recent_week : outline === "five_hour" ? day.today : false}
        tabIndex={day.today ? 0 : -1} style={{ gridColumn: day.week + 1, gridRow: day.weekday + 2 }}
        aria-label={day.pending ? `${usageDate(day.date, language)} · ${t("usage.history.pending")}` : t("usage.history.cell", { date: usageDate(day.date, language), value: compactTokens(day.amount.new_tokens, language) })}
        onMouseEnter={() => onDay(day.date)} onFocus={() => onDay(day.date)} onBlur={() => onDay(null)} onKeyDown={(event) => key(event, index)} />)}
    </div>
  </div>;
}

function Legend() {
  const { t } = useInterfaceTranslation();
  return <span className="inline-flex items-center gap-xxs text-micro text-muted-foreground" aria-hidden="true"><span>{t("usage.history.less")}</span>{[0, 1, 2, 3, 4].map((level) => <span key={level} className="usage-cell" data-level={level} />)}<span>{t("usage.history.more")}</span></span>;
}

function AgentShares({ shares, selected, hovered, onSelect, onHover, bars, day }: { shares: UsageShare[]; selected: string | null; hovered: string | null; onSelect: (agent: string) => void; onHover: (agent: string | null) => void; bars: boolean; day?: UsageDay | null }) {
  const { t, i18n } = useInterfaceTranslation();
  const language = requireInterfaceLanguage(i18n.language);
  const contributions: UsageContribution[] = day ? day.agents : shares.map((share) => share.contribution);
  return <div className={cn("flex gap-xs", bars ? "flex-col" : "flex-wrap")} data-usage-shares="true" onMouseLeave={() => onHover(null)}>
    {contributions.map((row) => <button key={row.agent} type="button" aria-label={t("usage.history.filter", { agent: agentName(row.agent) })} aria-pressed={selected === row.agent}
      className={cn("rounded-xs p-xs text-caption outline-none hover:bg-secondary focus-visible:ring-1 focus-visible:ring-ring", selected === row.agent && "bg-secondary", bars ? "usage-agent-bar" : "inline-flex items-center gap-xxs")}
      data-usage-filter={row.agent} onClick={() => onSelect(row.agent)} onMouseEnter={() => onHover(row.agent)} onFocus={() => onHover(row.agent)} onBlur={() => onHover(null)}>
      <span className="inline-flex min-w-0 items-center gap-xs"><AgentMark kind={row.agent} />{bars ? <span className="break-words text-left">{agentName(row.agent)}</span> : null}</span>
      {bars ? <span aria-hidden="true" className="h-xs overflow-hidden rounded-full bg-secondary"><span className="block h-full rounded-full bg-primary" style={{ width: `${row.share_basis_points / 100}%` }} /></span> : null}
      <span className="font-mono text-micro">{compactTokens(row.new_tokens, language)}</span>
      {bars && !day && hovered === row.agent ? <span className="col-span-3 text-left text-micro text-muted-foreground"><UsageCost amount={shares.find((share) => share.contribution.agent === row.agent)!.lifetime} /></span> : null}
    </button>)}
  </div>;
}

export function UsageHistory({ node, surface, outline, onHistory }: { node: NodeUsage; surface: "popover" | "year"; outline: UsageOutline; onHistory?: () => void }) {
  const { t, i18n } = useInterfaceTranslation();
  const language = requireInterfaceLanguage(i18n.language);
  const [selected, setSelected] = useState<string | null>(null);
  const [date, setDate] = useState<string | null>(null);
  const [agent, setAgent] = useState<string | null>(null);
  const range = selectedUsage(node, surface, selected);
  const history = node.history?.[surface];
  // All possible day details occupy one grid area invisibly, so the tallest
  // real localized content sets its height before any pointer enters a cell.
  // This is bounded by the core's 20-week range and exists only while open.
  const reservation = useMemo(() => surface === "popover" ? range?.days.map((day) => <div key={day.date} className="usage-detail-reserve" aria-hidden="true" inert><DayDetail day={day} /></div>) : null, [range, surface]);
  if (!range || !history) return null;
  const day = range.days.find((value) => value.date === date);
  const share = history.shares.find((value) => value.contribution.agent === agent);
  const toggle = (value: string) => { setSelected((before) => before === value ? null : value); setDate(null); };
  return <section className="usage-history flex min-w-0 flex-col gap-sm" data-usage-history={surface} data-surface={surface}>
    <div className="flex flex-wrap items-baseline justify-between gap-xs">
      <h3 className="text-subhead font-semibold">{t("usage.history.newTokens")}</h3>
      <div className="text-caption text-muted-foreground"><span data-usage-total="true">{range.amount.calls ? compactTokens(range.amount.new_tokens, language) : t("usage.history.noData")}</span>{range.reading ? ` · ${t("usage.history.soFar")}` : null} · {t("usage.history.active", { active: range.active_days, days: range.observed_days })}</div>
    </div>
    {range.reading ? <div className="flex flex-col gap-xs text-caption text-muted-foreground" role="status"><span>{t("usage.history.reading", { done: range.files_done, total: range.files_total })}</span><progress className="h-xs w-full accent-primary" value={range.files_done} max={Math.max(1, range.files_total)} /></div> : null}
    {range.limited ? <span className="text-micro text-muted-foreground" data-usage-limited="true">{t("usage.history.limited")}</span> : null}
    <Calendar range={range} outline={outline} onDay={(next) => { setDate(next); if (next) setAgent(null); }} />
    <div className="flex flex-wrap items-center justify-between gap-xs">{onHistory ? <Button variant="ghost" size="sm" onClick={onHistory} data-usage-history-link="true"><ArrowUpRightIcon />{t("usage.history.history")}</Button> : <Hint label={t("usage.history.longestStreak", { days: range.longest_streak })}><span className="text-caption text-muted-foreground">{t("usage.history.streak", { days: range.current_streak })}</span></Hint>}<Legend /></div>
    {surface === "year" ? <div className="text-caption text-muted-foreground">{t("usage.history.peak")}{range.peak ? ` · ${usageDate(range.peak.date, language)} · ${compactTokens(range.peak.new_tokens, language)}` : ` · ${t("usage.history.noData")}`}</div> : null}
    <div className={cn(surface === "popover" ? "usage-detail-slot" : "flex flex-col gap-sm rounded-md border border-border p-md")} data-usage-detail="true">
      {reservation}
      {surface === "popover" ? history.shares.map((item) => <div key={item.contribution.agent} className="usage-detail-reserve" aria-hidden="true" inert><ShareDetail share={item} /><AgentShares shares={history.shares} selected={selected} hovered={null} onSelect={() => {}} onHover={() => {}} bars={false} /></div>) : null}
      <div className="usage-detail-copy">
        {day ? <DayDetail day={day} /> : share && surface === "popover" ? <ShareDetail share={share} /> : surface === "popover" ? <div className="flex flex-wrap justify-between gap-xs text-caption text-muted-foreground"><span>{t("usage.history.peak")}{range.peak ? ` · ${usageDate(range.peak.date, language, { weekday: "short", month: "short", day: "numeric" })}` : null}</span><span className="font-mono">{range.peak ? compactTokens(range.peak.new_tokens, language) : t("usage.history.noData")}</span></div> : <div className="flex flex-wrap items-baseline gap-sm text-caption font-semibold"><span>{range.reading ? t("usage.history.soFar") : range.since ? t("usage.history.since", { date: usageDate(range.since, language) }) : t("usage.history.noData")}</span><span className="font-mono">{range.amount.calls ? compactTokens(range.amount.new_tokens, language) : null}</span></div>}
        {!day || surface === "year" ? <AgentShares shares={history.shares} selected={selected} hovered={agent} onSelect={toggle} onHover={setAgent} bars={surface === "year"} day={day} /> : null}
      </div>
    </div>
  </section>;
}
