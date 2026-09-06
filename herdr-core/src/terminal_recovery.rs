use std::sync::{Mutex, Weak, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use crate::ffi::ChangeNotifier;
use crate::runtime::Runtime;

const RETRY_SECONDS: [u64; 4] = [5, 10, 20, 30];

#[derive(Clone, Debug)]
pub(crate) struct Recovery {
    pub due: Option<Instant>,
    pub retries: usize,
    pub reason: String,
    pub last_attempt_at_unix_ms: Option<u64>,
}

impl Recovery {
    pub fn new(now: Instant, reason: String) -> Self {
        Self {
            due: Some(now + Duration::from_secs(RETRY_SECONDS[0])),
            retries: 0,
            reason,
            last_attempt_at_unix_ms: None,
        }
    }

    pub fn advance(&mut self, now: Instant) -> bool {
        if self.due.is_none_or(|due| now < due) {
            return false;
        }
        if self.retries == RETRY_SECONDS.len() {
            self.due = None;
            return false;
        }
        self.retries += 1;
        // Give the final attempt time to receive a frame before exhausting it.
        let seconds = RETRY_SECONDS.get(self.retries).copied().unwrap_or(5);
        self.due = Some(now + Duration::from_secs(seconds));
        true
    }

    pub fn decision(&self) -> &'static str {
        if self.due.is_some() {
            "automatic_bounded"
        } else {
            "manual"
        }
    }

    pub fn message(&self) -> String {
        if self.due.is_none() {
            format!(
                "{}. Automatic retries exhausted; use Reconnect to try again.",
                self.reason
            )
        } else if self.retries == 0 {
            format!("{}. Retrying in 5 seconds.", self.reason)
        } else {
            format!(
                "Retrying terminal connection ({}/4). {}.",
                self.retries, self.reason
            )
        }
    }
}

/// A single clock for terminal recovery, independent of session-sync network
/// requests. It performs no I/O while holding Runtime, and skips a busy lock.
pub(crate) struct Maintenance {
    stop: mpsc::Sender<()>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Maintenance {
    pub fn spawn(runtime: Weak<Mutex<Runtime>>, notifier: ChangeNotifier) -> std::io::Result<Self> {
        let (stop, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("terminal-recovery".into())
            .spawn(move || {
                // The earliest retry is due 5 s after a failure, so a one
                // second clock lands every retry within a second of its due
                // time without taking the runtime lock ten times a second.
                while matches!(
                    receiver.recv_timeout(Duration::from_secs(1)),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    let Some(runtime) = runtime.upgrade() else {
                        break;
                    };
                    let changed = if let Ok(mut runtime) = runtime.try_lock() {
                        runtime.maintain_terminals(Instant::now())
                    } else {
                        false
                    };
                    if changed {
                        notifier.notify();
                    }
                }
            })?;
        Ok(Self {
            stop,
            worker: Some(worker),
        })
    }
}

impl Drop for Maintenance {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_has_four_bounded_retries_and_a_visible_terminal_reason() {
        let start = Instant::now();
        let mut recovery = Recovery::new(start, "Waiting for pane size".into());
        for seconds in [5, 15, 35, 65] {
            assert!(!recovery.advance(start + Duration::from_secs(seconds - 1)));
            assert!(recovery.advance(start + Duration::from_secs(seconds)));
        }
        assert!(!recovery.advance(start + Duration::from_secs(70)));
        assert!(recovery.due.is_none());
        assert!(recovery.message().contains("Waiting for pane size"));
        assert!(recovery.message().contains("exhausted"));
        assert!(!recovery.advance(start + Duration::from_secs(300)));
    }
}
