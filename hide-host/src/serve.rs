//! The helper's request loop: JSON lines in, JSON lines out, until the input
//! ends. The input is the SSH channel, so the helper lives exactly as long as
//! the connection that started it (PRD S5.5 D-20).

use std::io::{self, BufRead, Read, Write};
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::error::{ErrorCode, HostError, HostResult};
use crate::protocol::{
    Call, Hello, Outcome, PROTOCOL_VERSION, Request, Response, RevisionNow, RootOpened, RootRef,
};
use crate::root::{Root, relative_path};
use crate::{bytes, document, git, index, list, mutate, save, worktrees};

/// Requests the helper works on at once; the core also admits at most this
/// many per device, so the helper never queues behind itself.
pub const CONCURRENCY: usize = 4;

/// A request line longer than this is refused without being parsed: a save
/// carries at most the 16 MiB editable size, as JSON-escaped text.
const MAX_REQUEST_BYTES: usize = 40 * 1024 * 1024;

pub fn serve(input: impl BufRead, output: impl Write + Send) -> io::Result<()> {
    let output = Mutex::new(output);
    let (sender, receiver) = mpsc::sync_channel::<Request>(0);
    // Only the workers hold the receiver. A worker stops when its answer
    // cannot be written, which means the SSH channel is gone; once the last
    // one has stopped, the next request's send fails and the helper exits,
    // rather than waiting on a rendezvous no worker will ever take.
    let receiver = Arc::new(Mutex::new(receiver));
    std::thread::scope(|scope| {
        for _ in 0..CONCURRENCY {
            let receiver = Arc::clone(&receiver);
            let output = &output;
            scope.spawn(move || {
                loop {
                    let request = match receiver.lock().map(|receiver| receiver.recv()) {
                        Ok(Ok(request)) => request,
                        _ => return,
                    };
                    let response = Response {
                        id: request.id,
                        outcome: match handle(request.call) {
                            Ok(value) => Outcome::Ok(value),
                            Err(error) => Outcome::Error(error),
                        },
                    };
                    if write_line(output, &response).is_err() {
                        return;
                    }
                }
            });
        }
        drop(receiver);
        let result = read_requests(input, &sender, &output);
        drop(sender);
        // The connection is gone: a kit step still running ends its child
        // rather than keep the helper alive after it.
        crate::kit::stop();
        result
    })
}

fn read_requests(
    mut input: impl BufRead,
    sender: &mpsc::SyncSender<Request>,
    output: &Mutex<impl Write>,
) -> io::Result<()> {
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = input
            .by_ref()
            .take(MAX_REQUEST_BYTES as u64 + 1)
            .read_until(b'\n', &mut line)?;
        if read == 0 {
            return Ok(());
        }
        if line.len() > MAX_REQUEST_BYTES {
            // The rest of the line cannot be framed; the channel is unusable.
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request line too long",
            ));
        }
        let request: Request = match serde_json::from_slice(&line) {
            Ok(request) => request,
            Err(error) => {
                let id = serde_json::from_slice::<Value>(&line)
                    .ok()
                    .and_then(|value| value.get("id").and_then(Value::as_u64))
                    .unwrap_or(0);
                write_line(
                    output,
                    &Response {
                        id,
                        outcome: Outcome::Error(HostError::new(
                            ErrorCode::InvalidRequest,
                            format!("The helper could not read the request: {error}"),
                        )),
                    },
                )?;
                continue;
            }
        };
        if sender.send(request).is_err() {
            return Ok(());
        }
    }
}

fn write_line(output: &Mutex<impl Write>, response: &Response) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(response).map_err(io::Error::other)?;
    bytes.push(b'\n');
    let mut output = output
        .lock()
        .map_err(|_| io::Error::other("helper output lock poisoned"))?;
    output.write_all(&bytes)?;
    output.flush()
}

fn to_value(value: impl serde::Serialize) -> HostResult<Value> {
    serde_json::to_value(value).map_err(|error| {
        HostError::new(
            ErrorCode::Io,
            format!("The answer could not be encoded: {error}"),
        )
    })
}

