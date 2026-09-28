//! Cloning a repository into a new folder on the machine that will hold it:
//! which URLs are accepted, the folder name a URL gives, and one bounded
//! `git clone` run that leaves either the whole repository or nothing.
//!
//! The run belongs to its caller. Git leads its own process group, so a
//! cancel, a stall or the caller going away stops everything Git started
//! (`ssh`, `git-remote-https`, `index-pack`), and dropping the run's guard on
//! any exit path does the same. Git clones into a hidden staging folder beside
//! the target, and only a finished clone is renamed into place, so a cancelled
//! or failed clone leaves no half-written folder under the target's name; the
//! staging folder is removed on every exit path.
//!
//! No prompt can hold the run: Git is told never to ask on a terminal
//! (`GIT_TERMINAL_PROMPT=0`) and `ssh` runs in batch mode, and a run whose
//! Git has printed nothing for [`STALL_LIMIT`] is ended as stalled, whatever
//! the transport. A URL can carry credentials, so the URL itself is never in
//! a message this module writes; callers log [`CloneSource::host`].

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long a clone may go without Git printing anything before it is ended
/// as stalled. Git reports progress at least once a second while data
/// arrives or deltas resolve, so silence this long is a transfer that stopped.
pub const STALL_LIMIT: Duration = Duration::from_secs(120);

/// How often the run looks at a cancel request and the stall clock.
const POLL: Duration = Duration::from_millis(100);

/// The most stderr lines a failure is classified from, each capped at
/// [`LINE_CAP`] bytes, so a chatty remote cannot grow the run's memory.
const TAIL_LINES: usize = 20;
const LINE_CAP: usize = 400;

/// The transports a clone may use, as Git's `GIT_ALLOW_PROTOCOL` names them.
/// `file` is a local repository, which is how the tests clone.
const ALLOWED_PROTOCOLS: &str = "https:ssh:file";

/// `ssh` never asks for a password or a host key, and a connection that stops
/// answering is dropped by `ssh` itself.
const SSH_COMMAND: &str =
    "ssh -o BatchMode=yes -o ConnectTimeout=30 -o ServerAliveInterval=15 -o ServerAliveCountMax=4";

/// A URL Git can clone from, and the folder name it gives.
#[derive(Clone, Eq, PartialEq)]
pub struct CloneSource {
    url: String,
    host: String,
    name: String,
}

impl std::fmt::Debug for CloneSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The URL can carry a token; only its host is printed.
        f.debug_struct("CloneSource")
            .field("host", &self.host)
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl CloneSource {
    /// Accepts `https://host/path`, `ssh://[user@]host[:port]/path`, the scp
    /// form `[user@]host:path`, and `file:///path`, and names the folder the
    /// way `git clone` does: the last path segment without a trailing `.git`.
    /// Anything else is refused with the reason in plain words.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let url = raw.trim();
        if url.is_empty() {
            return Err("Enter a Git URL.".to_owned());
        }
        if url.starts_with('-') || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err("This is not a Git URL.".to_owned());
        }
        let (host, path) = if let Some(rest) = url.strip_prefix("https://") {
            split_authority(rest)?
        } else if let Some(rest) = url.strip_prefix("ssh://") {
            split_authority(rest)?
        } else if let Some(rest) = url.strip_prefix("file://") {
            if !rest.starts_with('/') {
                return Err(
                    "A file URL names an absolute path: file:///path/to/repo.git".to_owned(),
                );
            }
            ("localhost".to_owned(), rest.to_owned())
        } else if url.contains("://") {
            return Err("Use an https, ssh or git@host:path URL.".to_owned());
        } else {
            split_scp(url)?
        };
        let name = folder_name(&path)
            .ok_or_else(|| "This URL names no repository to clone.".to_owned())?;
        Ok(Self {
            url: url.to_owned(),
            host,
            name,
        })
    }

    /// The URL as given, credentials included; for Git only, never a log.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The host the URL names, without user or credentials: what a log line
    /// may carry.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The folder the clone lands in under its parent.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// `[user[:secret]@]host[:port]/path` after the scheme.
