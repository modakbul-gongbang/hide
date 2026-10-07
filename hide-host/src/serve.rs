//! The helper's request loop: JSON lines in, JSON lines out, until the input
//! ends. The input is the SSH channel, so the helper lives exactly as long as
//! the connection that started it (PRD S5.5 D-20).

use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::error::{ErrorCode, HostError, HostResult};
use crate::protocol::{
    Call, Hello, MachineIdentity, Outcome, PROTOCOL_VERSION, Request, Response, RevisionNow,
    RootOpened, RootRef,
};
use crate::root::{Root, relative_path};
use crate::{bytes, document, git, index, list, mutate, save, worktrees};
use hide_node_link::process::ProcessStart;

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
        Call::RealPaths { paths } => {
            let paths = paths
                .iter()
                .map(|path| absolute(path))
                .collect::<HostResult<Vec<_>>>()?;
            to_value(crate::cleanup::real_paths(&paths))
        }
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
                .map(|pid| match hide_platform::process::start_time(pid) {
                    Ok(started) => ProcessStart::Running { started },
                    Err(error) if error.kind() == io::ErrorKind::NotFound => ProcessStart::Gone,
                    Err(error) => ProcessStart::Unreadable {
                        reason: format!("process {pid} could not be read: {error}"),
                    },
                })
                .collect::<Vec<_>>(),
        ),
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
            let (step, _) =
                hide_session::search_read::read_step(saved.as_ref(), agent, &absolute(&path)?)
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
            to_value(hide_session::search_read::stamps(&paths))
        }
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
