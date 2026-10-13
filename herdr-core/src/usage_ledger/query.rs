//! Bounded, content-free display calculations, prepared on the ledger worker.
//! The shell selects an already calculated range and formats its numbers.

use std::collections::{BTreeMap, BTreeSet};

use hide_session::{Agent, usage::Tokens};
use jiff::{ToSpan, civil::Date};
use serde::Serialize;

use super::{DAILY_DAYS, DAILY_LIMIT, local_day, prices, store::Daily, worker::Progress};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Cost {
    pub micro_usd: u64,
    pub partial: bool,
    pub waiting: bool,
    pub stale: bool,
    pub incomplete: bool,
    pub unpriced_models: Vec<String>,
    pub other_unpriced_models: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Amount {
    /// Zero calls means no data, never a known zero-dollar bill.
    pub calls: u64,
    pub new_tokens: u64,
    pub tokens: Tokens,
    pub cost: Cost,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Contribution {
    pub agent: String,
    pub new_tokens: u64,
    pub share_basis_points: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Day {
    pub date: String,
    pub week: u16,
    pub weekday: u8,
    pub today: bool,
    pub recent_week: bool,
    pub level: u8,
    pub pending: bool,
    pub amount: Amount,
    pub top_agents: Vec<Contribution>,
    pub agents: Vec<Contribution>,
    pub other_agents: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Peak {
    pub date: String,
    pub new_tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Range {
    /// Inclusive civil-date bounds. Empty dates are drawn by the shell.
    pub calendar_start: String,
    pub calendar_end: String,
    /// Only dates with recorded calls, including calls with zero new tokens.
    pub days: Vec<Day>,
    pub amount: Amount,
    pub since: Option<String>,
    pub active_days: u16,
    pub observed_days: u16,
    pub peak: Option<Peak>,
    pub current_streak: u16,
    pub longest_streak: u16,
    pub reading: bool,
    pub limited: bool,
    pub files_done: usize,
    pub files_total: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AgentRange {
    pub agent: String,
    pub range: Range,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Share {
    pub contribution: Contribution,
    /// Retained history, independent of the displayed grid or selection.
    pub since: Option<String>,
    pub lifetime: Amount,
    pub providers: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Surface {
    pub all: Range,
    pub filtered: Vec<AgentRange>,
    /// Selecting an agent never removes the other agents from this list.
    pub shares: Vec<Share>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Projection {
    pub popover: Surface,
    pub year: Surface,
}

#[derive(Clone, Default)]
struct Sum {
    tokens: Tokens,
    calls: u64,
    cost: Cost,
    unpriced: BTreeSet<String>,
    providers: BTreeSet<String>,
}

fn plus(a: &mut u64, b: u64) -> Result<(), &'static str> {
    *a = a.checked_add(b).ok_or("usage_total_overflow")?;
    Ok(())
}

impl Sum {
    fn merge(&mut self, other: &Self) -> Result<(), &'static str> {
        plus(&mut self.calls, other.calls)?;
        plus(&mut self.tokens.input, other.tokens.input)?;
        plus(&mut self.tokens.output, other.tokens.output)?;
        plus(&mut self.tokens.cache_read, other.tokens.cache_read)?;
        plus(&mut self.tokens.cache_write, other.tokens.cache_write)?;
        plus(&mut self.tokens.cache_write_1h, other.tokens.cache_write_1h)?;
        if let Some(value) = other.tokens.reasoning {
            plus(self.tokens.reasoning.get_or_insert(0), value)?;
        }
        plus(&mut self.cost.micro_usd, other.cost.micro_usd)?;
        self.cost.partial |= other.cost.partial;
        self.cost.waiting |= other.cost.waiting;
        self.cost.stale |= other.cost.stale;
        self.cost.incomplete |= other.cost.incomplete;
        self.unpriced.extend(other.unpriced.iter().cloned());
        self.providers.extend(other.providers.iter().cloned());
        if self.providers.len() > 32 {
            return Err("usage_provider_capacity");
        }
        Ok(())
    }

    fn amount(&self) -> Result<Amount, &'static str> {
        let mut cost = self.cost.clone();
        cost.unpriced_models = self.unpriced.iter().take(8).cloned().collect();
        cost.other_unpriced_models = self.unpriced.len().saturating_sub(8);
        Ok(Amount {
            calls: self.calls,
            new_tokens: self.new_tokens()?,
            tokens: self.tokens.clone(),
            cost,
        })
    }

    fn new_tokens(&self) -> Result<u64, &'static str> {
        self.tokens
            .input
            .checked_add(self.tokens.output)
            .and_then(|n| n.checked_add(self.tokens.cache_write))
            .ok_or("usage_total_overflow")
    }
}

type Days = BTreeMap<Date, BTreeMap<String, Sum>>;

/// Only the own-node records enter the projection. Dates were assigned at
/// ingestion; changing the current time zone never re-buckets those records.
pub fn prepare(
    rows: &[Daily],
    progress: &[Progress],
    table: Option<&prices::Table>,
    device: &str,
    now: u64,
    zone: &jiff::tz::TimeZone,
) -> Result<Option<Projection>, &'static str> {
    if rows.len() > DAILY_LIMIT || progress.len() > 6 {
        return Err("usage_projection_capacity");
    }
    let today: Date = local_day(now, zone)?
        .parse()
        .map_err(|_| "usage_day_invalid")?;
    let cutoff = shift(today, -(DAILY_DAYS - 1))?;
    let mut days = Days::new();
    let mut lifetime = BTreeMap::<String, Sum>::new();
    for row in rows.iter().filter(|row| row.device == device) {
        if !Agent::supported().any(|agent| agent != Agent::Cursor && agent.as_str() == row.agent) {
            return Err("usage_agent_invalid");
        }
        let date: Date = row.day.parse().map_err(|_| "usage_day_invalid")?;
        if date < cutoff || date > today {
            continue;
        }
        let resolved = prices::cost(row, table, now)?;
        let mut sum = Sum {
            calls: row.totals.calls,
            tokens: row.totals.tokens.clone(),
            cost: Cost {
                micro_usd: resolved.micro_usd,
                partial: resolved.unpriced || resolved.waiting || resolved.incomplete,
                waiting: resolved.waiting,
                stale: resolved.stale,
                incomplete: resolved.incomplete,
                ..Cost::default()
            },
            providers: row.totals.providers.iter().cloned().collect(),
            ..Sum::default()
        };
        if resolved.unpriced {
            sum.unpriced.insert(row.model.clone());
        }
        days.entry(date)
            .or_default()
            .entry(row.agent.clone())
            .or_default()
            .merge(&sum)?;
        lifetime.entry(row.agent.clone()).or_default().merge(&sum)?;
    }
    let reading = progress.iter().any(|p| p.reading);
    let Some(first) = days.keys().next().copied().or(reading.then_some(today)) else {
        return Ok(None);
    };
    // One stable calendar during backfill and across agent selections.
    let start = if reading { shift(today, -45)? } else { first };
    Ok(Some(Projection {
        popover: surface(&days, &lifetime, progress, today, start, 20)?,
        year: surface(&days, &lifetime, progress, today, start, 53)?,
    }))
}

fn shift(day: Date, delta: i64) -> Result<Date, &'static str> {
    day.checked_add(delta.days())
        .map_err(|_| "usage_day_invalid")
}

fn sunday(day: Date) -> Result<Date, &'static str> {
    shift(day, -i64::from(day.weekday().to_sunday_zero_offset()))
}

fn surface(
    days: &Days,
    lifetime: &BTreeMap<String, Sum>,
    progress: &[Progress],
    today: Date,
    start: Date,
    weeks: i64,
) -> Result<Surface, &'static str> {
    let start = sunday(start)?.max(shift(sunday(today)?, -(weeks - 1) * 7)?);
    let all = range(days, progress, today, start, None)?;
    let mut filtered = Vec::new();
    let mut shares = Vec::new();
    for agent in Agent::supported().filter(|agent| *agent != Agent::Cursor) {
        let name = agent.as_str();
        let range = range(days, progress, today, start, Some(name))?;
        if let Some(sum) = lifetime.get(name) {
            shares.push(Share {
                contribution: contribution(name, range.amount.new_tokens, all.amount.new_tokens),
                since: days
                    .iter()
                    .find(|(_, values)| values.contains_key(name))
                    .map(|(date, _)| date.to_string()),
                lifetime: sum.amount()?,
                providers: sum.providers.iter().cloned().collect(),
            });
        }
        filtered.push(AgentRange {
            agent: name.into(),
            range,
        });
    }
    shares.sort_by(|a, b| {
        b.contribution
            .new_tokens
            .cmp(&a.contribution.new_tokens)
            .then_with(|| a.contribution.agent.cmp(&b.contribution.agent))
    });
    Ok(Surface {
        all,
        filtered,
        shares,
    })
}

fn contribution(agent: &str, tokens: u64, total: u64) -> Contribution {
    Contribution {
        agent: agent.into(),
        new_tokens: tokens,
        share_basis_points: if total == 0 {
            0
        } else {
            (u128::from(tokens) * 10_000 / u128::from(total)) as u16
        },
    }
}

fn range(
    source: &Days,
    progress: &[Progress],
    today: Date,
    start: Date,
    selected: Option<&str>,
) -> Result<Range, &'static str> {
    let progress: Vec<_> = progress
        .iter()
        .filter(|p| selected.is_none_or(|a| a == p.agent.as_str()))
        .collect();
    let reading = progress.iter().any(|p| p.reading);
    let mut range = Range {
        calendar_start: start.to_string(),
        calendar_end: today.to_string(),
        days: Vec::new(),
        amount: Amount::default(),
        since: None,
        active_days: 0,
        observed_days: 0,
        peak: None,
        current_streak: 0,
        longest_streak: 0,
        reading,
        limited: progress.iter().any(|p| p.limited),
        files_done: progress.iter().map(|p| p.files_done).sum(),
        files_total: progress.iter().map(|p| p.files_total).sum(),
    };
    let mut total = Sum::default();
    let mut date = start;
    let mut day_index = 0;
    let mut streak = 0;
    let mut closed = Vec::new();
    while date <= today {
        let mut sum = Sum::default();
        let mut contributors = Vec::new();
        if let Some(values) = source.get(&date) {
            for (agent, value) in values
                .iter()
                .filter(|(agent, _)| selected.is_none_or(|a| a == agent.as_str()))
            {
                sum.merge(value)?;
                contributors.push((agent, value.new_tokens()?));
            }
        }
        total.merge(&sum)?;
        let amount = sum.amount()?;
        if amount.calls > 0 && range.since.is_none() {
            range.since = Some(date.to_string());
        }
        if range.since.is_some() {
            range.observed_days += 1;
        }
        if amount.new_tokens > 0 {
            range.active_days += 1;
            streak += 1;
            range.longest_streak = range.longest_streak.max(streak);
            if date < today {
                closed.push(amount.new_tokens);
            }
            if range
                .peak
                .as_ref()
                .is_none_or(|p| amount.new_tokens > p.new_tokens)
            {
                range.peak = Some(Peak {
                    date: date.to_string(),
                    new_tokens: amount.new_tokens,
                });
            }
        } else if date < today {
            streak = 0;
        }
        contributors.sort_by(|(a, x), (b, y)| y.cmp(x).then_with(|| a.cmp(b)));
        let count = contributors.len();
        let agents: Vec<_> = contributors
            .into_iter()
            .map(|(agent, n)| contribution(agent, n, amount.new_tokens))
            .collect();
        let top_agents = agents.iter().take(3).cloned().collect();
        if amount.calls > 0 {
            range.days.push(Day {
                date: date.to_string(),
                week: day_index / 7,
                weekday: date.weekday().to_sunday_zero_offset() as u8,
                today: date == today,
                recent_week: date >= shift(today, -6)?,
                level: 0,
                pending: false,
                amount,
                top_agents,
                agents,
                other_agents: count.saturating_sub(3),
            });
        }
        date = shift(date, 1)?;
        day_index += 1;
    }
    range.current_streak = streak;
    range.amount = total.amount()?;
    closed.sort_unstable();
    let cuts = if closed.len() >= 7 {
        [1, 2, 3].map(|q| closed[(closed.len() * q).div_ceil(4) - 1])
    } else {
        // Closed days alone set scale. A first active day still paints as
        // strongest instead of masquerading as an empty level-zero day.
        let max = closed.last().copied().unwrap_or(0);
        [1, 2, 3].map(|q| (u128::from(max) * q / 4) as u64)
    };
    for day in &mut range.days {
        let n = day.amount.new_tokens;
        day.level = if n == 0 {
            0
        } else {
            1 + cuts.iter().filter(|cut| n > **cut).count() as u8
        };
    }
    Ok(range)
}

#[cfg(test)]
mod tests;