fn split_authority(rest: &str) -> Result<(String, String), String> {
    let (authority, path) = rest
        .split_once('/')
        .ok_or_else(|| "This URL names no repository to clone.".to_owned())?;
    let host_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = host_port
        .rsplit_once(':')
        .filter(|(_, port)| port.chars().all(|c| c.is_ascii_digit()))
        .map_or(host_port, |(host, _)| host);
    if host.is_empty() || !valid_host(host) {
        return Err("This URL names no host.".to_owned());
    }
    Ok((host.to_ascii_lowercase(), path.to_owned()))
}

/// The scp form `[user@]host:path`, which Git reads as ssh.
fn split_scp(url: &str) -> Result<(String, String), String> {
    let (authority, path) = url
        .split_once(':')
        .ok_or_else(|| "Use an https, ssh or git@host:path URL.".to_owned())?;
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    // A slash before the colon is a local path, which Git would read as one.
    if host.is_empty() || authority.contains('/') || !valid_host(host) {
        return Err("Use an https, ssh or git@host:path URL.".to_owned());
    }
    Ok((host.to_ascii_lowercase(), path.to_owned()))
}

fn valid_host(host: &str) -> bool {
    host.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '[' | ']' | ':'))
}

/// Git's own naming: the last segment, trailing slashes and `.git` dropped.
fn folder_name(path: &str) -> Option<String> {
    let path = path.trim_end_matches('/');
    let path = path
        .strip_suffix(".git")
        .unwrap_or(path)
        .trim_end_matches('/');
    let name = path.rsplit(['/', ':']).next()?;
    (!name.is_empty() && name != "." && name != ".." && !name.contains('\\'))
        .then(|| name.to_owned())
}

/// Where Git says it is, as the progress line the operator reads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CloneProgress {
    /// Git's own stage name: `Receiving objects`, `Resolving deltas`, ...
    pub stage: String,
    pub percent: Option<u8>,
}

/// Why a clone ended without a repository. Each reads as one sentence the
/// operator can act on; none carries the URL.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CloneFailure {
    Cancelled,
    /// Git printed nothing for [`STALL_LIMIT`].
    Stalled(Duration),
    /// The host wanted credentials Git could not find without asking.
    Authentication,
    /// `ssh` does not know the host's key yet and would not ask.
    UnknownHostKey,
    NotFound,
    UnreachableHost,
    /// Something is already at the target's name.
    TargetExists(PathBuf),
    /// Git failed otherwise; its last `fatal:` line, credentials removed.
    Git(String),
    /// The run itself could not start or finish its own work.
    Io(String),
}

impl CloneFailure {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::Stalled(_) => "stalled",
            Self::Authentication => "authentication",
            Self::UnknownHostKey => "unknown_host_key",
            Self::NotFound => "not_found",
            Self::UnreachableHost => "unreachable_host",
            Self::TargetExists(_) => "target_exists",
            Self::Git(_) => "git",
            Self::Io(_) => "io",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Cancelled => "The clone was cancelled.".to_owned(),
            Self::Stalled(limit) => format!(
                "The transfer stalled: nothing arrived for {} seconds, so the clone was stopped.",
                limit.as_secs()
            ),
            Self::Authentication => "Authentication failed: this repository needs credentials Git could not find. Set up an SSH key or a credential helper, then try again.".to_owned(),
            Self::UnknownHostKey => "This host's SSH key is not known yet. Connect to it once from a terminal to trust it, then try again.".to_owned(),
            Self::NotFound => "The repository was not found, or it needs sign-in.".to_owned(),
            Self::UnreachableHost => "The host could not be reached.".to_owned(),
            Self::TargetExists(path) => format!("{} already exists.", path.display()),
            Self::Git(line) => line.clone(),
            Self::Io(reason) => reason.clone(),
        }
    }
}

