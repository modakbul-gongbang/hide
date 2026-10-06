//! A long piece of node work that its caller can stop: the work runs on its
//! own thread while the node asks the caller, at least once a second,
//! whether to go on.

use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::Duration;

/// How often a running piece of work asks its caller whether to go on.
pub const HEARTBEAT: Duration = Duration::from_secs(1);

/// Runs `work` on a thread named `name` and answers what it returned.
/// While it runs, `report` is asked every [`HEARTBEAT`]; a false answer
/// calls `stop` once, which `work` is expected to notice and end early.
pub fn run_reporting<T: Send + 'static>(
    name: &str,
    work: impl FnOnce() -> T + Send + 'static,
    stop: &dyn Fn(),
    report: &mut dyn FnMut() -> bool,
) -> Result<T, String> {
    let (sender, answer) = channel();
    std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || {
            let _ = sender.send(work());
        })
        .map_err(|error| format!("{name} could not start: {error}"))?;
    let mut stopped = false;
    loop {
        match answer.recv_timeout(HEARTBEAT) {
            Ok(value) => return Ok(value),
            Err(RecvTimeoutError::Timeout) => {
                if !stopped && !report() {
                    stop();
                    stopped = true;
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(format!("{name} ended without an answer"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    #[test]
    fn a_false_report_stops_the_work_once_and_its_answer_still_arrives() {
        let stopping = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&stopping);
        let mut reports = 0;
        let answer = run_reporting(
            "reporting-test",
            move || {
                while !seen.load(Ordering::SeqCst) {
                    std::thread::yield_now();
                }
                "stopped"
            },
            &|| stopping.store(true, Ordering::SeqCst),
            &mut || {
                reports += 1;
                false
            },
        );
        assert_eq!(answer, Ok("stopped"));
        assert_eq!(reports, 1);
    }
}
