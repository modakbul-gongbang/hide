//! Processes the app starts, and processes it only looks at.
//!
//! The one way a child is started is [`OwnedChild::spawn`]: the child leads
//! its own process group on Unix and lives in its own job object on Windows,
//! so ending it ends everything it started, and dropping the owner ends it
//! too (engineering rule 14; practice `process.md`). On Windows the job also
//! ends the child when the owner dies without running a destructor
//! (`KILL_ON_JOB_CLOSE`). [`OwnedChild::spawn_guarded`] adds an acknowledged
//! independent Unix owner channel for cooperative children that retain an
//! [`OwnerWatch`], without using or replacing stdin.
//!
//! What a caller can rely on, on all three systems (`tests/process.rs` checks
//! each line):
//!
//! - [`OwnedChild::kill_tree`] and dropping the owner end the child and every
//!   process it started, including ones that already left its direct line
//!   (while the child has not been waited for; a finished child's leftovers
//!   are ended by `kill_tree` at the moment it exits).
//! - [`kill_tree`] ends a process and its descendants by pid.
//! - [`is_alive`], [`parent_of`], [`descends_from`] and [`start_time`] describe a pid; a pid that
//!   does not exist is `false` or `NotFound`, never a default. A start time is
//!   an opaque number whose only use is to compare with itself, to tell a pid
//!   from a later process that reused it.
//! - [`measure_tree`] counts a process's descendants and sums their resident
//!   memory, reading the kernel's tables and forking nothing.
//! - [`run_to_end`] answers a child's exit code and capped output, or ends the
//!   child's whole tree when its deadline passes or the caller stops waiting.
//! - A pid that names no process a caller may signal (0 and 1) is refused with
//!   `InvalidInput` by every function that ends a process, so a damaged pid
//!   file cannot reach the caller's own group or the init process.
//!
//! Where the system cannot answer, the call says `ErrorKind::Unsupported`:
//! a Windows process has no working directory a caller can read, no process
//! group to signal, and no polite termination.

use std::io::{self, Read};
use std::path::PathBuf;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus};
use std::time::{Duration, Instant};

/// How deep the walk down a process tree goes. A real tree is a handful deep;
/// the bound only stops a pid-reuse cycle in a corrupt reading from looping.
const MAX_DEPTH: usize = 32;

/// Hard allocation ceiling for one capture, across stdout and stderr.
/// A caller's smaller limit remains binding (hooks use 64 KiB).
pub const MAX_CAPTURE_BYTES: usize = 1024 * 1024;

/// Runtime-injected launch metadata, never an operator setting: optional for
/// standalone commands, required by a guarded Unix launch. The value names
/// one inherited stream-socket descriptor >= 3; malformed values fail before
/// work starts. Missing metadata means no Unix watch (the parent then cannot
/// acknowledge a guarded launch). Windows instead injects a private Local
/// job name; the receiver verifies its own membership and closes the query
/// handle before returning. Neither key is a security authority for callers
/// that control this account's launch environment.
pub const OWNER_LAUNCH_KEYS: &[&str] = &["HIDE_PROCESS_OWNER_FD", "HIDE_PROCESS_OWNER_JOB"];

/// A cooperative Unix child's independent watch of its owning process.
/// Hold it until all work and output are finished. It uses its own channel,
/// not stdin; after startup the descriptor is close-on-exec. Only one watch
/// may live in a process. Windows's job supplies the lifetime boundary.
pub struct OwnerWatch {
    #[cfg(unix)]
    channel: std::os::unix::net::UnixStream,
    #[cfg(unix)]
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    #[cfg(unix)]
    reader: Option<std::thread::JoinHandle<()>>,
    #[cfg(windows)]
    active: bool,
}

impl OwnerWatch {
    pub fn from_launch() -> io::Result<Option<Self>> {
        #[cfg(unix)]
        let (key, foreign) = (OWNER_LAUNCH_KEYS[0], OWNER_LAUNCH_KEYS[1]);
        #[cfg(windows)]
        let (key, foreign) = (OWNER_LAUNCH_KEYS[1], OWNER_LAUNCH_KEYS[0]);
        if std::env::var_os(foreign).is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "owner launch metadata belongs to another operating system",
            ));
        }
        match std::env::var_os(key) {
            None => Ok(None),
            Some(value) => sys::watch_owner(value).map(Some),
        }
    }

    /// Cancels and joins the watch on a normal child exit. The explicit form
    /// reports failures; drop also cancels so an early return keeps no reader.
    pub fn close(mut self) -> io::Result<()> {
        self.close_inner()
    }

    fn close_inner(&mut self) -> io::Result<()> {
        #[cfg(unix)]
        {
            use std::sync::atomic::Ordering;
            let Some(reader) = self.reader.take() else {
                return Ok(());
            };
            self.cancelled.store(true, Ordering::SeqCst);
            let shutdown = self.channel.shutdown(std::net::Shutdown::Both);
            // The watch's bounded read timeout also wakes it if shutdown
            // fails. Join and return the admission slot on every path.
            let joined = reader
                .join()
                .map_err(|_| io::Error::other("owner watch panicked"));
            sys::watch_closed();
            shutdown?;
            joined?;
        }
        #[cfg(windows)]
        if self.active {
            self.active = false;
            sys::watch_closed();
        }
        Ok(())
    }
}

impl Drop for OwnerWatch {
    fn drop(&mut self) {
        if let Err(error) = self.close_inner() {
            eprintln!("process.owner_watch_cleanup_failed error={error}");
        }
    }
}

/// Complete output from a child whose exit and owned cleanup were observed.
#[derive(Debug)]
pub struct CapturedOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Debug)]
pub enum CaptureFailureKind {
    Deadline,
    OutputLimit { limit: usize },
    Io(io::Error),
    Cleanup,
}

/// A failed capture retains bounded partial bytes and the primary failure.
/// A separate cleanup error means the caller still owns an unconfirmed
/// child and must explicitly finish or report its cleanup; it is not gone.
#[derive(Debug)]
pub struct CaptureError {
    pub kind: CaptureFailureKind,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub cleanup: Option<io::Error>,
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            CaptureFailureKind::Deadline => {
                formatter.write_str("child capture deadline reached")?
            }
            CaptureFailureKind::OutputLimit { limit } => {
                write!(formatter, "child output exceeds {limit} bytes")?
            }
            CaptureFailureKind::Io(source) => write!(formatter, "child capture failed: {source}")?,
            CaptureFailureKind::Cleanup => formatter.write_str("child cleanup unconfirmed")?,
        }
        if let Some(source) = &self.cleanup {
            write!(formatter, "; cleanup: {source}")?;
        }
        Ok(())
    }
}

impl std::error::Error for CaptureError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            CaptureFailureKind::Io(source) => Some(source),
            _ => self.cleanup.as_ref().map(|error| error as _),
        }
    }
}

/// A failed guarded launch keeps its primary error. If cleanup could not be
/// confirmed within the launch deadline, its child handle stays in the error
/// for explicit recovery rather than being declared gone. Obtain this value
/// by downcasting the returned `io::Error`'s inner error.
#[derive(Debug)]
pub struct GuardedSpawnError {
    pub source: io::Error,
    pub cleanup: Option<io::Error>,
    pub child: Option<OwnedChild>,
}

impl std::fmt::Display for GuardedSpawnError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "guarded child launch failed: {}", self.source)?;
        if let Some(source) = &self.cleanup {
            write!(formatter, "; cleanup unconfirmed: {source}")?;
        }
        Ok(())
    }
}

impl std::error::Error for GuardedSpawnError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

