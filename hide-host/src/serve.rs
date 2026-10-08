//! The helper's request loop: JSON lines in, JSON lines out, until the input
//! ends. The input is the SSH channel, so the helper lives exactly as long as
//! the connection that started it (PRD S5.5 D-20).

use std::collections::HashMap;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::error::{ErrorCode, HostError, HostResult};
use crate::protocol::{
    Call, Hello, MachineIdentity, Outcome, PROTOCOL_VERSION, Progress, Request, Response,
    RevisionNow, RootOpened, RootRef,
};
use crate::root::{Root, relative_path};
use crate::{bytes, document, git, index, list, mutate, save, worktrees};
use hide_node_link::process::ProcessStart;

/// Requests the helper works on at once; the core also admits at most this
/// many per device, so the helper never queues behind itself.
pub const CONCURRENCY: usize = 4;

/// Requests waiting for a worker. The core admits at most [`CONCURRENCY`]
/// at once, so this fills only behind calls the core stopped waiting for; a
/// request past it is answered busy, and the reader never waits, so a cancel
/// or the end of input is always read.
const QUEUED: usize = 32;

/// A request line longer than this is refused without being parsed: a save
/// carries at most the 16 MiB editable size, as JSON-escaped text.
const MAX_REQUEST_BYTES: usize = 40 * 1024 * 1024;

/// A node's terminal service as this loop reaches it. The service lives in
/// `hide-node`, which depends on this crate, so the node role hands it in
/// (PRD core-host-node-terminal D-18): terminal lines from the core go to
/// [`Terminals::line`] as they are read, never behind a request, and the
/// service's own lines go out between the answers.
pub trait Terminals: Send + Sync {
    /// Starts the service for the Herdr at `herdr_socket`; starting it again
    /// while it runs changes nothing.
    fn start(&self, herdr_socket: &str) -> Result<(), String>;
    /// One terminal line from the core, its newline removed.
    fn line(&self, line: &[u8]);
    /// The next line to send up, newline included, waiting for one; `None`
    /// once [`Terminals::stop`] was called.
    fn next_up(&self) -> Option<Vec<u8>>;
    /// The link is gone: every session ends and [`Terminals::next_up`]
    /// answers `None`.
    fn stop(&self);
}

pub fn serve(input: impl BufRead, output: impl Write + Send) -> io::Result<()> {
    serve_in(input, output, Env::of_process(), None)
}

/// [`serve`] with the node's terminal service.
pub fn serve_with_terminals(
    input: impl BufRead,
    output: impl Write + Send,
    terminals: &dyn Terminals,
) -> io::Result<()> {
    serve_in(input, output, Env::of_process(), Some(terminals))
}

