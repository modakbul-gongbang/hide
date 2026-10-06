//! The local verify bundle runner (D-53): one run on the machine at a time,
//! a time cap per run, each command in its own process group the runner owns,
//! and the last 1 MiB of output kept as the run's log. Dropping the runner
//! ends whatever it started (engineering rule 14).

use std::collections::{BTreeMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use hide_platform::process::OwnedChild;

use crate::adapters::{EnvSignal, Failure, VerifyPoll};
use crate::exec::{Runner, checked, shell};

/// The most finished results kept for polling.
const RESULT_LIMIT: usize = 1_024;
/// The most runs waiting; a full queue is a reported failure (rule 15).
pub const QUEUE_LIMIT: usize = 256;
pub const LOG_LIMIT: u64 = 1024 * 1024;
/// The most output a running bundle may write before it is ended; the log
/// is cut to [`LOG_LIMIT`] only after the run (rule 15).
pub const RUN_OUTPUT_LIMIT: u64 = 256 * 1024 * 1024;

/// A step run before the commands, such as checking out a commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepare {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub id: String,
    pub cwd: PathBuf,
    pub commands: Vec<String>,
    pub prepare: Vec<Prepare>,
    pub timeout: Duration,
    /// [`RUN_OUTPUT_LIMIT`] outside tests.
    pub output_limit: u64,
}

struct Running {
    job: Job,
    index: usize,
    child: OwnedChild,
    /// When the run's first command started: the time cap covers the whole
    /// bundle (D-46).
    started: Instant,
    log: PathBuf,
}

pub struct VerifyRunner {
    logs: PathBuf,
    queue: VecDeque<Job>,
    running: Option<Running>,
    results: BTreeMap<String, VerifyPoll>,
    order: VecDeque<String>,
}

