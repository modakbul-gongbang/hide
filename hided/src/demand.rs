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
    /// Records one connection's demand. When that changes the aggregate,
    /// `announce` receives the flag the core should now hold, called while the
    /// demand is still locked, so two transitions reach the core in the order
    /// they happened. Returns the flag it announced, if any.
    pub fn set(
        &self,
        connection: u64,
        observing: bool,
        announce: impl FnOnce(bool),
    ) -> Option<bool> {
        let mut observers = self.observers.lock().expect("observation demand");
        let before = !observers.is_empty();
        if observing {
            observers.insert(connection);
        } else {
            observers.remove(&connection);
        }
        let after = !observers.is_empty();
        let changed = (before != after).then_some(after);
        if let Some(flag) = changed {
            announce(flag);
        }
        changed
    }

    /// Drops whatever demand a closed connection held.
    pub fn release(&self, connection: u64, announce: impl FnOnce(bool)) -> Option<bool> {
        self.set(connection, false, announce)
    }

    #[cfg(test)]
    fn observers(&self) -> usize {
        self.observers.lock().expect("observation demand").len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flag_follows_the_first_observer_in_and_the_last_one_out() {
        let demand = ObservationDemand::default();
        let announced = std::cell::RefCell::new(Vec::new());
        let note = |flag| announced.borrow_mut().push(flag);
        assert_eq!(demand.set(1, true, note), Some(true));
        assert_eq!(demand.set(2, true, note), None);
        assert_eq!(
            demand.set(1, true, note),
            None,
            "a repeated open is one demand"
        );
        assert_eq!(
            demand.set(1, false, note),
            None,
            "another tab is still looking"
        );
        assert_eq!(demand.release(2, note), Some(false));
        assert_eq!(demand.observers(), 0);
        assert_eq!(
            *announced.borrow(),
            [true, false],
            "only transitions are announced"
        );
    }

    #[test]
    fn a_connection_that_never_observed_releases_nothing() {
        let demand = ObservationDemand::default();
        assert_eq!(demand.release(7, |_| panic!("nothing to announce")), None);
        assert_eq!(
            demand.set(3, false, |_| panic!("nothing to announce")),
            None
        );
    }
}