/// A repository path from the core. Only an absolute path names a folder on
/// this machine; anything else is refused before Git sees it.
fn absolute(path: &str) -> HostResult<std::path::PathBuf> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err(HostError::new(
            ErrorCode::InvalidPath,
            "A repository path must be absolute",
        ));
    }
    Ok(path.to_path_buf())
}

fn project_facts(path: &str) -> HostResult<hide_project::ProjectFacts> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err(HostError::new(
            ErrorCode::InvalidPath,
            "A project path must be absolute",
        ));
    }
    hide_project::facts(path).map_err(|error| match error {
        hide_project::ResolveError::MissingPath(_) => {
            HostError::new(ErrorCode::NotFound, error.to_string())
        }
        other => HostError::new(ErrorCode::Io, other.to_string()),
    })
}

fn open_root(root: &RootRef) -> HostResult<Root> {
    Root::open_pinned(Path::new(&root.path), root.identity)
}

/// The `label_transcript` answer for the transcripts under `home`; the
/// message of a refusal is the stable reason code, never a path or
/// transcript text.
pub fn label_transcript(
    home: &Path,
    request: &hide_session::label_transcript::LabelTranscriptRequest,
) -> HostResult<Value> {
    let transcript = hide_session::label_transcript::read(home, request).map_err(|reason| {
        let code = match reason.as_str() {
            "session_file_missing" => ErrorCode::NotFound,
            "session_kind_unsupported" => ErrorCode::Unsupported,
            reason if reason.starts_with("session_capacity:") => ErrorCode::TooLarge,
            _ => ErrorCode::Io,
        };
        HostError::new(code, reason)
    })?;
    to_value(transcript)
}