impl VerifyRunner {
    pub fn new(logs: PathBuf) -> Self {
        Self {
            logs,
            queue: VecDeque::new(),
            running: None,
            results: BTreeMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn log_path(&self, id: &str) -> PathBuf {
        self.logs.join(format!("{}.log", sanitize(id)))
    }

    /// Queues a run; the same id is never queued twice.
    pub fn submit(&mut self, job: Job) -> Result<(), Failure> {
        if self.known(&job.id) {
            return Ok(());
        }
        if self.queue.len() >= QUEUE_LIMIT {
            return Err(Failure::task("verify", "verify queue full"));
        }
        self.queue.push_back(job);
        Ok(())
    }

    pub fn known(&self, id: &str) -> bool {
        self.results.contains_key(id)
            || self.queue.iter().any(|job| job.id == id)
            || self
                .running
                .as_ref()
                .is_some_and(|running| running.job.id == id)
    }

    /// Forgets a finished result so the same id can run again.
    pub fn forget(&mut self, id: &str) {
        self.results.remove(id);
        self.order.retain(|known| known != id);
    }

    pub fn poll(&mut self, runner: &mut dyn Runner, id: &str) -> VerifyPoll {
        self.pump(runner);
        self.results.get(id).cloned().unwrap_or(VerifyPoll::Pending)
    }

    pub fn cancel(&mut self, id: &str) {
        self.queue.retain(|job| job.id != id);
        if self
            .running
            .as_ref()
            .is_some_and(|running| running.job.id == id)
            && let Some(mut running) = self.running.take()
        {
            let _ = running.child.kill_tree();
        }
    }

    /// Advances the running command and starts the next queued run.
    pub fn pump(&mut self, runner: &mut dyn Runner) {
        for _ in 0..4 {
            if let Some(running) = &mut self.running {
                let command = running.job.commands[running.index].clone();
                if running.started.elapsed() > running.job.timeout {
                    let _ = running.child.kill_tree();
                    let id = running.job.id.clone();
                    let minutes = running.job.timeout.as_secs() / 60;
                    self.running = None;
                    self.finish(
                        &id,
                        VerifyPoll::Failed {
                            check: format!("{command} (timeout {minutes}m)"),
                            link: String::new(),
                        },
                    );
                    continue;
                }
                let written = std::fs::metadata(&running.log).map_or(0, |meta| meta.len());
                if written > running.job.output_limit {
                    let _ = running.child.kill_tree();
                    let id = running.job.id.clone();
                    let log = running.log.clone();
                    self.running = None;
                    trim(&log);
                    self.finish(
                        &id,
                        VerifyPoll::Failed {
                            check: format!("{command} (output over the log cap)"),
                            link: log.display().to_string(),
                        },
                    );
                    continue;
                }
                match running.child.try_wait() {
                    Ok(None) => return,
                    Ok(Some(status)) => {
                        let id = running.job.id.clone();
                        let log = running.log.clone();
                        if status.success() {
                            running.index += 1;
                            if running.index < running.job.commands.len() {
                                let job = running.job.clone();
                                let index = running.index;
                                let started = running.started;
                                self.running = None;
                                self.spawn(job, index, started);
                                continue;
                            }
                            self.running = None;
                            trim(&log);
                            self.finish(&id, VerifyPoll::Passed);
                        } else {
                            self.running = None;
                            trim(&log);
                            let poll = read_failure(&command, status.code(), &log);
                            self.finish(&id, poll);
                        }
                        continue;
                    }
                    Err(error) => {
                        let id = running.job.id.clone();
                        self.running = None;
                        self.finish(
                            &id,
                            VerifyPoll::Failed {
                                check: command,
                                link: error.to_string(),
                            },
                        );
                        continue;
                    }
                }
            }
            let Some(job) = self.queue.pop_front() else {
                return;
            };
            // A failed step ends the run: the commands never run in a tree
            // the step did not prepare.
            if let Err(poll) = prepare(runner, &job.prepare) {
                self.finish(&job.id, poll);
                continue;
            }
            if job.commands.is_empty() {
                self.finish(&job.id, VerifyPoll::Passed);
                continue;
            }
            self.spawn(job, 0, Instant::now());
        }
    }

    fn spawn(&mut self, job: Job, index: usize, started: Instant) {
        let log = self.log_path(&job.id);
        let file = (|| -> std::io::Result<(File, File)> {
            std::fs::create_dir_all(&self.logs)?;
            let file = OpenOptions::new()
                .create(true)
                .append(index > 0)
                .write(true)
                .truncate(index == 0)
                .open(&log)?;
            let second = file.try_clone()?;
            Ok((file, second))
        })();
        let (stdout, stderr) = match file {
            Ok(files) => files,
            Err(error) => {
                let signal = (error.raw_os_error() == Some(28)).then_some(EnvSignal::DiskFull);
                let poll = match signal {
                    Some(signal) => VerifyPoll::Environment {
                        signal,
                        check: "verify log".into(),
                    },
                    None => VerifyPoll::Failed {
                        check: "verify log".into(),
                        link: error.to_string(),
                    },
                };
                return self.finish(&job.id, poll);
            }
        };
        let command_text = job.commands[index].clone();
        let mut command = Command::new(shell().0);
        command
            .arg(shell().1)
            .arg(&command_text)
            .current_dir(&job.cwd)
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr);
        match OwnedChild::spawn(&mut command) {
            Ok(child) => {
                self.running = Some(Running {
                    job,
                    index,
                    child,
                    started,
                    log,
                });
            }
            Err(error) => {
                self.finish(
                    &job.id,
                    VerifyPoll::Failed {
                        check: command_text,
                        link: error.to_string(),
                    },
                );
            }
        }
    }

    fn finish(&mut self, id: &str, poll: VerifyPoll) {
        self.results.insert(id.to_owned(), poll);
        self.order.push_back(id.to_owned());
        while self.order.len() > RESULT_LIMIT {
            if let Some(old) = self.order.pop_front() {
                self.results.remove(&old);
            }
        }
    }

    pub fn busy(&self) -> bool {
        self.running.is_some() || !self.queue.is_empty()
    }
}

/// Runs the steps in order and stops at the first failure.
fn prepare(runner: &mut dyn Runner, steps: &[Prepare]) -> Result<(), VerifyPoll> {
    for step in steps {
        let args: Vec<&str> = step.args.iter().map(String::as_str).collect();
        if let Err(failure) = checked(
            runner,
            "verify.prepare",
            &step.program,
            &args,
            Some(&step.cwd),
        ) {
            let check = format!("{} {}", step.program, step.args.join(" "));
            return Err(match failure.signal {
                Some(signal) => VerifyPoll::Environment { signal, check },
                None => VerifyPoll::Failed {
                    check,
                    link: failure.detail,
                },
            });
        }
    }
    Ok(())
}

/// A failed command: exit 137 or a full disk in its output is the
/// environment's; anything else is the Task's.
fn read_failure(command: &str, code: Option<i32>, log: &Path) -> VerifyPoll {
    let tail = std::fs::read(log)
        .ok()
        .map(|bytes| {
            String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(64 * 1024)..]).to_lowercase()
        })
        .unwrap_or_default();
    if tail.contains("no space left on device") {
        return VerifyPoll::Environment {
            signal: EnvSignal::DiskFull,
            check: command.to_owned(),
        };
    }
    if code == Some(137) || code.is_none() && tail.contains("killed") {
        return VerifyPoll::Environment {
            signal: EnvSignal::OutOfMemory,
            check: command.to_owned(),
        };
    }
    VerifyPoll::Failed {
        check: command.to_owned(),
        link: log.display().to_string(),
    }
}