fn serve_in(
    input: impl BufRead,
    output: impl Write + Send,
    env: Env,
    terminals: Option<&dyn Terminals>,
) -> io::Result<()> {
    let output = Mutex::new(output);
    // The calls handed to a worker and not yet answered, each with whether
    // it was asked to stop; the reader enters one before handing it over, so
    // a cancel that arrives first still reaches it.
    let running: Mutex<HashMap<u64, bool>> = Mutex::new(HashMap::new());
    let panes = crate::panes::Panes::new();
    // Where a pane's `hide` on this machine finds the node's bootstrap
    // socket; read once, so every start of the service agrees.
    let bridges = hide_platform::host::home_dir().map(|home| {
        hide_kit::layout::workspace_bridges(&hide_kit::layout::state_dir_from_process(&home))
    });
    let (sender, receiver) = mpsc::sync_channel::<Request>(QUEUED);
    // Only the workers hold the receiver. A worker stops when its answer
    // cannot be written, which means the SSH channel is gone; once the last
    // one has stopped, the next request's send fails and the helper exits.
    let receiver = Arc::new(Mutex::new(receiver));
    let result = std::thread::scope(|scope| {
        if let Some(terminals) = terminals {
            let output = &output;
            scope.spawn(move || {
                // One line per turn of the output lock, so an answer waits
                // behind at most one terminal line.
                while let Some(line) = terminals.next_up() {
                    if write_raw(output, &line).is_err() {
                        terminals.stop();
                        return;
                    }
                }
            });
        }
        for _ in 0..CONCURRENCY {
            let receiver = Arc::clone(&receiver);
            let output = &output;
            let panes = &panes;
            let bridges = &bridges;
            let env = &env;
            let running = &running;
            scope.spawn(move || {
                loop {
                    let request = match receiver.lock().map(|receiver| receiver.recv()) {
                        Ok(Ok(request)) => request,
                        _ => return,
                    };
                    let id = request.id;
                    let outcome = match request.call {
                        call if !call.answered_by_device() => Err(HostError::new(
                            ErrorCode::Unsupported,
                            "A device does not answer this request; the core's own node does",
                        )),
                        Call::PanesStart { herdr_socket } => match bridges {
                            Ok(bridges) => panes
                                .start(scope, output, bridges, &herdr_socket)
                                .and_then(to_value),
                            Err(error) => Err(HostError::new(ErrorCode::Io, error.to_string())),
                        },
                        Call::TerminalsStart { herdr_socket } => match terminals {
                            Some(terminals) => terminals
                                .start(&herdr_socket)
                                .map(|()| Value::Null)
                                .map_err(|message| HostError::new(ErrorCode::Io, message)),
                            None => Err(HostError::new(
                                ErrorCode::Unsupported,
                                "This node runs no terminal service",
                            )),
                        },
                        Call::PaneProofAnswer { request, answer } => {
                            panes.answer_proof(request, answer)
                        }
                        Call::PaneInspect { pane_id } => panes.inspect(&pane_id).and_then(to_value),
                        Call::StreamWrite { stream, data } => panes.write_stream(stream, &data),
                        Call::StreamClose { stream } => panes.close_stream(stream),
                        call => handle_with_progress(call, env, &mut |report| {
                            write_line(
                                output,
                                &Progress {
                                    progress: id,
                                    report,
                                },
                            )
                            .is_ok()
                                && lock(running).get(&id) == Some(&false)
                        }),
                    };
                    lock(running).remove(&id);
                    let response = Response {
                        id,
                        outcome: match outcome {
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
        let result = read_requests(input, &sender, &output, &running, terminals);
        drop(sender);
        // The connection is gone: the terminal sessions end with it, so no
        // attach child outlives the link that asked for it (D-20, B20).
        if let Some(terminals) = terminals {
            terminals.stop();
        }
        // The connection is gone: a kit step still running ends its child
        // rather than keep the helper alive after it, and the pane service
        // ends its listener and streams so the scope can close.
        crate::kit::stop();
        panes.stop();
        result
    });
    panes.remove_folder();
    result
}

fn read_requests(
    mut input: impl BufRead,
    sender: &mpsc::SyncSender<Request>,
    output: &Mutex<impl Write>,
    running: &Mutex<HashMap<u64, bool>>,
    terminals: Option<&dyn Terminals>,
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
        if line.starts_with(hide_node_link::terminal::TERMINAL_LINE_PREFIX) {
            // A node without the service never answered `terminals_start`,
            // so the core sends it no terminal line; one that arrives anyway
            // has no pane to reach.
            if let Some(terminals) = terminals {
                terminals.line(line.trim_ascii_end());
            }
            continue;
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
        // A cancel is answered here, never queued behind the call it stops,
        // which may hold a worker for as long as it runs.
        if let Call::Cancel { request: target } = request.call {
            if let Some(cancelled) = lock(running).get_mut(&target) {
                *cancelled = true;
            }
            write_line(
                output,
                &Response {
                    id: request.id,
                    outcome: Outcome::Ok(Value::Null),
                },
            )?;
            continue;
        }
        let id = request.id;
        // An id still in flight would share its entry, and one ending would
        // take the other's cancel with it.
        if lock(running).contains_key(&id) {
            write_line(
                output,
                &Response {
                    id,
                    outcome: Outcome::Error(HostError::new(
                        ErrorCode::InvalidRequest,
                        "A request with this id is still running",
                    )),
                },
            )?;
            continue;
        }
        lock(running).insert(id, false);
        match sender.try_send(request) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_)) => {
                lock(running).remove(&id);
                write_line(
                    output,
                    &Response {
                        id,
                        outcome: Outcome::Error(HostError::new(
                            ErrorCode::Busy,
                            "The device is working on as many requests as it holds",
                        )),
                    },
                )?;
            }
            Err(mpsc::TrySendError::Disconnected(_)) => return Ok(()),
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn write_line(output: &Mutex<impl Write>, line: &impl serde::Serialize) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(line).map_err(io::Error::other)?;
    bytes.push(b'\n');
    let mut output = output
        .lock()
        .map_err(|_| io::Error::other("helper output lock poisoned"))?;
    output.write_all(&bytes)?;
    output.flush()
}

fn write_raw(output: &Mutex<impl Write>, line: &[u8]) -> io::Result<()> {
    let mut output = output
        .lock()
        .map_err(|_| io::Error::other("helper output lock poisoned"))?;
    output.write_all(line)?;
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

/// The agents whose session files the core reads by path.
const SESSION_FILE_AGENTS: &[hide_session::Agent] =
    &[hide_session::Agent::Claude, hide_session::Agent::Codex];

/// A session file the core names, refused unless it lies in one of `agents`'
/// session folders in this machine's home, answered with every link resolved.
fn session_file(env: &Env, agents: &[hide_session::Agent], path: &str) -> HostResult<PathBuf> {
    let path = absolute(path)?;
    let home = env.home("sessions_home_unavailable")?;
    hide_session::inside_session_root(Path::new(&home), agents, &path).map_err(|refusal| {
        match refusal {
            hide_session::RootRefusal::Unsupported | hide_session::RootRefusal::Outside => {
                HostError::new(ErrorCode::OutsideRoot, "session_outside_roots")
            }
            hide_session::RootRefusal::Missing => {
                HostError::new(ErrorCode::Io, "session_file_missing")
            }
            hide_session::RootRefusal::Unreadable => {
                HostError::new(ErrorCode::Io, "session_unreadable")
            }
        }
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

/// Local and remote activity share this path-free, metadata-only answer.
pub fn session_activity(
    home: &Path,
    request: &hide_session::session_activity::SessionActivityRequest,
) -> HostResult<Value> {
    let activity = hide_session::session_activity::read(home, request).map_err(|reason| {
        let code = match reason.as_str() {
            "session_file_missing" => ErrorCode::NotFound,
            "session_kind_unsupported" => ErrorCode::Unsupported,
            _ => ErrorCode::Io,
        };
        HostError::new(code, reason)
    })?;
    to_value(activity)
}

/// What a node answers from besides the request: the account home its
/// sessions, kit and Home folder live in, and where its kit's parts come
/// from. A helper reads them from its process once; the core's own node is
/// given the home the core was configured with, so a daemon running with a
/// private home never answers from the operator's.
#[derive(Clone, Debug)]
pub struct Env {
    pub home: Option<PathBuf>,
    pub kit: KitPlace,
    /// Raised when the node's owner goes away: a kit step still running
    /// ends the child it waits on, and no further part starts.
    pub stop: Arc<AtomicBool>,
    /// The background AI backends this node keeps for its core.
    pub ai: Arc<crate::ai::Backends>,
    /// The verify bundles this node runs for its core's Factory, ended when
    /// the last copy of the environment is dropped.
    pub factory: Arc<crate::factory::Verifies>,
}

/// The AI backends a node answering for this process keeps, shared by every
/// request it serves.
fn process_ai() -> Arc<crate::ai::Backends> {
    static BACKENDS: std::sync::OnceLock<Arc<crate::ai::Backends>> = std::sync::OnceLock::new();
    Arc::clone(BACKENDS.get_or_init(Arc::default))
}

/// Where a node's install kit takes its parts from, which decides the
/// target it installs into.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KitPlace {
    /// A helper Hide installed under a helper root: the root the running
    /// executable sits in (`kit::handle`).
    Installed,
    /// The resources folder of the desktop package the node runs from
    /// (`hide_kit::bundled_kit_dir`).
    Bundled(PathBuf),
    /// A development or standalone daemon, which installs nothing
    /// (`hide_kit::STANDALONE_REASON`).
    Standalone,
}

impl Env {
    pub fn of_process() -> Self {
        Self {
            home: std::env::var_os("HOME").map(PathBuf::from),
            kit: KitPlace::Installed,
            stop: crate::kit::process_stop(),
            ai: process_ai(),
            factory: Arc::default(),
        }
    }

    /// A node of its own, answering for `home`: it installs no kit and keeps
    /// its own AI backends, which end when the last copy of it is dropped.
    pub fn standalone(home: Option<PathBuf>) -> Self {
        Self {
            home,
            kit: KitPlace::Standalone,
            stop: Arc::default(),
            ai: Arc::default(),
            factory: Arc::default(),
        }
    }

    fn home(&self, missing: &str) -> HostResult<PathBuf> {
        self.home
            .clone()
            .ok_or_else(|| HostError::new(ErrorCode::Unsupported, missing))
    }
}

/// Answers one request with this process's environment. Public so the
/// core's tests can drive the exact dispatch the helper runs without a
/// process.
pub fn handle(call: Call) -> HostResult<Value> {
    handle_in(call, &Env::of_process())
}

/// Answers one request for the node whose environment is `env`.
pub fn handle_in(call: Call, env: &Env) -> HostResult<Value> {
    handle_with_progress(call, env, &mut |_| true)
}

/// [`handle_in`] for a caller that hears a long call's progress reports and
/// answers whether it should go on.
pub fn handle_with_progress(
    call: Call,
    env: &Env,
    progress: &mut dyn FnMut(Value) -> bool,
) -> HostResult<Value> {
    match call {
        Call::Hello => to_value(Hello {
            protocol: PROTOCOL_VERSION,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            home: env
                .home
                .as_ref()
                .map(|home| home.to_string_lossy().into_owned()),
            machine_identity: match hide_platform::host::machine_id() {
                Ok(id) => MachineIdentity::Available { id },
                Err(error) => MachineIdentity::Unavailable {
                    reason: error.to_string(),
                },
            },
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
            let home = env.home("HOME is not set, so no folder can be judged against it")?;
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
            let home = env.home("HOME is not set, so Hide's Home folder has no place to live")?;
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
            retirement_projects,
        } => crate::kit::handle(
            action,
            &cli_dir,
            herdr_socket.as_deref(),
            &retirement_projects,
            env,
        ),
        Call::WorktreesRegistered { root } => to_value(
            worktrees::registered(&absolute(&root)?)
                .map_err(|reason| HostError::new(ErrorCode::Io, reason))?,
        ),
        Call::IgnoredRepository { worktree } => to_value(
            worktrees::ignored_repository(&absolute(&worktree)?)
                .map_err(|reason| HostError::new(ErrorCode::Io, reason))?,
        ),
        Call::ProjectCreate {
            path,
            new_folder,
            initialize_git,
        } => to_value(
            crate::project::create(&absolute(&path)?, new_folder, initialize_git)
                .map_err(|reason| HostError::new(ErrorCode::Io, reason))?,
        ),
        Call::PathFacts { paths } => to_value(crate::catalog::path_facts(&paths)?),
        Call::RealPaths { paths } => to_value(crate::cleanup::real_paths(&paths)),
        Call::Repository { path } => to_value(crate::cleanup::repository(&absolute(&path)?)),
        Call::JudgeFolders {
            root,
            folders,
            walk,
        } => to_value(crate::cleanup::judge_folders(
            &absolute(&root)?,
            &folders,
            walk,
        )),
        Call::SetAsideFolder { common, folder } => to_value(
            worktrees::set_aside_folder(&absolute(&common)?, &absolute(&folder)?).map_err(
                |error| {
                    HostError::new(
                        ErrorCode::Io,
                        format!("The folder could not be moved aside: {error}"),
                    )
                },
            )?,
        ),
        Call::WorktreeRemoveClean {
            root,
            checkout,
            common,
        } => to_value(crate::cleanup::clean_removal(
            &absolute(&root)?,
            &absolute(&checkout)?,
            &absolute(&common)?,
        )),
        Call::DrainTrash {
            common,
            ours,
            wait_ms,
        } => to_value(crate::cleanup::drain_trash(
            &absolute(&common)?,
            &ours,
            std::time::Duration::from_millis(wait_ms),
        )),
        Call::RepositoryClone { source, parent } => {
            let parent = absolute(&parent)?;
            to_value(crate::clone::clone_reporting(
                &source,
                &parent,
                &mut |report| serde_json::to_value(report).map_or(true, &mut *progress),
            ))
        }
        Call::GitWatch { common_dirs } => {
            let common_dirs = common_dirs
                .iter()
                .map(|dir| absolute(dir))
                .collect::<HostResult<Vec<_>>>()?;
            to_value(crate::git_watch::watch(
                &common_dirs,
                &env.stop,
                &mut |report| serde_json::to_value(report).is_ok_and(&mut *progress),
            )?)
        }
        Call::Git { root, command } => {
            to_value(crate::git_command::run(&absolute(&root)?, &command)?)
        }
        Call::AiAvailability { backend } => to_value(env.ai.availability(&backend)?),
        Call::AiModels { backend } => to_value(env.ai.models(&backend)?),
        Call::AiExecute { backend, request } => to_value(env.ai.execute(
            &backend,
            request,
            &mut || progress(Value::Null),
        )?),
        Call::AiMeasurement { backend } => to_value(env.ai.measurement(&backend)?),
        Call::AiRestart { backend } => to_value(env.ai.restart(&backend)?),
        Call::AiRelease { instance } => {
            env.ai.release(instance);
            to_value(())
        }
        Call::CodexCredentials { codex_home } => {
            to_value(crate::usage::codex_credentials(&absolute(&codex_home)?))
        }
        Call::CodexSessionUsage { codex_home } => {
            to_value(crate::usage::codex_session_usage(&absolute(&codex_home)?))
        }
        Call::ClaudeUsageText { cwd } => {
            let cwd = absolute(&cwd)?;
            to_value(crate::usage::claude_usage_text(&cwd, &mut || {
                progress(Value::Null)
            }))
        }
        Call::ReadAttachments { paths } => to_value(
            crate::attachments::read_sources(&paths, &mut |index| progress(Value::from(index)))
                .map_err(|reason| HostError::new(ErrorCode::InvalidPath, reason))?,
        ),
        Call::RemoveClipboard { path } => to_value(
            crate::attachments::remove_clipboard(Path::new(&path))
                .map_err(|reason| HostError::new(ErrorCode::InvalidPath, reason))?,
        ),
        Call::AgentInstalled { name } => {
            to_value(hide_ai::resolve_binary(Path::new(&name)).is_some())
        }
        Call::TerminateGroup { leader } => {
            if leader <= 1 {
                return Err(HostError::new(
                    ErrorCode::InvalidPath,
                    format!("Process group {leader} is not a pane's"),
                ));
            }
            hide_platform::process::terminate_group(leader).map_err(|error| {
                HostError::new(
                    ErrorCode::Io,
                    format!("ending process group {leader} failed: {error}"),
                )
            })?;
            to_value(())
        }
        Call::ProcessStarts { pids } => to_value(
            pids.into_iter()
                .map(|pid| {
                    let read = hide_platform::process::start_time(pid).and_then(|started| {
                        hide_platform::process::name_of(pid)
                            .map(|name| ProcessStart::Running { started, name })
                    });
                    match read {
                        Ok(running) => running,
                        Err(error) if error.kind() == io::ErrorKind::NotFound => ProcessStart::Gone,
                        Err(error) => ProcessStart::Unreadable {
                            reason: format!("process {pid} could not be read: {error}"),
                        },
                    }
                })
                .collect::<Vec<_>>(),
        ),
        Call::ProcessDescendants { pid } => {
            to_value(hide_platform::process::descendants(pid).map_err(|error| {
                HostError::new(
                    ErrorCode::Io,
                    format!("the processes under {pid} could not be read: {error}"),
                )
            })?)
        }
        Call::DiskUsage { paths, shared_git } => {
            let request = crate::disk::DiskRequest {
                paths: paths.iter().map(PathBuf::from).collect(),
                shared_git: shared_git.iter().map(PathBuf::from).collect(),
            };
            to_value(crate::disk::read_with(&request, |row| {
                if let Ok(row) = serde_json::to_value(row) {
                    progress(row);
                }
            }))
        }
        Call::Gh { cwd, args } => {
            let cwd = cwd.as_deref().map(absolute).transpose()?;
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            to_value(crate::gh::run(cwd.as_deref(), &args))
        }
        Call::ListeningPorts => to_value(crate::ports::read()),
        Call::VolumeFree { path } => to_value(crate::disk::volume_free_bytes(&absolute(&path)?)),
        Call::HookDiagnosis => {
            let home = env.home("HOME is not set, so the agent hooks have no account to read")?;
            to_value(hide_agent_hooks::Diagnosis::read(&home))
        }
        Call::LabelTranscript { request } => {
            let home = env.home("label_session_home_unavailable")?;
            label_transcript(Path::new(&home), &request)
        }
        Call::SessionActivity { request } => {
            let home = env.home("session_activity_home_unavailable")?;
            session_activity(Path::new(&home), &request)
        }
        Call::LinkFiles {
            since_unix_ms,
            until_unix_ms,
        } => {
            let home = env.home("links_home_unavailable")?;
            let listed =
                hide_session::links::candidates(Path::new(&home), since_unix_ms, until_unix_ms)
                    .map_err(|code| HostError::new(ErrorCode::Io, code))?;
            to_value(listed)
        }
        Call::SessionIndexRead { agent, path, saved } => {
            let path = session_file(env, &[agent], &path)?;
            let (step, _) = hide_session::search_read::read_step(saved.as_ref(), agent, &path)
                .map_err(|reason| HostError::new(ErrorCode::Io, reason))?;
            to_value(step)
        }
        Call::SessionStamps { paths } => {
            if paths.len() > hide_session::search::STAMP_LIMIT {
                return Err(HostError::new(
                    ErrorCode::InvalidRequest,
                    format!(
                        "At most {} session stamps are read at once",
                        hide_session::search::STAMP_LIMIT
                    ),
                ));
            }
            let home = env.home("sessions_home_unavailable")?;
            to_value(
                paths
                    .iter()
                    .map(|path| {
                        let path = Path::new(path);
                        if !path.is_absolute() {
                            return None;
                        }
                        let path = hide_session::inside_session_root(
                            Path::new(&home),
                            SESSION_FILE_AGENTS,
                            path,
                        )
                        .ok()?;
                        hide_session::search_read::stamp_at(&path)
                    })
                    .collect::<Vec<_>>(),
            )
        }
        Call::ProjectSessions { project } => {
            let home = env.home("sessions_home_unavailable")?;
            let sessions =
                hide_session::SessionCatalog::new(Path::new(&home), project.device_id.clone())
                    .project_sessions(&project)
                    .map_err(|error| HostError::new(ErrorCode::Io, error.to_string()))?;
            to_value(sessions)
        }
        Call::SessionStat { path } => to_value(
            crate::sessions::stat(&session_file(env, SESSION_FILE_AGENTS, &path)?)
                .map_err(|error| HostError::new(ErrorCode::Io, error.to_string()))?,
        ),
        Call::SessionChunk { path, checkpoint } => to_value(
            crate::sessions::chunk(&session_file(env, SESSION_FILE_AGENTS, &path)?, checkpoint)
                .map_err(|error| HostError::new(ErrorCode::Io, error.to_string()))?,
        ),
        Call::PanesStart { .. }
        | Call::TerminalsStart { .. }
        | Call::PaneProofAnswer { .. }
        | Call::PaneInspect { .. }
        | Call::StreamWrite { .. }
        | Call::StreamClose { .. } => Err(HostError::new(
            ErrorCode::Unsupported,
            "Only a device node's link carries its panes' credentials and commands",
        )),
        Call::SessionText { path } => to_value(
            hide_session::read_bounded(
                &session_file(env, SESSION_FILE_AGENTS, &path)?,
                hide_session::SESSION_READ_LIMIT_BYTES,
            )
            .map_err(|error| HostError::new(ErrorCode::Io, error.to_string()))?,
        ),
        Call::LinkRead { requests } => {
            if requests.len() > hide_session::links::READ_FILE_LIMIT {
                return Err(HostError::new(
                    ErrorCode::InvalidRequest,
                    "links_read_limit",
                ));
            }
            let home = env.home("links_home_unavailable")?;
            to_value(hide_session::links::read(Path::new(&home), &requests))
        }
        Call::WorktreeRemove { removal } => {
            absolute(&removal.repository_root)?;
            absolute(&removal.checkout_path)?;
            to_value(worktrees::RemovalOutcome::from(
                worktrees::remove_confirmed(&removal),
            ))
        }
        Call::WorktreeRemovalCheck { removal } => {
            absolute(&removal.repository_root)?;
            absolute(&removal.checkout_path)?;
            worktrees::check_removal(&removal)
                .map_err(|reason| HostError::new(ErrorCode::Io, reason))?;
            to_value(())
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
        Call::Factory { call } => crate::factory::handle(call, &env.factory, progress),
        // In a process, a reporting call stops through its reports; there is
        // no request to cancel.
        Call::Cancel { .. } => Ok(Value::Null),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn activity_helper_answers_metadata_only_and_refuses_unsupported_references() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join(".codex/sessions/2026/01/01");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("activity.jsonl");
        let contents =
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"native-a\"}}\nsecret conversation\n";
        std::fs::write(&path, contents).unwrap();
        let mut request = hide_session::session_activity::SessionActivityRequest {
            agent: hide_session::Agent::Codex,
            reference_kind: "path".to_owned(),
            reference_value: path.to_string_lossy().into_owned(),
            cwd: None,
        };
        let answer = session_activity(home.path(), &request).unwrap();
        assert_eq!(answer.as_object().unwrap().len(), 2);
        assert_eq!(answer["bytes"], contents.len() as u64);
        assert!(answer["modified_at_unix_ms"].as_u64().unwrap() > 0);
        let serialized = answer.to_string();
        assert!(!serialized.contains("native-a"));
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains(&request.reference_value));
        let call = Call::SessionActivity {
            request: request.clone(),
        };
        assert_eq!(
            serde_json::from_value::<Call>(serde_json::to_value(&call).unwrap()).unwrap(),
            call
        );
        request.reference_kind = "opaque".to_owned();
        let error = session_activity(home.path(), &request).unwrap_err();
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert_eq!(error.message, "session_kind_unsupported");
    }

    /// A session read names a file the core found in the agents' session
    /// folders; a path outside them, or a link planted inside one that
    /// leads out, is refused unread, and a pipe in their place is refused
    /// at once instead of holding the node.
    #[cfg(unix)]
    #[test]
    fn a_session_read_stays_in_the_session_folders_and_never_waits() {
        let home = tempfile::tempdir().unwrap();
        let env = Env::standalone(Some(home.path().to_path_buf()));
        let folder = home.path().join(".claude/projects/-project");
        std::fs::create_dir_all(&folder).unwrap();
        let session = folder.join("session.jsonl");
        std::fs::write(&session, "{\"type\":\"user\"}\n").unwrap();
        let secret = home.path().join("secret.jsonl");
        std::fs::write(&secret, "secret\n").unwrap();
        let planted = folder.join("planted.jsonl");
        std::os::unix::fs::symlink(&secret, &planted).unwrap();
        let named = |path: &Path| path.to_string_lossy().into_owned();
        let reads = |path: String| {
            vec![
                Call::SessionText { path: path.clone() },
                Call::SessionStat { path: path.clone() },
                Call::SessionChunk {
                    path: path.clone(),
                    checkpoint: None,
                },
                Call::SessionIndexRead {
                    agent: hide_session::Agent::Claude,
                    path,
                    saved: None,
                },
            ]
        };

        for call in reads(named(&session)) {
            handle_in(call, &env).unwrap();
        }
        for (path, code) in [
            (named(&secret), ErrorCode::OutsideRoot),
            (named(&planted), ErrorCode::OutsideRoot),
            ("session.jsonl".to_owned(), ErrorCode::InvalidPath),
        ] {
            for call in reads(path.clone()) {
                let refused = handle_in(call, &env).unwrap_err();
                assert_eq!(refused.code, code, "{path}");
            }
        }
        let stamps = handle_in(
            Call::SessionStamps {
                paths: vec![named(&session), named(&secret), named(&planted)],
            },
            &env,
        )
        .unwrap();
        let stamps = stamps.as_array().unwrap();
        assert!(stamps[0].is_string());
        assert!(stamps[1].is_null() && stamps[2].is_null());

        let pipe = folder.join("pipe.jsonl");
        let made = std::process::Command::new("mkfifo")
            .arg(&pipe)
            .status()
            .unwrap();
        assert!(made.success());
        let (sender, answers) = mpsc::channel();
        std::thread::spawn(move || {
            for call in reads(named(&pipe)) {
                if matches!(call, Call::SessionStat { .. }) {
                    continue;
                }
                sender.send(handle_in(call, &env).map(|_| ())).unwrap();
            }
        });
        for _ in 0..3 {
            let refused = answers
                .recv_timeout(Duration::from_secs(5))
                .expect("a pipe read answers at once");
            assert_eq!(refused.unwrap_err().code, ErrorCode::Io);
        }
    }

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

    /// What the helper writes, kept for the test to read.
    #[derive(Clone, Default)]
    struct Kept(Arc<Mutex<Vec<u8>>>);

    impl Write for Kept {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// Every worker held by a call that reports until stopped: the reader
    /// still queues, answers past its queue busy, reads each cancel, and
    /// ends at the end of its input.
    #[cfg(unix)]
    #[test]
    fn the_reader_never_waits_for_a_worker() {
        use std::os::unix::net::UnixStream;
        /// Each line the helper writes, as it writes it.
        struct Lines(mpsc::Sender<Vec<u8>>);
        impl Write for Lines {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                let _ = self.0.send(bytes.to_vec());
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let common = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(common.path().join("refs/heads")).unwrap();
        let line = |id: u64, call: Call| {
            let mut line = serde_json::to_vec(&Request { id, call }).unwrap();
            line.push(b'\n');
            line
        };
        let watch = || Call::GitWatch {
            common_dirs: vec![common.path().to_string_lossy().into_owned()],
        };
        let (mut input, theirs) = UnixStream::pair().unwrap();
        let (written, lines) = mpsc::channel();
        let (done, finished) = mpsc::channel();
        std::thread::spawn(move || {
            // A stop of its own: another test's helper ending stops the
            // process's watches.
            let env = Env {
                stop: Arc::default(),
                ..Env::of_process()
            };
            let _ = done.send(serve_in(
                io::BufReader::new(theirs),
                Lines(written),
                env,
                None,
            ));
        });
        let mut text = String::new();
        let mut next = || {
            let bytes = lines
                .recv_timeout(Duration::from_secs(20))
                .expect("the helper went quiet");
            text.push_str(std::str::from_utf8(&bytes).unwrap());
            text.clone()
        };
        for id in 1..=CONCURRENCY as u64 {
            input.write_all(&line(id, watch())).unwrap();
        }
        // Every worker holds a watch once each has reported.
        let mut reported = std::collections::BTreeSet::new();
        while reported.len() < CONCURRENCY {
            for progress in next()
                .lines()
                .filter_map(|line| serde_json::from_str::<Progress>(line).ok())
            {
                reported.insert(progress.progress);
            }
        }
        for id in 0..=QUEUED as u64 {
            input.write_all(&line(100 + id, Call::Hello)).unwrap();
        }
        for request in 1..=CONCURRENCY as u64 {
            input
                .write_all(&line(200 + request, Call::Cancel { request }))
                .unwrap();
        }
        input.shutdown(std::net::Shutdown::Write).unwrap();
        let result = finished
            .recv_timeout(Duration::from_secs(20))
            .expect("the helper waited behind its workers");
        assert!(result.is_ok(), "{result:?}");
        let mut text = String::new();
        while let Ok(bytes) = lines.try_recv() {
            text.push_str(std::str::from_utf8(&bytes).unwrap());
        }
        let answers: Vec<(u64, Option<ErrorCode>)> = text
            .lines()
            .filter_map(|line| serde_json::from_str::<Response>(line).ok())
            .map(|answer| match answer.outcome {
                Outcome::Error(error) => (answer.id, Some(error.code)),
                _ => (answer.id, None),
            })
            .collect();
        let busy = 100 + QUEUED as u64;
        assert!(
            answers.contains(&(busy, Some(ErrorCode::Busy))),
            "{answers:?}"
        );
        for id in (1..=CONCURRENCY as u64)
            .chain(100..busy)
            .chain(201..=200 + CONCURRENCY as u64)
        {
            assert!(
                answers
                    .iter()
                    .any(|(answered, code)| *answered == id && *code != Some(ErrorCode::Busy)),
                "request {id} was not answered: {answers:?}"
            );
        }
    }

    /// A request whose id is still running is refused, and the running one
    /// keeps its own cancel.
    #[cfg(unix)]
    #[test]
    fn a_repeated_id_is_refused_and_leaves_the_running_call_alone() {
        use std::os::unix::net::UnixStream;
        let common = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(common.path().join("refs/heads")).unwrap();
        let line = |id: u64, call: Call| {
            let mut line = serde_json::to_vec(&Request { id, call }).unwrap();
            line.push(b'\n');
            line
        };
        let watch = || Call::GitWatch {
            common_dirs: vec![common.path().to_string_lossy().into_owned()],
        };
        let (mut input, theirs) = UnixStream::pair().unwrap();
        let kept = Kept::default();
        let output = kept.clone();
        let (done, finished) = mpsc::channel();
        std::thread::spawn(move || {
            let env = Env {
                stop: Arc::default(),
                ..Env::of_process()
            };
            let _ = done.send(serve_in(io::BufReader::new(theirs), output, env, None));
        });
        input.write_all(&line(7, watch())).unwrap();
        input.write_all(&line(7, watch())).unwrap();
        input
            .write_all(&line(8, Call::Cancel { request: 7 }))
            .unwrap();
        input.shutdown(std::net::Shutdown::Write).unwrap();
        let result = finished
            .recv_timeout(Duration::from_secs(20))
            .expect("the cancelled watch kept the helper running");
        assert!(result.is_ok(), "{result:?}");
        let written = String::from_utf8(kept.0.lock().unwrap().clone()).unwrap();
        let mut answers: Vec<(u64, Option<ErrorCode>)> = written
            .lines()
            .filter_map(|line| serde_json::from_str::<Response>(line).ok())
            .map(|answer| match answer.outcome {
                Outcome::Error(error) => (answer.id, Some(error.code)),
                _ => (answer.id, None),
            })
            .collect();
        answers.sort_by_key(|(id, code)| (*id, code.is_none()));
        assert_eq!(
            answers,
            [(7, Some(ErrorCode::InvalidRequest)), (7, None), (8, None)]
        );
    }

    /// A device refuses, unrun, what acts with the operator's logins or is
    /// the Factory's, and still answers its own work on the same channel.
    #[test]
    fn a_device_refuses_the_core_machine_s_requests_unrun() {
        let requests = [
            r#"{"id":1,"op":"gh","cwd":null,"args":["auth","status"]}"#,
            r#"{"id":2,"op":"factory","call":{"factory":"hide_program"}}"#,
            r#"{"id":3,"op":"codex_credentials","codex_home":"/"}"#,
            r#"{"id":4,"op":"hello"}"#,
        ];
        let input = requests.join("\n") + "\n";
        let kept = Kept::default();
        serve(io::Cursor::new(input.into_bytes()), kept.clone()).unwrap();
        let written = String::from_utf8(kept.0.lock().unwrap().clone()).unwrap();
        let mut answers: Vec<Response> = written
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        answers.sort_by_key(|answer| answer.id);
        assert_eq!(answers.len(), 4, "{written}");
        for answer in &answers[..3] {
            match &answer.outcome {
                Outcome::Error(error) => assert_eq!(error.code, ErrorCode::Unsupported),
                other => panic!("request {} was answered: {other:?}", answer.id),
            }
        }
        assert!(matches!(answers[3].outcome, Outcome::Ok(_)));
    }
}