/// Clones `source` into `parent/<name>` and answers the new folder. `cancel`
/// is asked every [`POLL`]; answering true ends the run as cancelled.
/// `progress` hears each change of stage or percent.
pub fn clone_repository(
    source: &CloneSource,
    parent: &Path,
    stall_limit: Duration,
    cancel: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(CloneProgress),
) -> Result<PathBuf, CloneFailure> {
    let target = parent.join(source.name());
    if target.symlink_metadata().is_ok() {
        return Err(CloneFailure::TargetExists(target));
    }
    let staging = Staging(parent.join(format!(
        ".{}.hide-clone-{}",
        source.name(),
        std::process::id()
    )));
    // A staging folder left by a run this process never finished (it was
    // killed mid-clone) has this process's id only by reuse; it is ours to
    // replace.
    staging.remove();
    let mut command = Command::new("git");
    command
        .args(["clone", "--progress", "--"])
        .arg(source.url())
        .arg(&staging.0)
        .current_dir(parent)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("GIT_SSH_COMMAND", SSH_COMMAND)
        .env("GIT_ALLOW_PROTOCOL", ALLOWED_PROTOCOLS)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let child = command
        .spawn()
        .map_err(|error| CloneFailure::Io(format!("Git could not be started: {error}")))?;
    let mut run = Run(child);
    let lines = read_lines(run.0.stderr.take());
    let mut tail: Vec<String> = Vec::new();
    let mut last: Option<CloneProgress> = None;
    let mut heard = Instant::now();
    let status = loop {
        match lines.recv_timeout(POLL) {
            Ok(line) => {
                heard = Instant::now();
                if let Some(now) = parse_progress(&line)
                    && last.as_ref() != Some(&now)
                {
                    progress(now.clone());
                    last = Some(now);
                }
                if tail.len() == TAIL_LINES {
                    tail.remove(0);
                }
                tail.push(line);
                continue;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            // Stderr closed: Git is ending; its status follows.
            Err(mpsc::RecvTimeoutError::Disconnected) => std::thread::sleep(POLL),
        }
        if cancel() {
            return Err(CloneFailure::Cancelled);
        }
        if heard.elapsed() >= stall_limit {
            return Err(CloneFailure::Stalled(stall_limit));
        }
        match run.0.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                return Err(CloneFailure::Io(format!(
                    "Git could not be watched: {error}"
                )));
            }
        }
    };
    // What Git printed last is read before its failure is classified.
    while let Ok(line) = lines.recv_timeout(POLL) {
        if tail.len() == TAIL_LINES {
            tail.remove(0);
        }
        tail.push(line);
    }
    run.finished();
    if !status.success() {
        return Err(classify(&tail, status.code()));
    }
    rename_no_replace(&staging.0, &target).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            CloneFailure::TargetExists(target.clone())
        } else {
            CloneFailure::Io(format!("The clone could not be moved into place: {error}"))
        }
    })?;
    std::mem::forget(staging);
    Ok(target)
}

/// The running Git. Dropping it on any path that did not see Git end stops
/// Git's whole process group and reaps it.
struct Run(Child);

impl Run {
    fn finished(self) {
        std::mem::forget(self);
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = crate::worktrees::kill_group(&mut self.0);
        let _ = self.0.wait();
    }
}

/// The folder Git writes into; removed on drop unless the clone moved it into
/// place.
struct Staging(PathBuf);

impl Staging {
    fn remove(&self) {
        if self.0.symlink_metadata().is_ok() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        self.remove();
    }
}

/// Git's stderr as lines, split on `\r` too, since progress rewrites one line.
fn read_lines(pipe: Option<std::process::ChildStderr>) -> mpsc::Receiver<String> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let Some(mut pipe) = pipe else { return };
        let mut line = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let read = match pipe.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };
            for &byte in &buffer[..read] {
                if byte == b'\r' || byte == b'\n' {
                    if !line.is_empty() {
                        let text = String::from_utf8_lossy(&line).trim().to_owned();
                        line.clear();
                        if !text.is_empty() && sender.send(text).is_err() {
                            return;
                        }
                    }
                } else if line.len() < LINE_CAP {
                    line.push(byte);
                }
            }
        }
        if !line.is_empty() {
            let _ = sender.send(String::from_utf8_lossy(&line).trim().to_owned());
        }
    });
    receiver
}

