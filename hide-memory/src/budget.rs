//! The two consumers of Memory's single UTC-day call budget.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryCallKind {
    Analysis,
    Check,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryCallDeferral {
    DailyCap,
    CheckShareSpent,
    ReservedForChecks,
}

/// The next reset of the same UTC-day counters admission reads.
pub fn memory_call_reset_at(now_unix_ms: u64) -> u64 {
    (now_unix_ms / crate::store::DAY_MS)
        .saturating_add(1)
        .saturating_mul(crate::store::DAY_MS)
}

pub(crate) fn deferral(
    total: u64,
    checks: u64,
    pending: u64,
    kind: MemoryCallKind,
) -> Option<MemoryCallDeferral> {
    if total >= crate::DAILY_ANALYSIS_CALL_LIMIT {
        return Some(MemoryCallDeferral::DailyCap);
    }
    let remaining = crate::MAINTENANCE_CALL_LIMIT.saturating_sub(checks);
    match kind {
        MemoryCallKind::Check if remaining == 0 => Some(MemoryCallDeferral::CheckShareSpent),
        MemoryCallKind::Analysis
            if crate::DAILY_ANALYSIS_CALL_LIMIT - total <= pending.min(remaining) =>
        {
            Some(MemoryCallDeferral::ReservedForChecks)
        }
        _ => None,
    }
}