fn failed_guarded_spawn(source: io::Error, mut child: OwnedChild, deadline: Instant) -> io::Error {
    let cleanup = child.finish_capture(deadline).err();
    let kind = source.kind();
    let child = cleanup.as_ref().map(|_| child);
    io::Error::new(
        kind,
        GuardedSpawnError {
            source,
            cleanup,
            child,
        },
    )
}

/// A child this process started and answers for: it leads its own group, and
/// dropping the owner ends the child and everything it started that is still
/// alive.
pub struct OwnedChild {
    child: Child,
    tie: sys::Tie,
    /// The tree is already ended, so a later drop has nothing to signal.
    ended: bool,
    /// The child has been waited for, so its id may name another process.
    reaped: bool,
    /// Ownership was given up: the drop ends nothing.
    released: bool,
    /// A deadline-bound run must never block in its destructor.
    bounded_drop: bool,
}

impl OwnedChild {
    /// Starts `command` as a child that leads its own group, so a terminal's
    /// interrupt or hangup sent to this process's group no longer reaches it.
    /// The caller sets the program, arguments, environment and stdio first; on
    /// Windows the creation flags are replaced by the ones the job needs.
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        let (child, tie) = sys::spawn(command, None)?;
        Ok(Self {
            child,
            tie,
            ended: false,
            reaped: false,
            released: false,
            bounded_drop: false,
        })
    }

    /// Starts a cooperative child with an abrupt-owner-death boundary.
    /// Unix children call [`OwnerWatch::from_launch`] before other work and
    /// hold the watch for their full lifetime; the constructor waits for its
    /// acknowledgement using the same absolute deadline. Windows uses the
    /// existing non-inherited kill-on-close job. Runtime stdin is untouched.
    /// An uncooperative Unix program cannot return guarded success.
    pub fn spawn_guarded(mut command: Command, deadline: Instant) -> io::Result<Self> {
        let (child, tie) = sys::spawn(&mut command, Some(deadline))?;
        Ok(Self {
            child,
            tie,
            ended: false,
            reaped: false,
            released: false,
            bounded_drop: true,
        })
    }

    /// Captures piped stdout/stderr until both close and the child exits.
    /// Reads are nonblocking on Unix and peek only available pipe bytes on
    /// Windows; there are no reader threads or joins. An inherited pipe
    /// cannot extend the absolute deadline, and neither pipe can exceed the
    /// caller's combined limit or [`MAX_CAPTURE_BYTES`]. Stdin stays owned
    /// by the caller and is neither read nor replaced here.
    ///
    /// Every error attempts owned tree cleanup within that same deadline.
    /// When cleanup cannot be confirmed, the returned error reports it and
    /// this handle remains the caller's responsibility for explicit reaping.
    /// Kernel calls themselves have no universal cancellation guarantee.
    pub fn capture_until(
        &mut self,
        deadline: Instant,
        output_limit: usize,
    ) -> Result<CapturedOutput, CaptureError> {
        self.bounded_drop = true;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let captured = (|| {
            if output_limit == 0 || output_limit > MAX_CAPTURE_BYTES {
                return Err(CaptureFailureKind::Io(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "capture limit must be between 1 byte and 1 MiB",
                )));
            }
            let mut out = self.child.stdout.take();
            let mut err = self.child.stderr.take();
            if let Some(pipe) = &out {
                sys::nonblocking(pipe).map_err(CaptureFailureKind::Io)?;
            }
            if let Some(pipe) = &err {
                sys::nonblocking(pipe).map_err(CaptureFailureKind::Io)?;
            }
            loop {
                if Instant::now() >= deadline {
                    return Err(CaptureFailureKind::Deadline);
                }
                let out_progress = read_capture(&mut out, &mut stdout, stderr.len(), output_limit)?;
                let err_progress = read_capture(&mut err, &mut stderr, stdout.len(), output_limit)?;
                if out.is_none()
                    && err.is_none()
                    && sys::has_exited(&self.child).map_err(CaptureFailureKind::Io)?
                {
                    break;
                }
                if !out_progress && !err_progress {
                    pause_until(deadline);
                }
            }
            Ok(())
        })();
        let cleanup = self.finish_capture(deadline);
        match (captured, cleanup) {
            (Ok(()), Ok(status)) => Ok(CapturedOutput {
                status,
                stdout,
                stderr,
            }),
            (captured, cleanup) => Err(CaptureError {
                kind: captured.err().unwrap_or(CaptureFailureKind::Cleanup),
                stdout,
                stderr,
                cleanup: cleanup.err(),
            }),
        }
    }

    fn finish_capture(&mut self, deadline: Instant) -> io::Result<ExitStatus> {
        // Keep the leader unreaped until the tree is walked: a waited-for
        // leader no longer proves ancestry or owns its pid.
        self.kill_tree()?;
        loop {
            if let Some(status) = self.try_wait()? {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "owned child exit unconfirmed",
                ));
            }
            pause_until(deadline);
        }
    }

    pub fn id(&self) -> u32 {
        self.child.id()
    }

    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child.stdin.take()
    }

    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }

    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.stderr.take()
    }

    /// `Ok(None)` while the child runs.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        self.reaped |= status.is_some();
        Ok(status)
    }

    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        let status = self.child.wait()?;
        self.reaped = true;
        Ok(status)
    }

    /// Gives up ownership without ending anything: what the child started
    /// stays running (a daemon a finished command left on purpose). The child
    /// should have been waited for first, so none is left unreaped.
    pub fn release(mut self) {
        // A cooperative watch requires its owner until the guarded child
        // finishes. Releasing a still-live guarded child closes that channel,
        // and it exits; detached daemons must use the legacy spawn path.
        sys::release(&self.tie);
        self.released = true;
    }

    /// Ends the child and everything it started, with no chance to clean up,
    /// and says whether anything was still running to end. Safe to call after
    /// the child has exited (it still ends what the child left behind, as long
    /// as something of its group lives) and more than once. The child is not
    /// reaped: `wait` does that. On Unix, descendants that left the group with
    /// `setsid` are found by walking the process table before the group is
    /// signalled, which is only done while the child has not been waited for:
    /// a reaped child's pid may name another process.
    pub fn kill_tree(&mut self) -> io::Result<bool> {
        if self.ended && self.reaped {
            // Nothing is left that this owner could name, and the child's id
            // may be another process's by now.
            return Ok(false);
        }
        let ended = sys::kill_tree_of(&mut self.child, &self.tie, self.reaped)?;
        self.ended = true;
        Ok(ended)
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        // A child already waited for leaves nothing on Unix for the drop to
        // name (its pid may be another process's by now); a caller that wants
        // the leftovers of a finished child ended calls `kill_tree` at the
        // moment it exits. A Windows job ends them when it closes after this.
        if !self.ended && !self.reaped {
            self.ended = sys::kill_tree_of(&mut self.child, &self.tie, false).is_ok();
        }
        if (self.ended || self.reaped) && !self.bounded_drop {
            // The kill is a signal away from the child exiting; reaping here
            // leaves no zombie and costs microseconds.
            let _ = self.child.wait();
        } else {
            // A failed kill or a deadline-bound run cannot wait on a live
            // child here. The run reports uncertainty and retains ownership.
            let _ = self.child.try_wait();
        }
    }
}

fn pause_until(deadline: Instant) {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if !remaining.is_zero() {
        std::thread::sleep(remaining.min(Duration::from_millis(2)));
    }
}

