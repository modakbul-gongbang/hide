//! The two consumers of Memory's single UTC-day call budget.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryCallKind {
    BackgroundAnalysis,
    /// An explicit add is not delayed by background work's reservation.
    ForegroundAnalysis,
    Check,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryCallDeferral {
    DailyCap,
    CheckShareSpent,
    ReservedForChecks,
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
        MemoryCallKind::BackgroundAnalysis
            if crate::DAILY_ANALYSIS_CALL_LIMIT - total <= pending.min(remaining) =>
        {
            Some(MemoryCallDeferral::ReservedForChecks)
        }
        _ => None,
    }
}