/// Answers one request. Public so the core's tests can drive the exact
/// dispatch the helper runs without a process.
pub fn handle(call: Call) -> HostResult<Value> {
    match call {
        Call::Hello => to_value(Hello {
            protocol: PROTOCOL_VERSION,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            home: std::env::var_os("HOME").map(|home| home.to_string_lossy().into_owned()),
        }),
        Call::RootOpen { root } => {
            let opened = Root::open(Path::new(&root))?;
            to_value(RootOpened {
                identity: opened.identity(),
            })
        }
        Call::List { root, path } => {
            let root = open_root(&root)?;
            let relative = relative_path(&path)?;
            list::require_directory(root.dir(), &relative)?;
            to_value(list::list(root.dir(), &relative, root.real_path())?)
        }
        Call::Stamps { root, folders } => {
            let root = open_root(&root)?;
            to_value(list::stamps(root.dir(), &folders)?)
        }
        Call::Bytes {
            root,
            path,
            offset,
            length,
        } => {
            let root = open_root(&root)?;
            to_value(bytes::read(
                root.dir(),
                &relative_path(&path)?,
                offset,
                length,
            )?)
        }
        Call::Index { root } => {
            let root = open_root(&root)?;
            to_value(index::walk(root.dir(), root.real_path()))
        }
        Call::OpenDocument { root, path } => {
            let root = open_root(&root)?;
            to_value(document::open(root.dir(), &relative_path(&path)?)?)
        }
        Call::Revision { root, path } => {
            let root = open_root(&root)?;
            to_value(RevisionNow {
                revision: save::current_revision(root.dir(), &relative_path(&path)?)?,
            })
        }
        Call::Project { path } => to_value(project_facts(&path)?),
        Call::Worktrees {
            path,
            bases,
            base_override,
        } => to_value(worktrees::read(
            &absolute(&path)?,
            &bases,
            base_override.as_deref(),
        )),
        Call::BranchCheck { path, branch } => {
            worktrees::check_new_branch(&absolute(&path)?, &branch)?;
            to_value(())
        }
        Call::Directory { path } => to_value(worktrees::directory(&absolute(&path)?)),
        Call::Registrable { path } => {
            let home = std::env::var_os("HOME").ok_or_else(|| {
                HostError::new(
                    ErrorCode::Unsupported,
                    "HOME is not set, so no folder can be judged against it",
                )
            })?;
            // `~` is this host's home, as a shell on it would read it.
            let path = match path.strip_prefix('~') {
                Some(rest) if rest.is_empty() || rest.starts_with('/') => {
                    format!("{}{rest}", home.to_string_lossy())
                }
                _ => path,
            };
            to_value(crate::register::check(&absolute(&path)?, Path::new(&home))?)
        }
        Call::HomeSync { projects } => {
            let home = std::env::var_os("HOME").ok_or_else(|| {
                HostError::new(
                    ErrorCode::Unsupported,
                    "HOME is not set, so Hide's Home folder has no place to live",
                )
            })?;
            if !Path::new(&home).is_absolute() {
                return Err(HostError::new(
                    ErrorCode::Unsupported,
                    "HOME is not an absolute path, so Hide's Home folder has no place to live",
                ));
            }
            to_value(crate::home::sync(Path::new(&home), &projects)?)
        }
        Call::Kit {
            action,
            cli_dir,
            herdr_socket,
        } => crate::kit::handle(action, &cli_dir, herdr_socket.as_deref()),
        Call::LabelTranscript { request } => {
            let home = std::env::var_os("HOME").ok_or_else(|| {
                HostError::new(ErrorCode::Unsupported, "label_session_home_unavailable")
            })?;
            label_transcript(Path::new(&home), &request)
        }
        Call::WorktreeRemove { removal } => {
            absolute(&removal.repository_root)?;
            absolute(&removal.checkout_path)?;
            to_value(worktrees::RemovalOutcome::from(
                worktrees::remove_confirmed(&removal),
            ))
        }
        Call::Changes {
            root,
            scope,
            selected,
            committed,
            base,
            diffs,
        } => {
            let root = open_root(&root)?;
            to_value(git::changes(
                &root,
                &git::ChangesQuery {
                    scope,
                    selected,
                    committed,
                    base,
                    diffs,
                },
            )?)
        }
        Call::Create {
            root,
            parent,
            name,
            directory,
        } => {
            let root = open_root(&root)?;
            to_value(mutate::create(
                root.dir(),
                &relative_path(&parent)?,
                &name,
                directory,
            )?)
        }
        Call::Rename { root, path, name } => {
            let root = open_root(&root)?;
            to_value(mutate::rename(root.dir(), &relative_path(&path)?, &name)?)
        }
        Call::Move {
            root,
            path,
            destination,
        } => {
            let root = open_root(&root)?;
            to_value(mutate::move_into(
                root.dir(),
                &relative_path(&path)?,
                &relative_path(&destination)?,
            )?)
        }
        Call::Trash { root, path, inode } => {
            let root = open_root(&root)?;
            to_value(mutate::trash(&root, &relative_path(&path)?, inode)?)
        }
        Call::Save {
            root,
            path,
            contents,
            expected_revision,
        } => {
            let root = open_root(&root)?;
            to_value(save::save(
                root.dir(),
                &relative_path(&path)?,
                contents.as_bytes(),
                &expected_revision,
            )?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// The SSH channel is gone: nothing written reaches anyone.
    struct Closed;

    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
    }

    #[test]
    fn a_closed_channel_ends_the_helper_with_requests_still_arriving() {
        // More requests than workers: each worker stops on its first failed
        // answer, and the rest must not leave the reader waiting forever.
        let requests: String = (1..=CONCURRENCY as u64 * 3)
            .map(|id| format!("{{\"id\":{id},\"op\":\"hello\"}}\n"))
            .collect();
        let (done, finished) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = done.send(serve(io::Cursor::new(requests.into_bytes()), Closed));
        });
        let result = finished
            .recv_timeout(Duration::from_secs(10))
            .expect("the helper kept running after its channel closed");
        assert!(result.is_ok(), "{result:?}");
    }
}
