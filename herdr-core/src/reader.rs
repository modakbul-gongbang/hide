//! One worker thread per capability reader, so a slow subprocess never
//! becomes coordinator latency.
//!
//! The existing `lsof` and `git status` readers run their subprocess inline on
//! the session-sync coordinator thread, which is also the thread that applies
//! every Herdr topology event. That is affordable at tens of milliseconds and
//! ruinous at seconds: `du` over a `target/` tree takes ten, `gh pr list`
//! takes one to three, and every pane event behind them would land that late.
//!
//! So the readers built on this helper keep the same shape the coordinator
//! already drives - one `read_if_due` call per wake, an answer or nothing -
//! and move only the blocking part off the thread. The coordinator wakes at
//! least once a second for agent telemetry, so a finished read is picked up on
//! the next wake rather than needing its own wakeup channel.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

/// Drives one reader's work on a worker thread.
///
/// `Q` is what to read and doubles as the freshness key: a changed request is
/// always due, so selecting a different checkout does not wait out the
/// interval.
///
/// `spacing` is the least time between one read finishing and a changed
/// request starting the next. Without it a request that moves every second -
/// a project whose dozen agents each finish a turn - is read every second,
/// two `gh` subprocesses at a time. The changed request is not dropped: it
/// stays different from the settled one, so it starts as soon as the spacing
/// has passed, and every move inside the wait coalesces into that one read.
///
/// Every finished read is handed back, even one whose request has since
/// moved. The first version dropped those, and starved: a worktree read over
/// eight repositories takes longer than the request stays still (a `gh`
/// answer changes the bases, a pane changes the project list), so no answer
/// ever landed and the sidebar showed no worktrees at all. A slightly old
/// answer is corrected by the read that starts on the same wake; no answer is
/// corrected by nothing. Each answer carries what it describes, so an ingest
/// that must not accept a stale one - the disk measurement, keyed by path -
/// can tell.
pub struct BackgroundRead<Q, A> {
    interval: Option<Duration>,
    spacing: Duration,
    read: Arc<dyn Fn(&Q) -> A + Send + Sync>,
    inflight: Option<(Q, Receiver<A>)>,
    settled: Option<(Q, Instant)>,
}

impl<Q, A> BackgroundRead<Q, A>
where
    Q: Clone + PartialEq + Send + 'static,
    A: Send + 'static,
{
    pub fn new(
        interval: Duration,
        spacing: Duration,
        read: impl Fn(&Q) -> A + Send + Sync + 'static,
    ) -> Self {
        Self {
            interval: Some(interval),
            spacing,
            read: Arc::new(read),
            inflight: None,
            settled: None,
        }
    }

    /// A reader with no wall-clock expiry. Only changed inputs request work.
    pub fn on_change(spacing: Duration, read: impl Fn(&Q) -> A + Send + Sync + 'static) -> Self {
        let mut reader = Self::new(Duration::ZERO, spacing, read);
        reader.interval = None;
        reader
    }

    /// Returns an answer on the wake a worker's result arrives. `None` covers
    /// every other case: a worker is still running, or the last answer is
    /// still inside its window. A read for a request that has since moved is
    /// still returned, and the next read starts on this same call.
    pub fn poll(&mut self, request: Q) -> Option<A> {
        let mut answer = None;
        if let Some((inflight_request, receiver)) = self.inflight.as_ref() {
            match receiver.try_recv() {
                Ok(value) => {
                    self.settled = Some((inflight_request.clone(), Instant::now()));
                    self.inflight = None;
                    answer = Some(value);
                }
                // The worker panicked, so nothing will ever arrive on this
                // channel. Clearing it lets the next due check start a fresh
                // read instead of waiting forever on a dead thread.
                Err(TryRecvError::Disconnected) => self.inflight = None,
                Err(TryRecvError::Empty) => return None,
            }
        }

        let due = match self.settled.as_ref() {
            Some((settled_request, settled_at)) => {
                let since = settled_at.elapsed();
                (*settled_request != request && since >= self.spacing)
                    || self.interval.is_some_and(|interval| since >= interval)
            }
            None => true,
        };
        if due && self.inflight.is_none() {
            let (sender, receiver) = channel();
            let read = Arc::clone(&self.read);
            let worker_request = request.clone();
            // A detached thread: the answer arrives on the channel or it does
            // not, and the coordinator never joins on it. Dropping the
            // receiver on shutdown makes the send fail, which the worker
            // ignores.
            let spawned = std::thread::Builder::new()
                .name("hide-reader".to_owned())
                .spawn(move || {
                    let _ = sender.send(read(&worker_request));
                });
            match spawned {
                Ok(_) => self.inflight = Some((request, receiver)),
                // A thread the OS refused is a real failure, not an empty
                // answer: it is stated once here and the next wake retries.
                Err(error) => eprintln!(
                    "{}",
                    serde_json::json!({
                        "component": "reader",
                        "kind": "worker.spawn_failed",
                        "message": error.to_string(),
                    })
                ),
            }
        }
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settle<Q, A>(reader: &mut BackgroundRead<Q, A>, request: Q) -> A
    where
        Q: Clone + PartialEq + Send + 'static,
        A: Send + 'static,
    {
        for _ in 0..500 {
            if let Some(answer) = reader.poll(request.clone()) {
                return answer;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("the worker never answered");
    }

    #[test]
    fn an_answer_arrives_on_a_later_poll_rather_than_blocking_the_first() {
        let mut reader =
            BackgroundRead::new(Duration::from_secs(60), Duration::ZERO, |request: &u8| {
                std::thread::sleep(Duration::from_millis(20));
                u32::from(*request) * 2
            });
        assert_eq!(reader.poll(7), None);
        assert_eq!(settle(&mut reader, 7), 14);
    }

    #[test]
    fn a_settled_request_is_not_read_again_inside_its_window() {
        let counter = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let counted = Arc::clone(&counter);
        let mut reader =
            BackgroundRead::new(Duration::from_secs(60), Duration::ZERO, move |_: &u8| {
                counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            });
        settle(&mut reader, 1);
        for _ in 0..20 {
            assert_eq!(reader.poll(1), None);
        }
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// The answer for the overtaken request is still handed back, and the
    /// read for the new one starts on that same poll - so a request that
    /// keeps moving still publishes on every completed read instead of never.
    #[test]
    fn a_changed_request_publishes_the_overtaken_answer_and_reads_again() {
        let mut reader =
            BackgroundRead::new(Duration::from_secs(60), Duration::ZERO, |request: &u8| {
                std::thread::sleep(Duration::from_millis(20));
                u32::from(*request)
            });
        assert_eq!(reader.poll(1), None);
        assert_eq!(
            settle(&mut reader, 2),
            1,
            "the finished read for 1 is not thrown away"
        );
        assert_eq!(settle(&mut reader, 2), 2, "and the read for 2 follows");
    }

    /// A request that keeps moving is read once per spacing, not once per
    /// move, and the move is never lost: it is read as soon as the spacing
    /// has passed.
    #[test]
    fn a_request_that_moves_inside_the_spacing_waits_for_it_and_is_then_read() {
        let mut reader = BackgroundRead::new(
            Duration::from_secs(60),
            Duration::from_millis(80),
            |request: &u8| u32::from(*request),
        );
        assert_eq!(settle(&mut reader, 1), 1);
        let started = Instant::now();
        let answer = settle(&mut reader, 2);
        assert_eq!(answer, 2);
        assert!(
            started.elapsed() >= Duration::from_millis(80),
            "the changed request waited out the spacing"
        );
    }
}
