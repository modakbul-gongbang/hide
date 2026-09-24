//! Which connected clients are looking at a screen whose data costs the core a
//! probe.
//!
//! The core keeps one runtime-wide flag for the Settings agents tab
//! (`ai_settings.observing`): while it is set, the provider probe and the hook
//! diagnosis run on the coordinator. One browser tab closing that screen must
//! not stop another tab's, and a tab that disconnects without saying so must
//! not leave the probe running, so the daemon owns the flag and each
//! connection only owns its own demand.

use std::collections::HashSet;
use std::sync::Mutex;

#[derive(Default)]
pub struct ObservationDemand {
    observers: Mutex<HashSet<u64>>,
}

impl ObservationDemand {
    /// Records one connection's demand. Returns the flag the core should now
    /// hold when this changed it, and `None` when the aggregate is unchanged.
    pub fn set(&self, connection: u64, observing: bool) -> Option<bool> {
        let mut observers = self.observers.lock().expect("observation demand");
        let before = !observers.is_empty();
        if observing {
            observers.insert(connection);
        } else {
            observers.remove(&connection);
        }
        let after = !observers.is_empty();
        (before != after).then_some(after)
    }

    /// Drops whatever demand a closed connection held.
    pub fn release(&self, connection: u64) -> Option<bool> {
        self.set(connection, false)
    }

    pub fn observers(&self) -> usize {
        self.observers.lock().expect("observation demand").len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flag_follows_the_first_observer_in_and_the_last_one_out() {
        let demand = ObservationDemand::default();
        assert_eq!(demand.set(1, true), Some(true));
        assert_eq!(demand.set(2, true), None);
        assert_eq!(demand.set(1, true), None, "a repeated open is one demand");
        assert_eq!(demand.set(1, false), None, "another tab is still looking");
        assert_eq!(demand.release(2), Some(false));
        assert_eq!(demand.observers(), 0);
    }

    #[test]
    fn a_connection_that_never_observed_releases_nothing() {
        let demand = ObservationDemand::default();
        assert_eq!(demand.release(7), None);
        assert_eq!(demand.set(3, false), None);
    }
}