/// `Receiving objects:  45% (450/1000), 1.2 MiB | 600 KiB/s` and its
/// `remote: ` forms, as a stage and a percent.
pub fn parse_progress(line: &str) -> Option<CloneProgress> {
    let line = line.strip_prefix("remote:").map_or(line, str::trim_start);
    let (stage, rest) = line.split_once(':')?;
    // Git's stages are capitalized; `fatal:`, `error:` and `warning:` are not.
    if !stage.starts_with(|c: char| c.is_ascii_uppercase())
        || stage.len() > 40
        || !stage.chars().all(|c| c.is_ascii_alphabetic() || c == ' ')
    {
        return None;
    }
    let rest = rest.trim_start();
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let percent = (!digits.is_empty() && rest[digits.len()..].starts_with('%'))
        .then(|| digits.parse::<u8>().ok())
        .flatten();
    Some(CloneProgress {
        stage: stage.to_owned(),
        percent,
    })
}

/// Git's words for a failure, read in the C locale, as the one reason the
/// operator can act on.
fn classify(tail: &[String], code: Option<i32>) -> CloneFailure {
    let said = |needle: &str| tail.iter().any(|line| line.contains(needle));
    if said("Authentication failed")
        || said("could not read Username")
        || said("could not read Password")
        || said("terminal prompts disabled")
        || said("Permission denied (publickey")
        || said("Permission denied, please try again")
    {
        return CloneFailure::Authentication;
    }
    if said("Host key verification failed") {
        return CloneFailure::UnknownHostKey;
    }
    if said("Repository not found")
        || said("does not appear to be a git repository")
        || said("not found")
    {
        return CloneFailure::NotFound;
    }
    if said("Could not resolve host")
        || said("Could not resolve hostname")
        || said("Connection refused")
        || said("Operation timed out")
    {
        return CloneFailure::UnreachableHost;
    }
    let fatal = tail
        .iter()
        .rev()
        .find_map(|line| line.strip_prefix("fatal:").map(str::trim));
    CloneFailure::Git(match fatal {
        Some(line) => redact(line),
        None => format!(
            "Git stopped with status {}.",
            code.map_or("unknown".to_owned(), |code| code.to_string())
        ),
    })
}

/// A line with any `scheme://user:secret@` credentials taken out.
pub fn redact(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find("://") {
        let (head, tail) = rest.split_at(at + 3);
        out.push_str(head);
        let end = tail
            .find(|c: char| c == '/' || c.is_whitespace() || c == '\'')
            .unwrap_or(tail.len());
        match tail[..end].rfind('@') {
            Some(credentials) => rest = &tail[credentials + 1..],
            None => rest = tail,
        }
    }
    out.push_str(rest);
    out
}