/// Keeps the last [`LOG_LIMIT`] bytes of a log.
fn trim(log: &Path) {
    let Ok(meta) = std::fs::metadata(log) else {
        return;
    };
    if meta.len() <= LOG_LIMIT {
        return;
    }
    // Only the tail is read: a log cut at the output cap can be hundreds of
    // megabytes (rule 15).
    let tail = std::fs::File::open(log).and_then(|mut file| {
        use std::io::{Read, Seek, SeekFrom};
        file.seek(SeekFrom::Start(meta.len() - LOG_LIMIT))?;
        let mut tail = Vec::with_capacity(LOG_LIMIT as usize);
        file.take(LOG_LIMIT).read_to_end(&mut tail)?;
        Ok(tail)
    });
    if let Ok(tail) = tail {
        let _ = std::fs::write(log, tail);
    }
}

fn sanitize(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::Output;

    struct NoRunner;
    impl Runner for NoRunner {
        fn run(
            &mut self,
            _program: &str,
            _args: &[String],
            _cwd: Option<&Path>,
        ) -> Result<Output, Failure> {
            Ok(Output {
                code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }

    fn job(id: &str, dir: &Path, commands: &[&str], timeout: Duration) -> Job {
        Job {
            id: id.into(),
            cwd: dir.to_owned(),
            commands: commands.iter().map(|c| (*c).to_owned()).collect(),
            prepare: Vec::new(),
            timeout,
            output_limit: RUN_OUTPUT_LIMIT,
        }
    }

    /// Polls until the run leaves Pending; the deadline is a hang guard only.
    fn wait(runner: &mut VerifyRunner, id: &str) -> VerifyPoll {
        let guard = Instant::now() + Duration::from_secs(60);
        loop {
            let poll = runner.poll(&mut NoRunner, id);
            if poll != VerifyPoll::Pending || Instant::now() > guard {
                return poll;
            }
            std::thread::yield_now();
        }
    }

    #[test]
    fn commands_run_in_order_one_run_at_a_time_and_name_the_failing_command() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = VerifyRunner::new(dir.path().join("logs"));
        runner
            .submit(job(
                "a",
                dir.path(),
                &["echo one > a.txt", "test -f a.txt"],
                Duration::from_secs(60),
            ))
            .unwrap();
        runner
            .submit(job(
                "b",
                dir.path(),
                &["true", "echo broken >&2; exit 3"],
                Duration::from_secs(60),
            ))
            .unwrap();
        runner.pump(&mut NoRunner);
        assert!(
            runner.running.as_ref().is_some_and(|r| r.job.id == "a"),
            "one run at a time"
        );
        assert_eq!(wait(&mut runner, "a"), VerifyPoll::Passed);
        match wait(&mut runner, "b") {
            VerifyPoll::Failed { check, link } => {
                assert_eq!(check, "echo broken >&2; exit 3");
                assert!(std::fs::read_to_string(link).unwrap().contains("broken"));
            }
            other => panic!("{other:?}"),
        }
        // The same id is not queued again until forgotten.
        runner
            .submit(job("a", dir.path(), &["false"], Duration::from_secs(60)))
            .unwrap();
        assert_eq!(runner.poll(&mut NoRunner, "a"), VerifyPoll::Passed);
    }

    #[test]
    fn a_run_past_its_cap_is_ended_and_failed_and_cancel_ends_the_tree() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = VerifyRunner::new(dir.path().join("logs"));
        runner
            .submit(job("slow", dir.path(), &["sleep 30"], Duration::ZERO))
            .unwrap();
        runner.pump(&mut NoRunner);
        match wait(&mut runner, "slow") {
            VerifyPoll::Failed { check, .. } => assert_eq!(check, "sleep 30 (timeout 0m)"),
            other => panic!("{other:?}"),
        }
        runner
            .submit(job(
                "cancelled",
                dir.path(),
                &["sleep 30"],
                Duration::from_secs(60),
            ))
            .unwrap();
        runner.pump(&mut NoRunner);
        assert!(runner.busy());
        runner.cancel("cancelled");
        assert!(!runner.busy());
    }

    #[test]
    fn the_time_cap_covers_the_whole_bundle_not_each_command() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = VerifyRunner::new(dir.path().join("logs"));
        // Each command fits the cap alone; together they do not.
        runner
            .submit(job(
                "bundle",
                dir.path(),
                &["sleep 1", "sleep 1"],
                Duration::from_millis(1_500),
            ))
            .unwrap();
        runner.pump(&mut NoRunner);
        match wait(&mut runner, "bundle") {
            VerifyPoll::Failed { check, .. } => assert_eq!(check, "sleep 1 (timeout 0m)"),
            other => panic!("{other:?}"),
        }
    }

    struct FailingRunner;
    impl Runner for FailingRunner {
        fn run(
            &mut self,
            _program: &str,
            _args: &[String],
            _cwd: Option<&Path>,
        ) -> Result<Output, Failure> {
            Ok(Output {
                code: Some(1),
                stdout: String::new(),
                stderr: "error: your local changes would be overwritten".into(),
            })
        }
    }

    #[test]
    fn a_failed_prepare_step_ends_the_run_before_any_command() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = VerifyRunner::new(dir.path().join("logs"));
        let step = Prepare {
            program: "git".into(),
            args: vec!["checkout".into(), "--detach".into(), "abc".into()],
            cwd: dir.path().to_owned(),
        };
        for (id, commands) in [("with", vec!["touch ran.txt"]), ("without", Vec::new())] {
            let mut job = job(id, dir.path(), &commands, Duration::from_secs(60));
            job.prepare = vec![step.clone()];
            runner.submit(job).unwrap();
            match runner.poll(&mut FailingRunner, id) {
                VerifyPoll::Failed { check, .. } => assert_eq!(check, "git checkout --detach abc"),
                other => panic!("{other:?}"),
            }
            assert!(!runner.busy(), "nothing runs after the failed step");
            // The failure stays the answer.
            assert!(matches!(
                runner.poll(&mut FailingRunner, id),
                VerifyPoll::Failed { .. }
            ));
        }
        assert!(!dir.path().join("ran.txt").exists());
    }

    #[test]
    fn a_long_log_keeps_its_last_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("run.log");
        let mut text = vec![b'a'; LOG_LIMIT as usize + 10];
        text.extend_from_slice(b"the end");
        std::fs::write(&log, &text).unwrap();
        trim(&log);
        let kept = std::fs::read(&log).unwrap();
        assert_eq!(kept.len() as u64, LOG_LIMIT);
        assert!(kept.ends_with(b"the end"));
    }

    #[test]
    fn a_run_writing_past_the_output_cap_is_ended_and_failed() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = VerifyRunner::new(dir.path().join("logs"));
        let mut loud = job("loud", dir.path(), &["yes"], Duration::from_secs(60));
        loud.output_limit = 64 * 1024;
        runner.submit(loud).unwrap();
        match wait(&mut runner, "loud") {
            VerifyPoll::Failed { check, link } => {
                assert_eq!(check, "yes (output over the log cap)");
                assert!(std::fs::metadata(link).unwrap().len() <= LOG_LIMIT);
            }
            other => panic!("{other:?}"),
        }
        assert!(!runner.busy());
    }

    #[test]
    fn exit_137_is_the_environment() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = VerifyRunner::new(dir.path().join("logs"));
        runner
            .submit(job(
                "oom",
                dir.path(),
                &["exit 137"],
                Duration::from_secs(60),
            ))
            .unwrap();
        assert_eq!(
            wait(&mut runner, "oom"),
            VerifyPoll::Environment {
                signal: EnvSignal::OutOfMemory,
                check: "exit 137".into()
            }
        );
    }
}
