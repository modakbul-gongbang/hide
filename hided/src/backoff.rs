//! The wait between tries of something that failed, shared by the node's
//! link to its core and its announce of this machine's windows: two seconds
//! first, doubling to a minute, and starting over when a try worked or the
//! caller is woken.

use std::time::Duration;

/// The first wait after a failure.
pub const FIRST_WAIT: Duration = Duration::from_secs(2);
/// The longest wait between tries.
pub const LONGEST_WAIT: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug)]
pub struct Backoff {
    next: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self { next: FIRST_WAIT }
    }
}

impl Backoff {
    /// A try failed: the wait before the next one. The wait after it is
    /// twice as long, up to [`LONGEST_WAIT`].
    pub fn failed(&mut self) -> Duration {
        let wait = self.next;
        self.next = (wait * 2).min(LONGEST_WAIT);
        wait
    }

    /// The next failure waits [`FIRST_WAIT`] again.
    pub fn reset(&mut self) {
        self.next = FIRST_WAIT;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wait_doubles_to_a_minute_and_a_reset_starts_it_over() {
        let mut backoff = Backoff::default();
        let waits: Vec<_> = (0..7).map(|_| backoff.failed().as_secs()).collect();
        assert_eq!(waits, [2, 4, 8, 16, 32, 60, 60]);
        backoff.reset();
        assert_eq!(backoff.failed(), FIRST_WAIT);
    }
}