fn read_capture<P: Read + sys::PipeHandle>(
    pipe: &mut Option<P>,
    bytes: &mut Vec<u8>,
    other_len: usize,
    limit: usize,
) -> Result<bool, CaptureFailureKind> {
    let Some(reader) = pipe else { return Ok(false) };
    let remaining = limit - bytes.len() - other_len;
    let mut buffer = [0u8; 4096];
    let capacity = buffer.len().min(remaining + 1);
    match sys::read_available(reader, &mut buffer[..capacity]) {
        Ok(Some(0)) => {
            *pipe = None;
            Ok(true)
        }
        Ok(Some(count)) => {
            bytes.extend_from_slice(&buffer[..count.min(remaining)]);
            if count > remaining {
                Err(CaptureFailureKind::OutputLimit { limit })
            } else {
                Ok(true)
            }
        }
        Ok(None) => Ok(false),
        Err(source) => Err(CaptureFailureKind::Io(source)),
    }
}

impl std::fmt::Debug for OwnedChild {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OwnedChild")
            .field("id", &self.child.id())
            .finish_non_exhaustive()
    }
}

/// The most output [`run_to_end`] keeps from one stream; the rest is read and
/// dropped, because a child that writes forever must not grow its caller
/// (engineering rule 15).
pub const RUN_OUTPUT_CAP: usize = 64 * 1024;

/// What a child [`run_to_end`] waited for answered.
#[derive(Debug)]
pub struct Finished {
    /// `None` when a signal ended it.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Finished {
    pub fn succeeded(&self) -> bool {
        self.code == Some(0)
    }

    /// The last non-empty line of standard error, for a one-sentence reason.
    pub fn last_error_line(&self) -> String {
        self.stderr
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_owned()
    }
}

/// Why [`run_to_end`] has no answer. The child and everything it started are
/// ended in every case but `Start`.
#[derive(Debug)]
pub enum RunFailure {
    Start(io::Error),
    /// A wait, pipe read or cleanup failed; cleanup uncertainty retains a
    /// [`RunCleanupFailure`] inside this error for explicit recovery.
    Wait(io::Error),
    /// The deadline passed first.
    TimedOut,
    /// The caller raised `stop` first.
    Stopped,
}

/// Cleanup could not be confirmed. The original answer and child ownership
/// remain available through `RunFailure::Wait`'s I/O error, never a fake exit.
#[derive(Debug)]
pub struct RunCleanupFailure {
    pub outcome: Result<Finished, RunFailure>,
    pub cleanup: io::Error,
    pub child: OwnedChild,
}

impl std::fmt::Display for RunCleanupFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "child {} cleanup unconfirmed: {}; original ",
            self.child.id(),
            self.cleanup
        )?;
        match &self.outcome {
            Ok(finished) => write!(formatter, "exit code {:?}", finished.code)?,
            Err(failure) => write!(formatter, "failure {failure:?}")?,
        }
        write!(formatter, "; recover the retained child before proceeding")
    }
}

impl std::error::Error for RunCleanupFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cleanup)
    }
}

const RUN_POLL: std::time::Duration = std::time::Duration::from_millis(20);

/// Runs `command` as an [`OwnedChild`] with no input and capped outputs.
/// The original deadline and stop apply through pipe draining, including after
/// normal exit. Cleanup precedes that drain on every exit; reaping gets at most
/// one existing poll interval and uncertainty retains ownership in the error.
pub fn run_to_end(
    command: &mut Command,
    deadline: std::time::Duration,
    stop: &std::sync::atomic::AtomicBool,
) -> Result<Finished, RunFailure> {
    use std::process::Stdio;
    use std::time::Instant;
    let end = Instant::now().checked_add(deadline).ok_or_else(|| {
        RunFailure::Start(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid run deadline",
        ))
    })?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = OwnedChild::spawn(command).map_err(RunFailure::Start)?;
    child.bounded_drop = true;
    let outcome = capture_run(&mut child, end, stop);
    let cleanup = child.kill_tree().and_then(|_| {
        let reap_end = Instant::now() + RUN_POLL;
        loop {
            match child.try_wait()? {
                Some(_) => return Ok(()),
                None if Instant::now() >= reap_end => {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "child exit was not confirmed within the cleanup bound",
                    ));
                }
                None => std::thread::sleep(
                    reap_end
                        .saturating_duration_since(Instant::now())
                        .min(RUN_POLL),
                ),
            }
        }
    });
    match cleanup {
        Ok(()) => outcome,
        Err(cleanup) => Err(RunFailure::Wait(io::Error::other(RunCleanupFailure {
            outcome,
            cleanup,
            child,
        }))),
    }
}

fn capture_run(
    child: &mut OwnedChild,
    end: std::time::Instant,
    stop: &std::sync::atomic::AtomicBool,
) -> Result<Finished, RunFailure> {
    use std::sync::atomic::Ordering;
    use std::time::Instant;
    let mut stdout = child
        .take_stdout()
        .ok_or_else(|| RunFailure::Wait(io::Error::other("missing stdout pipe")))?;
    let mut stderr = child
        .take_stderr()
        .ok_or_else(|| RunFailure::Wait(io::Error::other("missing stderr pipe")))?;
    sys::nonblocking(&stdout).map_err(RunFailure::Wait)?;
    sys::nonblocking(&stderr).map_err(RunFailure::Wait)?;
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let (mut out_end, mut err_end) = (false, false);
    let mut status = None;
    loop {
        if status.is_none() && sys::has_exited(&child.child).map_err(RunFailure::Wait)? {
            // Observe without reaping: the leader's identity still belongs to
            // this owner while its group and remaining descendants are ended.
            child.kill_tree().map_err(RunFailure::Wait)?;
            status = Some(child.try_wait().map_err(RunFailure::Wait)?.ok_or_else(|| {
                RunFailure::Wait(io::Error::other("observed exit could not be reaped"))
            })?);
        }
        if stop.load(Ordering::Relaxed) {
            return Err(RunFailure::Stopped);
        }
        if Instant::now() >= end {
            return Err(RunFailure::TimedOut);
        }
        let out_read =
            drain_available(&mut stdout, &mut out, &mut out_end).map_err(RunFailure::Wait)?;
        let err_read =
            drain_available(&mut stderr, &mut err, &mut err_end).map_err(RunFailure::Wait)?;
        if let Some(status) = status.filter(|_| out_end && err_end) {
            return Ok(Finished {
                code: status.code(),
                stdout: String::from_utf8_lossy(&out).into_owned(),
                stderr: String::from_utf8_lossy(&err).into_owned(),
            });
        }
        if !out_read && !err_read {
            std::thread::sleep(end.saturating_duration_since(Instant::now()).min(RUN_POLL));
        }
    }
}

fn drain_available<P: io::Read + sys::PipeHandle>(
    stream: &mut P,
    kept: &mut Vec<u8>,
    ended: &mut bool,
) -> io::Result<bool> {
    if *ended {
        return Ok(false);
    }
    let mut buffer = [0u8; 8192];
    match sys::read_available(stream, &mut buffer)? {
        Some(0) => {
            *ended = true;
            Ok(false)
        }
        Some(read) => {
            let room = RUN_OUTPUT_CAP.saturating_sub(kept.len());
            kept.extend_from_slice(&buffer[..read.min(room)]);
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Makes `command` start a child that no signal sent to this process's group,
/// terminal or console reaches, and that holds none of this process's
/// standard handles: a daemon whose lifetime is its own, and whose start
/// leaves a caller reading this process's output to see that output end when
/// this process does. Nothing owns the child afterwards.
///
/// Windows hands a child every inheritable handle of its parent, and this
/// process's standard handles are inheritable whenever its own parent passed
/// them in (the pipe a caller reads), so there they are made uninheritable
/// first. A child started later with [`std::process::Stdio::inherit`] still
/// gets them: the standard library hands it an inheritable copy.
pub fn detach(command: &mut Command) -> io::Result<()> {
    sys::detach(command)
}

/// What the process cap is made of: how many processes run under one pid and
/// how much memory they hold.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TreeMeasure {
    /// Every process under the pid, transitively; the pid itself is not
    /// counted.
    pub descendants: usize,
    /// Resident memory summed over the pid and all of its descendants.
    pub rss_bytes: u64,
}

fn checked_pid(pid: u32) -> io::Result<u32> {
    if pid <= 1 || pid > i32::MAX as u32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{pid} names no process that may be signalled"),
        ));
    }
    Ok(pid)
}

