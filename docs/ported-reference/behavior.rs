use crate::aggregate::AggregateSummary;

pub const SLEEP_IDLE_MS: u64 = 60_000;
pub const SLEEP_PHASE_MS: u64 = 4_000;
pub const FREE_ROAM_IDLE_MS: u64 = 8_000;
pub const FREE_ROAM_INTERVAL_MS: u64 = 4_000;
pub const FREE_ROAM_SPEED_PX_PER_SECOND: f64 = 80.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SleepPhase {
    #[default]
    Awake,
    Yawning,
    Dozing,
    Collapsing,
    Sleeping,
    Waking,
}

impl SleepPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Awake => "awake",
            Self::Yawning => "yawning",
            Self::Dozing => "dozing",
            Self::Collapsing => "collapsing",
            Self::Sleeping => "sleeping",
            Self::Waking => "waking",
        }
    }
}

/// The clawd-compatible sleep sequence starts after one minute of inactivity.
/// Each transitional pose lasts four seconds; once asleep it remains asleep
/// until an activity event calls the wake path.
pub fn sleep_phase_for_idle_ms(idle_ms: u64) -> SleepPhase {
    if idle_ms < SLEEP_IDLE_MS {
        return SleepPhase::Awake;
    }
    let phase = (idle_ms - SLEEP_IDLE_MS) / SLEEP_PHASE_MS;
    match phase {
        0 => SleepPhase::Yawning,
        1 => SleepPhase::Dozing,
        2 => SleepPhase::Collapsing,
        _ => SleepPhase::Sleeping,
    }
}

pub fn expanded_state(summary: AggregateSummary, idle_ms: u64, waking: bool) -> &'static str {
    if summary.error > 0 {
        "error"
    } else if summary.attention > 0 {
        "notification"
    } else if summary.working >= 2 {
        "juggling"
    } else if summary.working == 1 {
        "carrying"
    } else if waking {
        "waking"
    } else if sleep_phase_for_idle_ms(idle_ms) == SleepPhase::Sleeping {
        "sleeping"
    } else if idle_ms >= FREE_ROAM_IDLE_MS {
        "roam"
    } else if summary.working > 0 {
        "working"
    } else {
        "idle"
    }
}

pub fn is_roam_allowed(
    summary: AggregateSummary,
    idle_ms: u64,
    dragging: bool,
    mini: bool,
) -> bool {
    !dragging
        && !mini
        && summary.error == 0
        && summary.attention == 0
        && summary.working == 0
        && summary.idle > 0
        && idle_ms >= FREE_ROAM_IDLE_MS
        && idle_ms < SLEEP_IDLE_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> AggregateSummary {
        AggregateSummary {
            idle: 1,
            ..Default::default()
        }
    }

    #[test]
    fn sleep_sequence_has_four_second_transitions() {
        assert_eq!(sleep_phase_for_idle_ms(59_999), SleepPhase::Awake);
        assert_eq!(sleep_phase_for_idle_ms(60_000), SleepPhase::Yawning);
        assert_eq!(sleep_phase_for_idle_ms(64_000), SleepPhase::Dozing);
        assert_eq!(sleep_phase_for_idle_ms(68_000), SleepPhase::Collapsing);
        assert_eq!(sleep_phase_for_idle_ms(72_000), SleepPhase::Sleeping);
    }

    #[test]
    fn expanded_priority_keeps_urgent_states_above_activity() {
        let mut summary = idle();
        summary.working = 2;
        assert_eq!(expanded_state(summary, 0, false), "juggling");
        summary.attention = 1;
        assert_eq!(expanded_state(summary, 90_000, false), "notification");
        summary.error = 1;
        assert_eq!(expanded_state(summary, 90_000, false), "error");
    }

    #[test]
    fn roam_is_cancelled_by_work_or_sleep() {
        assert!(is_roam_allowed(idle(), 8_000, false, false));
        assert!(!is_roam_allowed(idle(), 60_000, false, false));
        assert!(!is_roam_allowed(idle(), 8_000, true, false));
        let mut working = idle();
        working.working = 1;
        assert!(!is_roam_allowed(working, 8_000, false, false));
    }
}