/// Moves the finished clone to its name, refusing to replace anything that
/// appeared there meanwhile, even an empty folder `rename` would replace.
fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let from = CString::new(from.as_os_str().as_bytes())?;
        let to = CString::new(to.as_os_str().as_bytes())?;
        // SAFETY: both are NUL-terminated paths that outlive the call.
        let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
        if result == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
    #[cfg(target_os = "linux")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let from = CString::new(from.as_os_str().as_bytes())?;
        let to = CString::new(to.as_os_str().as_bytes())?;
        // SAFETY: both are NUL-terminated paths that outlive the call.
        let result = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                from.as_ptr(),
                libc::AT_FDCWD,
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        if to.symlink_metadata().is_ok() {
            return Err(std::io::ErrorKind::AlreadyExists.into());
        }
        std::fs::rename(from, to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(url: &str) -> Result<(String, String), String> {
        CloneSource::parse(url).map(|source| (source.host().to_owned(), source.name().to_owned()))
    }

    #[test]
    fn accepted_urls_name_their_folder_the_way_git_does() {
        let ok = |url: &str, host: &str, folder: &str| {
            assert_eq!(name(url), Ok((host.to_owned(), folder.to_owned())), "{url}");
        };
        ok("https://github.com/user/repo.git", "github.com", "repo");
        ok("https://github.com/user/repo", "github.com", "repo");
        ok("https://github.com/user/repo/", "github.com", "repo");
        ok("  https://GitHub.com/user/repo.git\n", "github.com", "repo");
        ok(
            "https://user:token@github.com:8443/org/repo.git",
            "github.com",
            "repo",
        );
        ok("git@github.com:user/repo.git", "github.com", "repo");
        ok("git@github.com:repo.git", "github.com", "repo");
        ok("github.com:user/.dotfiles", "github.com", ".dotfiles");
        ok(
            "ssh://git@example.com:2222/srv/repo.git",
            "example.com",
            "repo",
        );
        ok("ssh://example.com/repo.git/", "example.com", "repo");
        ok("file:///tmp/fixtures/origin.git", "localhost", "origin");
    }

    #[test]
    fn refused_urls_say_why() {
        for url in [
            "",
            "   ",
            "http://example.com/repo.git",
            "git://example.com/repo.git",
            "ext::sh -c touch% /tmp/pwned",
            "-uhttps://example.com/x",
            "https://github.com",
            "https://github.com/",
            "https:///repo.git",
            "/home/me/repo",
            "./repo",
            "repo",
            "git@github.com:",
            "git@github.com:.git",
            "file://relative/repo",
            "https://github.com/user/repo name",
        ] {
            assert!(CloneSource::parse(url).is_err(), "{url:?} was accepted");
        }
    }

    #[test]
    fn debug_never_prints_the_url() {
        let source = CloneSource::parse("https://user:s3cret@github.com/org/repo.git").unwrap();
        let printed = format!("{source:?}");
        assert!(!printed.contains("s3cret"), "{printed}");
        assert!(printed.contains("github.com"), "{printed}");
    }

    #[test]
    fn progress_lines_read_as_stage_and_percent() {
        assert_eq!(
            parse_progress("Receiving objects:  45% (450/1000), 1.20 MiB | 600.00 KiB/s"),
            Some(CloneProgress {
                stage: "Receiving objects".into(),
                percent: Some(45)
            })
        );
        assert_eq!(
            parse_progress("remote: Counting objects: 100% (5/5), done."),
            Some(CloneProgress {
                stage: "Counting objects".into(),
                percent: Some(100)
            })
        );
        assert_eq!(
            parse_progress("remote: Enumerating objects: 5, done."),
            Some(CloneProgress {
                stage: "Enumerating objects".into(),
                percent: None
            })
        );
        assert_eq!(parse_progress("Cloning into '/tmp/x'..."), None);
        assert_eq!(parse_progress("fatal: repository 'x' not found"), None);
        assert_eq!(
            parse_progress("warning: redirecting to https://example.com/r.git/"),
            None
        );
    }

    #[test]
    fn failures_read_in_plain_words_without_credentials() {
        let tail = |lines: &[&str]| {
            lines
                .iter()
                .map(|line| (*line).to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            classify(
                &tail(&[
                    "fatal: could not read Username for 'https://github.com': terminal prompts disabled"
                ]),
                Some(128)
            ),
            CloneFailure::Authentication
        );
        assert_eq!(
            classify(
                &tail(&[
                    "git@github.com: Permission denied (publickey).",
                    "fatal: Could not read from remote repository."
                ]),
                Some(128)
            ),
            CloneFailure::Authentication
        );
        assert_eq!(
            classify(&tail(&["Host key verification failed."]), Some(128)),
            CloneFailure::UnknownHostKey
        );
        assert_eq!(
            classify(&tail(&["remote: Repository not found."]), Some(128)),
            CloneFailure::NotFound
        );
        let other = classify(
            &tail(&[
                "fatal: unable to access 'https://me:tok@example.com/r.git/': SSL certificate problem",
            ]),
            Some(128),
        );
        assert_eq!(
            other,
            CloneFailure::Git(
                "unable to access 'https://example.com/r.git/': SSL certificate problem".into()
            )
        );
        assert_eq!(
            classify(&[], Some(1)),
            CloneFailure::Git("Git stopped with status 1.".into())
        );
    }

    #[test]
    fn redact_takes_out_every_credential() {
        assert_eq!(
            redact("https://a:b@h/x and ssh://u@h2/y"),
            "https://h/x and ssh://h2/y"
        );
        assert_eq!(redact("no url here"), "no url here");
    }
}