/// Ends `pid` and every process under it, and on Unix the group `pid` leads,
/// which reaches members whose parent has already exited. Windows keeps no
/// group, so a descendant whose parent has exited is out of reach there; an
/// owner that must end one uses [`OwnedChild`]. A process that is already gone
/// is not an error: the result a caller wants holds.
pub fn kill_tree(pid: u32) -> io::Result<()> {
    let pid = checked_pid(pid)?;
    sys::kill_tree(pid)
}

/// Asks `pid` to end, as politely as the system allows: `SIGTERM` on Unix.
/// Windows has no polite form, so there the process ends at once.
pub fn terminate(pid: u32) -> io::Result<()> {
    sys::terminate(checked_pid(pid)?)
}

/// `SIGTERM` to every member of the process group `leader` leads, which is how
/// a terminal's foreground job is ended. `Unsupported` on Windows.
pub fn terminate_group(leader: u32) -> io::Result<()> {
    sys::terminate_group(checked_pid(leader)?)
}

/// Whether a process with this pid exists. A process owned by another account
/// counts: it exists.
pub fn is_alive(pid: u32) -> bool {
    pid != 0 && sys::is_alive(pid)
}

/// The pid of the process that started `pid`. `NotFound` when `pid` does not
/// exist; on Windows also when its parent has exited, because the system keeps
/// only the parent's old pid and another process may hold it now.
pub fn parent_of(pid: u32) -> io::Result<u32> {
    sys::parent_of(pid)
}

/// Whether `pid` is `ancestor` or one of its descendants, by walking the
/// parent column up from `pid`. A walk that meets a process that no longer
/// exists, the root of the tree, or more than [`MAX_DEPTH`] parents answers
/// `false`: an ancestry that cannot be shown is not claimed.
pub fn descends_from(pid: u32, ancestor: u32) -> bool {
    let mut current = pid;
    for _ in 0..=MAX_DEPTH {
        if current == ancestor {
            return true;
        }
        if current <= 1 {
            return false;
        }
        match parent_of(current) {
            Ok(parent) if parent != current => current = parent,
            _ => return false,
        }
    }
    false
}

/// When `pid` started, as a number only comparable with another start time of
/// the same system. `NotFound` when `pid` does not exist.
pub fn start_time(pid: u32) -> io::Result<u64> {
    sys::start_time(pid)
}

/// The directory `pid` is working in, as the kernel reports it. `NotFound`
/// when the process is gone, `PermissionDenied` when the system refuses to
/// say, `Unsupported` on Windows.
pub fn cwd_of(pid: u32) -> io::Result<PathBuf> {
    sys::cwd_of(pid)
}

/// Counts and sizes the process tree rooted at `pid`. `NotFound` when `pid`
/// does not exist.
pub fn measure_tree(pid: u32) -> io::Result<TreeMeasure> {
    let descendants = descendants(pid)?;
    let mut rss_bytes = sys::rss(pid)?;
    for member in &descendants {
        // A member that exited since the walk holds nothing.
        rss_bytes = rss_bytes.saturating_add(sys::rss(*member).unwrap_or(0));
    }
    Ok(TreeMeasure {
        descendants: descendants.len(),
        rss_bytes,
    })
}

/// Every process under `root`, parents before children.
fn descendants(root: u32) -> io::Result<Vec<u32>> {
    let mut walk = sys::Walk::new()?;
    let mut found: Vec<u32> = Vec::new();
    let mut level = vec![root];
    for _ in 0..MAX_DEPTH {
        let mut next = Vec::new();
        for parent in &level {
            for child in walk.children_of(*parent) {
                if child != root && !found.contains(&child) {
                    found.push(child);
                    next.push(child);
                }
            }
        }
        if next.is_empty() {
            break;
        }
        level = next;
    }
    Ok(found)
}

#[cfg(unix)]
mod sys {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::net::UnixStream;
    use std::os::unix::process::CommandExt;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    /// A group needs no handle: its id is the leader's pid.
    pub(super) struct Tie {
        // The child's dedicated reader gets EOF if the owner crashes.
        _owner: Option<UnixStream>,
    }

