//! The helper's request loop: JSON lines in, JSON lines out, until the input
//! ends. The input is the SSH channel, so the helper lives exactly as long as
//! the connection that started it (PRD S5.5 D-20).

use std::io::{self, BufRead, Read, Write};
use std::path::Path;
use std::sync::Mutex;
use std::sync::mpsc;

use serde_json::Value;

use crate::error::{ErrorCode, HostError, HostResult};
use crate::protocol::{
    Call, Hello, Outcome, PROTOCOL_VERSION, Request, Response, RevisionNow, RootOpened, RootRef,
};
use crate::root::{Root, relative_path};
use crate::{document, git, list, mutate, save};

/// Requests the helper works on at once; the core also admits at most this
/// many per device, so the helper never queues behind itself.
pub const CONCURRENCY: usize = 4;

/// A request line longer than this is refused without being parsed: a save
/// carries at most the 16 MiB editable size, as JSON-escaped text.
const MAX_REQUEST_BYTES: usize = 40 * 1024 * 1024;

pub fn serve(input: impl BufRead, output: impl Write + Send) -> io::Result<()> {
    let output = Mutex::new(output);
    let (sender, receiver) = mpsc::sync_channel::<Request>(0);
    let receiver = Mutex::new(receiver);
    std::thread::scope(|scope| {
        for _ in 0..CONCURRENCY {
            scope.spawn(|| {
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
                    if write_line(&output, &response).is_err() {
                        return;
                    }
                }
            });
        }
        let result = read_requests(input, &sender, &output);
        drop(sender);
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
        Call::Changes {
            root,
            scope,
            selected,
            committed,
            base,
        } => {
            let root = open_root(&root)?;
            let scope_path = relative_path(&scope)?;
            to_value(git::changes(
                &root,
                &scope_path,
                &git::ChangesQuery {
                    scope,
                    selected,
                    committed,
                    base,
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