    pub(super) fn spawn(
        command: &mut Command,
        deadline: Option<Instant>,
    ) -> io::Result<(Child, Tie)> {
        for key in OWNER_LAUNCH_KEYS {
            command.env_remove(key);
        }
        command.process_group(0);
        let Some(deadline) = deadline else {
            return command.spawn().map(|child| (child, Tie { _owner: None }));
        };
        deadline
            .checked_duration_since(Instant::now())
            .filter(|value| !value.is_zero())
            .ok_or_else(|| io::Error::from(io::ErrorKind::TimedOut))?;
        let (reader, mut owner) = UnixStream::pair()?;
        let descriptor = reader.as_raw_fd();
        command.env(OWNER_LAUNCH_KEYS[0], descriptor.to_string());
        // SAFETY: only async-signal-safe fcntl runs after fork. The descriptor
        // lives through spawn; guarded spawn consumes Command, so the closure
        // cannot later act on a recycled descriptor in a reused Command.
        unsafe {
            command.pre_exec(move || {
                let flags = libc::fcntl(descriptor, libc::F_GETFD);
                if flags < 0
                    || libc::fcntl(descriptor, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0
                {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        drop(reader);
        let mut ready = [0u8; 1];
        let acknowledged = (|| {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .filter(|value| !value.is_zero())
                .ok_or_else(|| io::Error::from(io::ErrorKind::TimedOut))?;
            owner.set_read_timeout(Some(remaining))?;
            owner.read_exact(&mut ready)
        })()
        .and_then(|()| {
            if ready == *b"R" && Instant::now() < deadline {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "owner watch not acknowledged",
                ))
            }
        });
        if let Err(source) = acknowledged {
            let tie = Tie {
                _owner: Some(owner),
            };
            let owned = OwnedChild {
                child,
                tie,
                ended: false,
                reaped: false,
                released: false,
                bounded_drop: true,
            };
            return Err(failed_guarded_spawn(source, owned, deadline));
        }
        Ok((
            child,
            Tie {
                _owner: Some(owner),
            },
        ))
    }

    static WATCH_OPEN: AtomicBool = AtomicBool::new(false);

    pub(super) fn watch_closed() {
        WATCH_OPEN.store(false, Ordering::SeqCst);
    }

    pub(super) fn watch_owner(value: std::ffi::OsString) -> io::Result<OwnerWatch> {
        use std::io::Write;
        let descriptor = value
            .to_str()
            .and_then(|v| v.parse::<libc::c_int>().ok())
            .filter(|v| *v >= 3)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid owner launch descriptor",
                )
            })?;
        let mut kind = 0;
        let mut length = std::mem::size_of_val(&kind) as libc::socklen_t;
        // SAFETY: output pointers refer to live locals, and descriptor is
        // checked as a stream socket before ownership is taken.
        if unsafe {
            libc::getsockopt(
                descriptor,
                libc::SOL_SOCKET,
                libc::SO_TYPE,
                (&mut kind as *mut libc::c_int).cast(),
                &mut length,
            )
        } < 0
            || kind != libc::SOCK_STREAM
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "owner descriptor is not a stream socket",
            ));
        }
        // SAFETY: fcntl acts only on the validated inherited descriptor.
        let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFD) };
        if flags < 0
            || unsafe { libc::fcntl(descriptor, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0
        {
            return Err(io::Error::last_os_error());
        }
        if WATCH_OPEN
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "one owner watch is already active",
            ));
        }
        // SAFETY: guarded launch reserved this descriptor for this receiver.
        // This is the only owner; downstream execs do not inherit it.
        let channel = unsafe { UnixStream::from_raw_fd(descriptor) };
        let cancelled = std::sync::Arc::new(AtomicBool::new(false));
        let mut stream = match channel.try_clone() {
            Ok(stream) => stream,
            Err(source) => {
                watch_closed();
                return Err(source);
            }
        };
        // A failed socket shutdown must not make normal watch teardown wait
        // forever. This timeout is cancellation polling, not a launch delay.
        if let Err(source) = stream.set_read_timeout(Some(Duration::from_millis(25))) {
            watch_closed();
            return Err(source);
        }
        let stopping = cancelled.clone();
        let reader = match std::thread::Builder::new().name("owned-child-watch".into()).spawn(move || {
            let mut byte = [0u8; 1];
            loop {
                let result = stream.read(&mut byte);
                if stopping.load(Ordering::SeqCst) { return; }
                if matches!(&result, Err(source) if matches!(source.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut)) {
                    continue;
                }
                // EOF or a channel failure cannot leave a live unguarded child.
                if let Err(source) = super::kill_tree(std::process::id()) {
                    eprintln!("process.owner_lost_cleanup_failed error={source}");
                }
                // SAFETY: this process's owner is gone; a failed kill must
                // still stop this cooperative child rather than continue.
                unsafe { libc::_exit(125) };
            }
        }) {
            Ok(reader) => reader,
            Err(source) => { watch_closed(); return Err(source); }
        };
        let mut watch = OwnerWatch {
            channel,
            cancelled,
            reader: Some(reader),
        };
        watch.channel.write_all(b"R")?;
        Ok(watch)
    }

    pub(super) use std::os::fd::AsRawFd as PipeHandle;

    pub(super) fn nonblocking(pipe: &impl PipeHandle) -> io::Result<()> {
        let descriptor = pipe.as_raw_fd();
        // SAFETY: the descriptor is borrowed and open for the operation.
        let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
        if flags < 0
            || unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
        {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub(super) fn read_available<P: Read + PipeHandle>(
        pipe: &mut P,
        bytes: &mut [u8],
    ) -> io::Result<Option<usize>> {
        match pipe.read(bytes) {
            Err(source)
                if matches!(
                    source.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(None)
            }
            read => read.map(Some),
        }
    }

    pub(super) fn has_exited(child: &Child) -> io::Result<bool> {
        // SAFETY: a zeroed siginfo_t is valid writable storage. WNOWAIT keeps
        // the leader unreaped so cleanup can still identify its descendants.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        if unsafe {
            libc::waitid(
                libc::P_PID,
                child.id() as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        #[cfg(target_os = "macos")]
        return Ok(info.si_pid != 0);
        #[cfg(target_os = "linux")]
        // SAFETY: waitid initialized siginfo_t above.
        return Ok(unsafe { info.si_pid() } != 0);
    }

    pub(super) fn detach(command: &mut Command) -> io::Result<()> {
        // A child's standard streams are the ones its Command names, and the
        // standard library opens every other descriptor close-on-exec.
        command.process_group(0);
        Ok(())
    }

    /// Signals `target` (a pid, or a negative group id), where the process
    /// having gone already is an answer, not a failure.
    fn signal(target: libc::pid_t, signal: libc::c_int) -> io::Result<()> {
        // SAFETY: `kill` takes plain integers and has no memory effects.
        if unsafe { libc::kill(target, signal) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::ESRCH) => Err(io::ErrorKind::NotFound.into()),
            _ => Err(error),
        }
    }

    pub(super) fn release(_: &Tie) {}

    pub(super) fn kill_tree_of(child: &mut Child, _: &Tie, reaped: bool) -> io::Result<bool> {
        let leader = child.id();
        // Members that left the group (a `setsid` helper) are reached only by
        // walking the table, and the walk comes before the first signal:
        // ending a parent hands its children to init. A reaped child's id may
        // be another process's, so nothing is walked for it.
        let members = if reaped {
            Vec::new()
        } else {
            descendants(leader).unwrap_or_default()
        };
        // The group is the child's own, so this reaches only what the child
        // started, even once the child has exited (an unreaped leader, or any
        // living member, keeps the group id from being reused).
        let mut ended = match signal(-(leader as libc::pid_t), libc::SIGKILL) {
            Ok(()) => true,
            // The group is this account's own, so a refusal means only
            // zombies are left in it, which macOS reports as `EPERM`.
            Err(error) if error.raw_os_error() == Some(libc::EPERM) => false,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // No group left: the child is gone or never led one.
                if child.try_wait()?.is_some() {
                    false
                } else {
                    child.kill()?;
                    true
                }
            }
            Err(error) => return Err(error),
        };
        for member in members {
            ended |= signal(member as libc::pid_t, libc::SIGKILL).is_ok();
        }
        Ok(ended)
    }

    pub(super) fn kill_tree(pid: u32) -> io::Result<()> {
        // Read the tree before the first kill: ending a parent hands its
        // children to init, out of the walk's reach.
        let members = descendants(pid).unwrap_or_default();
        let mut first_error = None;
        let mut end = |target: u32| match signal(target as libc::pid_t, libc::SIGKILL) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                first_error.get_or_insert(error);
            }
        };
        // The pid last: when it is this process's own, nothing after it runs.
        for member in members {
            end(member);
        }
        // Signal descendants before the group: when pid is this process,
        // killing its group first would prevent it from ending helpers that
        // left that group. The group also reaches reparented members.
        let _ = signal(-(pid as libc::pid_t), libc::SIGKILL);
        end(pid);
        first_error.map_or(Ok(()), Err)
    }

    pub(super) fn terminate(pid: u32) -> io::Result<()> {
        signal(pid as libc::pid_t, libc::SIGTERM)
    }

    pub(super) fn terminate_group(leader: u32) -> io::Result<()> {
        signal(-(leader as libc::pid_t), libc::SIGTERM)
    }

    pub(super) fn is_alive(pid: u32) -> bool {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return false;
        };
        // SAFETY: signal 0 only checks that the process exists and may be
        // signalled.
        if unsafe { libc::kill(pid, 0) } == 0 {
            return true;
        }
        // `EPERM` is a process of another account: it exists.
        io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }

    #[cfg(target_os = "macos")]
    mod mac {
        use super::*;

        pub(in super::super) fn error_for_missing_info() -> io::Error {
            let error = io::Error::last_os_error();
            match error.raw_os_error() {
                Some(libc::ESRCH) | Some(0) | None => io::ErrorKind::NotFound.into(),
                Some(libc::EPERM) | Some(libc::EACCES) => io::ErrorKind::PermissionDenied.into(),
                _ => error,
            }
        }

        pub(in super::super) fn bsd_info(pid: u32) -> io::Result<libc::proc_bsdinfo> {
            // SAFETY: an all-zero `proc_bsdinfo` is a valid value.
            let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
            let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
            // SAFETY: the buffer is valid for `size` bytes and read only after
            // `proc_pidinfo` reports that it filled the whole structure.
            let written = unsafe {
                libc::proc_pidinfo(
                    pid as libc::c_int,
                    libc::PROC_PIDTBSDINFO,
                    0,
                    (&mut info as *mut libc::proc_bsdinfo).cast(),
                    size,
                )
            };
            if written == size {
                Ok(info)
            } else {
                Err(error_for_missing_info())
            }
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn parent_of(pid: u32) -> io::Result<u32> {
        mac::bsd_info(pid).map(|info| info.pbi_ppid)
    }

    #[cfg(target_os = "macos")]
    pub(super) fn start_time(pid: u32) -> io::Result<u64> {
        mac::bsd_info(pid).map(|info| info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
    }

    #[cfg(target_os = "macos")]
    pub(super) fn cwd_of(pid: u32) -> io::Result<PathBuf> {
        use std::os::unix::ffi::OsStringExt;

        // SAFETY: an all-zero `proc_vnodepathinfo` is a valid value.
        let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
        // SAFETY: the buffer is valid for `size` bytes and read only after
        // `proc_pidinfo` reports that it filled the whole structure.
        let written = unsafe {
            libc::proc_pidinfo(
                pid as libc::c_int,
                libc::PROC_PIDVNODEPATHINFO,
                0,
                (&mut info as *mut libc::proc_vnodepathinfo).cast(),
                size,
            )
        };
        if written != size {
            return Err(mac::error_for_missing_info());
        }
        // libc declares the MAXPATHLEN buffer as 32 rows of 32 for old
        // compilers; the path is the NUL-terminated prefix of the flattened
        // bytes.
        let bytes: Vec<u8> = info
            .pvi_cdir
            .vip_path
            .iter()
            .flatten()
            .map(|&byte| byte as u8)
            .take_while(|&byte| byte != 0)
            .collect();
        if bytes.is_empty() {
            return Err(io::ErrorKind::NotFound.into());
        }
        Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
    }

    #[cfg(target_os = "macos")]
    pub(super) fn rss(pid: u32) -> io::Result<u64> {
        // SAFETY: an all-zero `proc_taskinfo` is a valid value.
        let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
        // SAFETY: `proc_pidinfo` writes at most `size` bytes into a value we
        // own, and reports how many it wrote.
        let written = unsafe {
            libc::proc_pidinfo(
                pid as libc::c_int,
                libc::PROC_PIDTASKINFO,
                0,
                (&mut info as *mut libc::proc_taskinfo).cast(),
                size,
            )
        };
        if written == size {
            Ok(info.pti_resident_size)
        } else {
            Err(mac::error_for_missing_info())
        }
    }

    /// The kernel answers "who are the children of this pid" directly, so a
    /// walk asks for each level and reads no table.
    #[cfg(target_os = "macos")]
    pub(super) struct Walk;

    #[cfg(target_os = "macos")]
    impl Walk {
        pub(super) fn new() -> io::Result<Self> {
            Ok(Self)
        }

        pub(super) fn children_of(&mut self, pid: u32) -> Vec<u32> {
            // `proc_listchildpids` takes its buffer size in bytes but returns
            // a count of pids, both for the null-buffer size query and for
            // the filled buffer (libproc divides by `sizeof(int)` before
            // returning). That differs from `proc_listpids`, which returns
            // bytes. Reading the count as bytes and dividing again made three
            // children measure as zero (2026-09-20).
            // SAFETY: the first call asks for the size with a null buffer;
            // the second writes at most `capacity` ints into a buffer we own.
            // Both are the documented `proc_listchildpids` contract.
            unsafe {
                let needed = libc::proc_listchildpids(pid as libc::c_int, std::ptr::null_mut(), 0);
                if needed <= 0 {
                    return Vec::new();
                }
                // Slack for children that appear between the two calls.
                let capacity = needed as usize + 16;
                let mut buffer = vec![0i32; capacity];
                let byte_len = (capacity * std::mem::size_of::<i32>()) as libc::c_int;
                let written = libc::proc_listchildpids(
                    pid as libc::c_int,
                    buffer.as_mut_ptr().cast(),
                    byte_len,
                );
                if written <= 0 {
                    return Vec::new();
                }
                buffer.truncate((written as usize).min(capacity));
                buffer
                    .into_iter()
                    .filter(|child| *child > 0)
                    .map(|child| child as u32)
                    .collect()
            }
        }
    }

    #[cfg(target_os = "linux")]
    mod linux {
        use super::*;

        /// The fields of `/proc/<pid>/stat` after the command name, which may
        /// itself contain spaces and parentheses (hence the last `) `).
        pub(in super::super) fn fields(pid: u32) -> io::Result<Vec<String>> {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).map_err(|error| {
                // A process that exits during the read answers `ESRCH`, which
                // Rust does not name.
                if error.raw_os_error() == Some(libc::ESRCH) {
                    io::ErrorKind::NotFound.into()
                } else {
                    error
                }
            })?;
            let (_, rest) = stat
                .rsplit_once(") ")
                .ok_or_else(|| io::Error::other("unreadable process status"))?;
            Ok(rest.split_whitespace().map(str::to_owned).collect())
        }

        pub(in super::super) fn field<T: std::str::FromStr>(
            fields: &[String],
            index: usize,
        ) -> io::Result<T> {
            fields
                .get(index)
                .and_then(|value| value.parse().ok())
                .ok_or_else(|| io::Error::other("unreadable process status"))
        }
    }

    #[cfg(target_os = "linux")]
    pub(super) fn parent_of(pid: u32) -> io::Result<u32> {
        linux::field(&linux::fields(pid)?, 1)
    }

    #[cfg(target_os = "linux")]
    pub(super) fn start_time(pid: u32) -> io::Result<u64> {
        linux::field(&linux::fields(pid)?, 19)
    }

    #[cfg(target_os = "linux")]
    pub(super) fn cwd_of(pid: u32) -> io::Result<PathBuf> {
        std::fs::read_link(format!("/proc/{pid}/cwd"))
    }

    #[cfg(target_os = "linux")]
    pub(super) fn rss(pid: u32) -> io::Result<u64> {
        let pages: u64 = linux::field(&linux::fields(pid)?, 21)?;
        // SAFETY: `sysconf` takes a plain integer and has no memory effects.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        Ok(pages.saturating_mul(u64::try_from(page).unwrap_or(4096)))
    }

    /// `/proc` lists every process once; one pass builds the parent table a
    /// walk then reads from.
    #[cfg(target_os = "linux")]
    pub(super) struct Walk {
        children: std::collections::HashMap<u32, Vec<u32>>,
    }

    #[cfg(target_os = "linux")]
    impl Walk {
        pub(super) fn new() -> io::Result<Self> {
            let mut children: std::collections::HashMap<u32, Vec<u32>> =
                std::collections::HashMap::new();
            for entry in std::fs::read_dir("/proc")? {
                let Ok(entry) = entry else { continue };
                let Some(pid) = entry
                    .file_name()
                    .to_str()
                    .and_then(|name| name.parse::<u32>().ok())
                else {
                    continue;
                };
                // A process that exits during the pass is simply not listed.
                if let Ok(parent) = parent_of(pid) {
                    children.entry(parent).or_default().push(pid);
                }
            }
            Ok(Self { children })
        }

        pub(super) fn children_of(&mut self, pid: u32) -> Vec<u32> {
            self.children.get(&pid).cloned().unwrap_or_default()
        }
    }
}

#[cfg(windows)]
mod sys {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, FILETIME, GetLastError, HANDLE, HANDLE_FLAG_INHERIT,
        INVALID_HANDLE_VALUE, STILL_ACTIVE, SetHandleInformation, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::System::Console::{
        GetConsoleCP, GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicAccountingInformation,
        JobObjectExtendedLimitInformation, OpenJobObjectW, QueryInformationJobObject,
        SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::Pipes::PeekNamedPipe;
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::SystemServices::JOB_OBJECT_QUERY;
    use windows_sys::Win32::System::Threading::{
        CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, CREATE_SUSPENDED, DETACHED_PROCESS,
        GetCurrentProcess, GetExitCodeProcess, GetProcessTimes, OpenProcess, OpenThread,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, ResumeThread, THREAD_SUSPEND_RESUME,
        TerminateProcess, WaitForSingleObject,
    };

    use super::*;

    /// A handle that closes when dropped.
    struct Owned(HANDLE);

    // SAFETY: a kernel handle is a plain number that any thread may use.
    unsafe impl Send for Owned {}
    // SAFETY: as above; every use is a call the kernel serialises.
    unsafe impl Sync for Owned {}

    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: the handle is open and owned by this value.
            unsafe { CloseHandle(self.0) };
        }
    }

    fn open(pid: u32, access: u32) -> io::Result<Owned> {
        // SAFETY: a plain call with integers; a null result is the failure.
        let handle = unsafe { OpenProcess(access, 0, pid) };
        if handle.is_null() {
            let error = io::Error::last_os_error();
            // ERROR_INVALID_PARAMETER is what a pid that does not exist gets.
            return Err(if error.raw_os_error() == Some(87) {
                io::ErrorKind::NotFound.into()
            } else {
                error
            });
        }
        Ok(Owned(handle))
    }

    /// Like [`open`], but a process that has exited is `NotFound` even while
    /// some handle keeps its object alive: it is gone for every purpose of the
    /// caller, as a reaped Unix process is.
    fn open_live(pid: u32, access: u32) -> io::Result<Owned> {
        let process = open(pid, access | PROCESS_QUERY_LIMITED_INFORMATION)?;
        let mut code = 0u32;
        // SAFETY: the handle is open and `code` is writable.
        let read = unsafe { GetExitCodeProcess(process.0, &mut code) };
        if read != 0 && code != STILL_ACTIVE as u32 {
            return Err(io::ErrorKind::NotFound.into());
        }
        Ok(process)
    }

    /// The job a child lives in. Closing it ends everything still in it.
    pub(super) struct Tie(Owned, bool);

    pub(super) fn spawn(
        command: &mut Command,
        deadline: Option<Instant>,
    ) -> io::Result<(Child, Tie)> {
        for key in OWNER_LAUNCH_KEYS {
            command.env_remove(key);
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(io::ErrorKind::TimedOut.into());
        }
        // SAFETY: an all-zero limit structure is a valid value.
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        static NEXT_JOB: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let name = deadline.map(|_| {
            format!(
                "Local\\hide-owner-{}-{}",
                std::process::id(),
                NEXT_JOB.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            )
        });
        let wide = name
            .as_ref()
            .map(|name| name.encode_utf16().chain(Some(0)).collect::<Vec<_>>());
        // SAFETY: attributes may be null; the optional name is terminated
        // and lives through the call. Never adopt a pre-existing named job.
        let job = unsafe {
            CreateJobObjectW(
                std::ptr::null(),
                wide.as_ref().map_or(std::ptr::null(), |name| name.as_ptr()),
            )
        };
        if job.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = Owned(job);
        // SAFETY: read immediately after successful CreateJobObjectW.
        if name.is_some() && unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        if let Some(name) = name {
            command.env(OWNER_LAUNCH_KEYS[1], name);
        }
        // SAFETY: `limits` is valid for the size passed.
        let set = unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if set == 0 {
            return Err(io::Error::last_os_error());
        }
        // Suspended, so the child cannot start a process of its own before it
        // is inside the job. An owner with no console of its own (a detached
        // daemon) would have Windows open a visible console window for every
        // console child, so such a child gets a console without a window; an
        // owner with a console shares it, as before, whether or not that
        // console has a window (a CI runner's has none).
        // SAFETY: a plain call; code page 0 means this process has no console.
        let windowless = if unsafe { GetConsoleCP() } == 0 {
            CREATE_NO_WINDOW
        } else {
            0
        };
        command.creation_flags(CREATE_NEW_PROCESS_GROUP | windowless | CREATE_SUSPENDED);
        let mut child = command.spawn()?;
        // SAFETY: both handles are open; the child's is owned by `child`.
        let assigned = unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle()) };
        if assigned == 0 {
            let error = io::Error::last_os_error();
            if let Some(deadline) = deadline {
                let owned = OwnedChild {
                    child,
                    tie: Tie(job, false),
                    ended: false,
                    reaped: false,
                    released: false,
                    bounded_drop: true,
                };
                return Err(failed_guarded_spawn(error, owned, deadline));
            }
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        if let Err(error) = resume(child.id()) {
            if let Some(deadline) = deadline {
                let owned = OwnedChild {
                    child,
                    tie: Tie(job, true),
                    ended: false,
                    reaped: false,
                    released: false,
                    bounded_drop: true,
                };
                return Err(failed_guarded_spawn(error, owned, deadline));
            }
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        let tie = Tie(job, true);
        if let Some(deadline) = deadline.filter(|deadline| Instant::now() >= *deadline) {
            let owned = OwnedChild {
                child,
                tie,
                ended: false,
                reaped: false,
                released: false,
                bounded_drop: true,
            };
            return Err(failed_guarded_spawn(
                io::ErrorKind::TimedOut.into(),
                owned,
                deadline,
            ));
        }
        Ok((child, tie))
    }

    static WATCH_OPEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    pub(super) fn watch_closed() {
        WATCH_OPEN.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    pub(super) fn watch_owner(value: std::ffi::OsString) -> io::Result<OwnerWatch> {
        let name = value
            .to_str()
            .filter(|name| name.len() <= 96)
            .and_then(|name| name.strip_prefix("Local\\hide-owner-"))
            .and_then(|suffix| suffix.split_once('-'))
            .filter(|(pid, sequence)| {
                pid.parse::<u32>().is_ok() && sequence.parse::<u64>().is_ok()
            });
        if name.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid owner launch job",
            ));
        }
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = value.encode_wide().chain(Some(0)).collect();
        // SAFETY: the validated name is terminated; the handle is not
        // inherited and is kept only for this membership query.
        let handle = unsafe { OpenJobObjectW(JOB_OBJECT_QUERY, 0, wide.as_ptr()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = Owned(handle);
        let mut member = 0;
        // SAFETY: current-process pseudo handle and query handle are valid,
        // and member is writable. An unrelated inherited CI job is no proof.
        if unsafe { IsProcessInJob(GetCurrentProcess(), job.0, &mut member) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if member == 0 {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        // SAFETY: the query-only handle is open, and the limits buffer is
        // valid for its declared size. Membership alone is not a lifetime
        // boundary if a different launcher created a job without this flag.
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe {
            QueryInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if limits.BasicLimitInformation.LimitFlags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "owner job has no lifetime boundary",
            ));
        }
        // Closing this proof handle before returning keeps the parent the
        // sole persistent handle owner: the child cannot keep the job alive.
        drop(job);
        if WATCH_OPEN
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .is_err()
        {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        Ok(OwnerWatch { active: true })
    }

    pub(super) use std::os::windows::io::AsRawHandle as PipeHandle;

    // Windows anonymous pipes cannot use Unix O_NONBLOCK. The owned reader
    // peeks first and reads no more than the bytes already in the pipe.
    pub(super) fn nonblocking(_: &impl PipeHandle) -> io::Result<()> {
        Ok(())
    }

    pub(super) fn read_available<P: Read + PipeHandle>(
        pipe: &mut P,
        bytes: &mut [u8],
    ) -> io::Result<Option<usize>> {
        let mut available = 0;
        // SAFETY: the pipe handle is borrowed and the only output is live.
        let peeked = unsafe {
            PeekNamedPipe(
                pipe.as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if peeked == 0 {
            let source = io::Error::last_os_error();
            // ERROR_BROKEN_PIPE / NO_DATA / PIPE_NOT_CONNECTED mark EOF.
            return if matches!(source.raw_os_error(), Some(109 | 232 | 233)) {
                Ok(Some(0))
            } else {
                Err(source)
            };
        }
        if available == 0 {
            return Ok(None);
        }
        let count = bytes.len().min(available as usize);
        pipe.read(&mut bytes[..count]).map(Some)
    }

    pub(super) fn has_exited(child: &Child) -> io::Result<bool> {
        // SAFETY: the process handle is owned by child. A zero wait observes
        // exit without blocking or releasing the process's identity.
        match unsafe { WaitForSingleObject(child.as_raw_handle(), 0) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }

    /// Starts the one thread a suspended process was created with.
    fn resume(pid: u32) -> io::Result<()> {
        // SAFETY: a plain call; the invalid handle value is the failure.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let snapshot = Owned(snapshot);
        // SAFETY: an all-zero entry with `dwSize` set is what the API wants.
        let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        // SAFETY: `entry` is valid for the size it declares.
        let mut more = unsafe { Thread32First(snapshot.0, &mut entry) } != 0;
        while more {
            if entry.th32OwnerProcessID == pid {
                // SAFETY: a plain call; a null result is the failure.
                let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                if thread.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let thread = Owned(thread);
                // SAFETY: the thread handle is open; `u32::MAX` is the failure.
                if unsafe { ResumeThread(thread.0) } == u32::MAX {
                    return Err(io::Error::last_os_error());
                }
                return Ok(());
            }
            // SAFETY: as for the first call.
            more = unsafe { Thread32Next(snapshot.0, &mut entry) } != 0;
        }
        Err(io::Error::other("the new process has no thread to start"))
    }

    pub(super) fn detach(command: &mut Command) -> io::Result<()> {
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
        for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            // SAFETY: a plain call; null or INVALID_HANDLE_VALUE means none.
            let handle = unsafe { GetStdHandle(which) };
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                continue;
            }
            // SAFETY: the handle is this process's own standard handle.
            if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) } == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    pub(super) fn release(tie: &Tie) {
        // SAFETY: an all-zero limit structure is a valid value: no flags.
        let limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: the job handle is open and `limits` is valid for the size
        // passed. A failure leaves the job closing on drop, which ends the tree.
        unsafe {
            SetInformationJobObject(
                (tie.0).0,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
    }

    pub(super) fn kill_tree_of(child: &mut Child, tie: &Tie, reaped: bool) -> io::Result<bool> {
        if !tie.1 {
            // Assignment failed before the suspended child could run. Its
            // process handle is still ours, but the empty job cannot end it.
            if reaped || has_exited(child)? {
                return Ok(false);
            }
            child.kill()?;
            return Ok(true);
        }
        // SAFETY: an all-zero accounting structure is a valid value.
        let mut accounting: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: the job handle is open for as long as `tie` lives and
        // `accounting` is valid for the size passed.
        let queried = unsafe {
            QueryInformationJobObject(
                (tie.0).0,
                JobObjectBasicAccountingInformation,
                (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        if queried == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: as above.
        if unsafe { TerminateJobObject((tie.0).0, 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(accounting.ActiveProcesses > 0)
    }

    pub(super) fn kill_tree(pid: u32) -> io::Result<()> {
        let members = descendants(pid).unwrap_or_default();
        let mut first_error = None;
        for target in std::iter::once(pid).chain(members) {
            match terminate(target) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    pub(super) fn terminate(pid: u32) -> io::Result<()> {
        let process = open_live(pid, PROCESS_TERMINATE)?;
        // SAFETY: the handle is open with the access the call needs.
        if unsafe { TerminateProcess(process.0, 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(super) fn terminate_group(_: u32) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "a Windows process group cannot be signalled",
        ))
    }

    pub(super) fn is_alive(pid: u32) -> bool {
        let process = match open(pid, PROCESS_QUERY_LIMITED_INFORMATION) {
            Ok(process) => process,
            // A process of another account refuses the query and exists.
            Err(error) => return error.kind() == io::ErrorKind::PermissionDenied,
        };
        let mut code = 0u32;
        // SAFETY: the handle is open and `code` is writable.
        let read = unsafe { GetExitCodeProcess(process.0, &mut code) };
        read != 0 && code == STILL_ACTIVE as u32
    }

    pub(super) fn start_time(pid: u32) -> io::Result<u64> {
        let process = open_live(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
        // SAFETY: the handle is open and every output is writable.
        let read = unsafe {
            GetProcessTimes(process.0, &mut created, &mut exited, &mut kernel, &mut user)
        };
        if read == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }

    pub(super) fn cwd_of(_: u32) -> io::Result<PathBuf> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows reports no working directory for another process",
        ))
    }

    pub(super) fn rss(pid: u32) -> io::Result<u64> {
        let process = open_live(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
        // SAFETY: an all-zero counter structure is a valid value.
        let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
        counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        // SAFETY: the handle is open and `counters` is valid for `cb` bytes.
        let read = unsafe { K32GetProcessMemoryInfo(process.0, &mut counters, counters.cb) };
        if read == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(counters.WorkingSetSize as u64)
    }

    pub(super) fn parent_of(pid: u32) -> io::Result<u32> {
        let started = start_time(pid)?;
        let table = process_table()?;
        let (_, parent) = table
            .iter()
            .find(|(candidate, _)| *candidate == pid)
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        // The table keeps the pid the parent had when the child started. A
        // parent that has exited, or whose pid another process now holds (it
        // started after the child), is not shown.
        match start_time(*parent) {
            Ok(parent_started) if parent_started <= started => Ok(*parent),
            _ => Err(io::ErrorKind::NotFound.into()),
        }
    }

    /// `(pid, parent pid)` for every process, from one snapshot.
    fn process_table() -> io::Result<Vec<(u32, u32)>> {
        // SAFETY: a plain call; the invalid handle value is the failure.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let snapshot = Owned(snapshot);
        // SAFETY: an all-zero entry with `dwSize` set is what the API wants.
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut table = Vec::new();
        // SAFETY: `entry` is valid for the size it declares.
        let mut more = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
        while more {
            table.push((entry.th32ProcessID, entry.th32ParentProcessID));
            // SAFETY: as for the first call.
            more = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
        }
        Ok(table)
    }

    /// The parent column of a Windows process table is the pid at the time the
    /// child started, and the parent may have exited and its pid been reused
    /// since: a child that started before its "parent" is not its child.
    pub(super) struct Walk {
        table: Vec<(u32, u32)>,
    }

    impl Walk {
        pub(super) fn new() -> io::Result<Self> {
            Ok(Self {
                table: process_table()?,
            })
        }

        pub(super) fn children_of(&mut self, pid: u32) -> Vec<u32> {
            let parent_started = start_time(pid).ok();
            self.table
                .iter()
                .filter(|(_, parent)| *parent == pid)
                .map(|(child, _)| *child)
                .filter(|child| match (parent_started, start_time(*child).ok()) {
                    (Some(parent), Some(child)) => child >= parent,
                    _ => true,
                })
                .collect()
        }
    }
}
