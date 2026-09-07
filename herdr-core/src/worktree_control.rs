//! Worktree actions cross the socket boundary before publishing removal readiness.
use super::*;
use std::path::Path;
use std::process::Command;

const CONFIRM_TIMEOUT: Duration = Duration::from_secs(5);
const CONFIRM_POLL: Duration = Duration::from_millis(100);

pub fn spawn_worktree_close(
    context: LiveContext,
    id: u64,
    checkout_path: String,
    pane_ids: Vec<String>,
) -> Result<(), String> {
    thread::Builder::new()
        .name("herdr-core-worktree-close".into())
        .spawn(move || {
            let result = close_worktree_panes(
                context.api_connector.as_ref(),
                &checkout_path,
                &pane_ids,
                CONFIRM_TIMEOUT,
            );
            if let Some(runtime) = context.runtime.upgrade() {
                match runtime.lock() {
                    Ok(mut guard) => {
                        guard.ingest_worktree_close_result(id, result);
                    }
                    Err(error) => {
                        trace(
                            &checkout_path,
                            &pane_ids,
                            "publish_failed",
                            Some(&error.to_string()),
                        );
                        return;
                    }
                }
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|error| format!("worktree close worker could not be started: {error}"))
}

pub fn spawn_worktree_open(
    context: LiveContext,
    checkout_path: String,
    repository_root: String,
    pane_id: Option<String>,
) -> Result<(), String> {
    thread::Builder::new()
        .name("herdr-core-worktree-open".into())
        .spawn(move || {
            let result = open_worktree(
                context.api_connector.as_ref(),
                &checkout_path,
                &repository_root,
                pane_id.as_deref(),
            );
            if let Some(runtime) = context.runtime.upgrade() {
                match runtime.lock() {
                    Ok(mut guard) => {
                        guard.ingest_worktree_open_result(checkout_path.clone(), result);
                    }
                    Err(error) => {
                        trace(
                            &checkout_path,
                            &[],
                            "publish_failed",
                            Some(&error.to_string()),
                        );
                        return;
                    }
                }
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|error| format!("worktree open worker could not be started: {error}"))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeTaskRequest {
    pub id: u64,
    pub repository_root: String,
    pub branch: String,
    pub base_branch: Option<String>,
    pub agent_kind: Option<String>,
    pub focus: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeTaskOutcome {
    pub path: String,
    pub pane_id: String,
}

pub fn spawn_worktree_create(
    context: LiveContext,
    request: WorktreeTaskRequest,
) -> Result<(), String> {
    thread::Builder::new()
        .name("herdr-core-worktree-create".into())
        .spawn(move || {
            let result =
                reject_existing_unchecked_out_branch(&request.repository_root, &request.branch)
                    .and_then(|()| create_worktree(context.api_connector.as_ref(), &request));
            if let Some(runtime) = context.runtime.upgrade() {
                if let Ok(mut guard) = runtime.lock() {
                    guard.ingest_task_operation_result(request.id, result);
                } else {
                    return;
                }
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|error| format!("worktree create worker could not be started: {error}"))
}

pub fn spawn_branch_migration(
    context: LiveContext,
    request: WorktreeTaskRequest,
) -> Result<(), String> {
    thread::Builder::new()
        .name("herdr-core-branch-migrate".into())
        .spawn(move || {
            let runner = SystemGit;
            let result = migrate_branch(context.api_connector.as_ref(), &runner, &request);
            if let Some(runtime) = context.runtime.upgrade() {
                if let Ok(mut guard) = runtime.lock() {
                    guard.ingest_task_operation_result(request.id, result);
                } else {
                    return;
                }
                context.notifier.notify();
            }
        })
        .map(|_| ())
        .map_err(|error| format!("branch migration worker could not be started: {error}"))
}

fn create_worktree(
    connector: &dyn ApiConnector,
    request: &WorktreeTaskRequest,
) -> Result<WorktreeTaskOutcome, String> {
    let params = wire::worktree_create_params(
        &request.repository_root,
        &request.branch,
        request.base_branch.as_deref(),
        request.focus,
    )?;
    let result = control_request(connector, "worktree.create", params)
        .map_err(|error| format!("create worktree: {error}"))?;
    let created = wire::created_worktree(result)?;
    let path_exists = Path::new(&created.path).is_dir();
    let listed = control_request(
        connector,
        "worktree.list",
        wire::worktree_list_params(&request.repository_root)?,
    )
    .and_then(|result| wire::listed_worktree_path(result, &request.branch));
    let identity_matches = created.branch.as_deref() == Some(request.branch.as_str())
        && path_exists
        && listed
            .as_ref()
            .ok()
            .and_then(|path| path.as_deref())
            .is_some_and(|path| same_checkout_path(path, &created.path));
    if identity_matches {
        return Ok(WorktreeTaskOutcome {
            path: created.path,
            pane_id: created.pane_id,
        });
    }

    let reason = if created.branch.as_deref() != Some(request.branch.as_str()) {
        format!(
            "created branch mismatch: requested {}, Herdr returned {}",
            request.branch,
            created.branch.as_deref().unwrap_or("detached")
        )
    } else if !path_exists {
        format!("created path is missing on disk: {}", created.path)
    } else {
        let detail = match &listed {
            Ok(Some(path)) => format!("Herdr listed the branch at {path}"),
            Ok(None) => "Herdr did not list the created branch".to_owned(),
            Err(error) => error.clone(),
        };
        format!(
            "created worktree is absent from Herdr's worktree list: {}",
            detail
        )
    };
    let rollback = control_request(
        connector,
        "worktree.remove",
        wire::worktree_remove_params(&created.workspace_id)?,
    );
    if let Err(error) = rollback {
        return Err(format!("{reason}; rollback failed: {error}"));
    }
    let still_listed = control_request(
        connector,
        "worktree.list",
        wire::worktree_list_params(&request.repository_root)?,
    )
    .and_then(|result| wire::listed_worktree_path(result, &request.branch))
    .map(|path| path.is_some())
    .unwrap_or(true);
    if Path::new(&created.path).exists() || still_listed {
        return Err(format!(
            "{reason}; rollback left residue at {} or in Herdr's worktree list",
            created.path
        ));
    }
    Err(format!("{reason}; the created worktree was rolled back"))
}

fn same_checkout_path(left: &str, right: &str) -> bool {
    let left = std::fs::canonicalize(left).unwrap_or_else(|_| Path::new(left).to_path_buf());
    let right = std::fs::canonicalize(right).unwrap_or_else(|_| Path::new(right).to_path_buf());
    left == right
}

fn reject_existing_unchecked_out_branch(repository_root: &str, branch: &str) -> Result<(), String> {
    let reference = format!("refs/heads/{branch}");
    let exists = Command::new("git")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(repository_root)
        .args(["show-ref", "--verify", "--quiet", &reference])
        .output()
        .map_err(|error| format!("git could not be run: {error}"))?;
    if !exists.status.success() {
        return match exists.status.code() {
            Some(1) => Ok(()),
            _ => Err(String::from_utf8_lossy(&exists.stderr).trim().to_owned()),
        };
    }

    let worktrees = Command::new("git")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(repository_root)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .map_err(|error| format!("git could not be run: {error}"))?;
    if !worktrees.status.success() {
        return Err(String::from_utf8_lossy(&worktrees.stderr).trim().to_owned());
    }
    let checked_out = String::from_utf8_lossy(&worktrees.stdout)
        .lines()
        .any(|line| line == format!("branch {reference}"));
    if checked_out {
        // Herdr owns this case and returns Git's worktree-specific
        // "already used by worktree at ..." refusal.
        return Ok(());
    }

    // Ask Git itself for the branch-exists diagnostic. This command is
    // side-effect free because the branch was proven to exist above.
    let refusal = Command::new("git")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(repository_root)
        .args(["branch", "--", branch])
        .output()
        .map_err(|error| format!("git could not be run: {error}"))?;
    if refusal.status.success() {
        return Err("git branch existence check unexpectedly succeeded".into());
    }
    let detail = String::from_utf8_lossy(&refusal.stderr).trim().to_owned();
    Err(if detail.is_empty() {
        format!("git branch exited with {}", refusal.status)
    } else {
        detail
    })
}

trait GitCommands {
    fn run(&self, cwd: &str, args: &[&str]) -> Result<String, String>;
}

struct SystemGit;
impl GitCommands for SystemGit {
    fn run(&self, cwd: &str, args: &[&str]) -> Result<String, String> {
        let output = Command::new("git")
            .arg("--no-optional-locks")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .output()
            .map_err(|error| format!("git could not be run: {error}"))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
        } else {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            Err(if detail.is_empty() {
                format!("git {} exited with {}", args[0], output.status)
            } else {
                format!("git {}: {detail}", args[0])
            })
        }
    }
}

fn migrate_branch(
    connector: &dyn ApiConnector,
    git: &dyn GitCommands,
    request: &WorktreeTaskRequest,
) -> Result<WorktreeTaskOutcome, String> {
    let status = git.run(
        &request.repository_root,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    if !status.is_empty() {
        return Err(
            "preflight: commit or discard uncommitted changes before moving the branch".into(),
        );
    }
    let original = git.run(
        &request.repository_root,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
    )?;
    let Some(base) = request.base_branch.as_deref() else {
        return Err("preflight: the project base branch is unknown".into());
    };
    if original == base {
        return Err(format!("preflight: the main worktree is already on {base}"));
    }
    git.run(
        &request.repository_root,
        &["checkout", "--no-overwrite-ignore", base],
    )
        .map_err(|error| format!("checkout base branch: {error}"))?;

    let create_request = WorktreeTaskRequest {
        branch: original.clone(),
        focus: false,
        ..request.clone()
    };
    match create_worktree(connector, &create_request) {
        Ok(outcome) => Ok(outcome),
        Err(create_error) => match git.run(
            &request.repository_root,
            &["checkout", "--no-overwrite-ignore", &original],
        ) {
            Ok(_) => Err(format!(
                "create worktree: {create_error}; restored the main worktree to {original}"
            )),
            Err(restore_error) => {
                let current = git
                    .run(
                        &request.repository_root,
                        &["symbolic-ref", "--quiet", "--short", "HEAD"],
                    )
                    .unwrap_or_else(|_| "an unknown branch".into());
                Err(format!(
                    "restore original branch: {restore_error}. The main worktree is now on {current}. After resolving the Git error, open the repository at {} and check out {original}.",
                    request.repository_root
                ))
            }
        },
    }
}

fn trace(path: &str, pane_ids: &[String], stage: &str, error: Option<&str>) {
    eprintln!(
        "{}",
        json!({"event":"worktree.control", "path":path, "pane_ids":pane_ids, "stage":stage, "error":error})
    );
}

fn close_worktree_panes(
    connector: &dyn ApiConnector,
    path: &str,
    pane_ids: &[String],
    timeout: Duration,
) -> Result<(), String> {
    let result = (|| {
        for pane_id in pane_ids {
            trace(path, std::slice::from_ref(pane_id), "close_requested", None);
            control_request(connector, "pane.close", json!({"pane_id":pane_id}))?;
        }
        if pane_ids.is_empty() {
            return Ok(());
        }
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| {
                    format!(
                        "Timed out waiting for Herdr to confirm closed panes: {}",
                        pane_ids.join(", ")
                    )
                })?;
            let result =
                request_with_connector(connector, "session.snapshot", json!({}), remaining)
                    .map_err(|error| {
                        format!("session.snapshot close confirmation failed: {error}")
                    })?;
            // No serde defaults here: missing topology is never proof of absence.
            if result["type"] != "session_snapshot" {
                return Err("Herdr close confirmation is not a session_snapshot".into());
            }
            let panes = result
                .pointer("/snapshot/panes")
                .and_then(Value::as_array)
                .ok_or("Herdr close confirmation is missing snapshot.panes")?;
            let (present, pane_at_checkout) = confirmation_state(panes, path)?;
            if pane_ids.iter().all(|id| !present.contains(id)) && !pane_at_checkout {
                return Ok(());
            }
            thread::sleep(CONFIRM_POLL.min(deadline.saturating_duration_since(Instant::now())));
        }
    })();
    trace(
        path,
        pane_ids,
        if result.is_ok() {
            "close_confirmed"
        } else {
            "close_failed"
        },
        result.as_ref().err().map(String::as_str),
    );
    result
}

fn confirmation_state(panes: &[Value], path: &str) -> Result<(Vec<String>, bool), String> {
    let present = panes
        .iter()
        .map(|pane| {
            pane.get("pane_id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| "Herdr close confirmation has an invalid pane_id".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let pane_at_checkout = panes.iter().any(|pane| {
        ["cwd", "foreground_cwd"]
            .into_iter()
            .any(|key| pane.get(key).and_then(Value::as_str) == Some(path))
    });
    Ok((present, pane_at_checkout))
}

fn open_worktree(
    connector: &dyn ApiConnector,
    path: &str,
    repository_root: &str,
    pane_id: Option<&str>,
) -> Result<(), String> {
    let panes = pane_id.into_iter().map(str::to_owned).collect::<Vec<_>>();
    trace(path, &panes, "open_requested", None);
    let result = match pane_id {
        Some(id) => control_request(connector, "pane.focus", json!({"pane_id":id})),
        None => control_request(
            connector,
            "worktree.open",
            json!({"cwd":repository_root,"path":path,"focus":true}),
        ),
    }
    .map(|_| ());
    trace(
        path,
        &panes,
        if result.is_ok() {
            "open_completed"
        } else {
            "open_failed"
        },
        result.as_ref().err().map(String::as_str),
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herdr_api::{ApiError, ApiStream};
    use std::collections::VecDeque;
    use std::os::unix::net::UnixStream;

    // The only fake is the external server's newline-delimited protocol.
    struct Server {
        replies: Mutex<VecDeque<Value>>,
        requests: Arc<Mutex<Vec<Value>>>,
    }
    impl ApiConnector for Server {
        fn connect(&self) -> Result<Box<dyn ApiStream>, ApiError> {
            let reply = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected request");
            let requests = self.requests.clone();
            let (client, mut server) = UnixStream::pair().unwrap();
            thread::spawn(move || {
                let mut line = String::new();
                BufReader::new(server.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                let mut response = reply;
                response["id"] = request["id"].clone();
                requests.lock().unwrap().push(request);
                writeln!(server, "{response}").unwrap();
            });
            Ok(Box::new(client))
        }
    }
    fn server(replies: Vec<Value>) -> Server {
        Server {
            replies: Mutex::new(replies.into()),
            requests: Arc::new(Mutex::new(vec![])),
        }
    }
    fn snapshot(ids: &[&str]) -> Value {
        json!({"result":{"type":"session_snapshot","snapshot":{"panes":ids.iter().map(|id|json!({"pane_id":id})).collect::<Vec<_>>()}}})
    }
    fn workspace_created(path: &str, workspace_id: &str, pane_id: &str) -> Value {
        json!({"result":{
            "type":"workspace_created",
            "workspace":{"workspace_id":workspace_id,"label":"Scratch","number":1,"focused":true,"pane_count":1,"tab_count":1,"active_tab_id":format!("{workspace_id}:t1"),"agent_status":"unknown"},
            "tab":{"workspace_id":workspace_id,"tab_id":format!("{workspace_id}:t1"),"label":"hide codex","number":1,"focused":true,"pane_count":1,"agent_status":"unknown"},
            "root_pane":{"workspace_id":workspace_id,"tab_id":format!("{workspace_id}:t1"),"pane_id":pane_id,"cwd":path,"foreground_cwd":path,"focused":true,"agent_status":"unknown","revision":0,"scroll":{"max_offset_from_bottom":0,"offset_from_bottom":0,"viewport_rows":40},"surface":{"kind":"terminal","attach":{"host":{"host_id":"fixture","session_id":"fixture"},"protocol":21,"terminal_id":"term","transport":"herdr_client"}}}
        }})
    }
    fn tab_created(path: &str, workspace_id: &str, pane_id: &str) -> Value {
        let mut value = workspace_created(path, workspace_id, pane_id);
        let result = value["result"].as_object_mut().unwrap();
        result.insert("type".into(), json!("tab_created"));
        result.remove("workspace");
        value
    }
    fn worktree_created(path: &str, branch: &str) -> Value {
        let mut value = workspace_created(path, "w2", "w2:p1");
        let result = value["result"].as_object_mut().unwrap();
        result.insert("type".into(), json!("worktree_created"));
        result.insert(
            "worktree".into(),
            json!({
                "branch":branch,"is_bare":false,"is_detached":false,
                "is_linked_worktree":true,"is_prunable":false,"label":"repo",
                "open_workspace_id":"w2","path":path
            }),
        );
        value
    }
    fn worktree_list(path: &str, branch: &str) -> Value {
        json!({"result":{"type":"worktree_list","source":{
            "repo_key":"/fixture/repo/.git","repo_name":"repo","repo_root":"/fixture/repo",
            "source_checkout_path":"/fixture/repo","source_workspace_id":"w1"
        },"worktrees":[{
            "branch":branch,"is_bare":false,"is_detached":false,
            "is_linked_worktree":true,"is_prunable":false,"label":"repo",
            "open_workspace_id":"w2","path":path
        }]}})
    }
    struct ScriptedGit {
        replies: Mutex<VecDeque<Result<String, String>>>,
        calls: Mutex<Vec<Vec<String>>>,
    }
    impl ScriptedGit {
        fn new(replies: Vec<Result<&str, &str>>) -> Self {
            Self {
                replies: Mutex::new(
                    replies
                        .into_iter()
                        .map(|result| result.map(str::to_owned).map_err(str::to_owned))
                        .collect(),
                ),
                calls: Mutex::new(Vec::new()),
            }
        }
    }
    impl GitCommands for ScriptedGit {
        fn run(&self, _cwd: &str, args: &[&str]) -> Result<String, String> {
            self.calls
                .lock()
                .unwrap()
                .push(args.iter().map(|arg| (*arg).to_owned()).collect());
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected git call")
        }
    }
    fn task(branch: &str) -> WorktreeTaskRequest {
        WorktreeTaskRequest {
            id: 1,
            repository_root: "/fixture/repo".into(),
            branch: branch.into(),
            base_branch: Some("main".into()),
            agent_kind: None,
            focus: true,
        }
    }

    #[test]
    fn refusal_never_authorizes_removal_or_closes_the_next_pane() {
        let server = server(vec![
            json!({"error":{"code":"confirmation_required","message":"close refused"}}),
        ]);
        let result = close_worktree_panes(
            &server,
            "/fixture/topic",
            &["w1:p1".into(), "w1:p2".into()],
            CONFIRM_TIMEOUT,
        );
        assert!(result.unwrap_err().contains("close refused"));
        assert_eq!(server.requests.lock().unwrap().len(), 1);
    }
    #[test]
    fn every_requested_pane_must_disappear_before_removal_is_ready() {
        let server = server(vec![
            json!({"result":{"type":"ok"}}),
            json!({"result":{"type":"ok"}}),
            snapshot(&["w1:p2", "w2:p1"]),
            snapshot(&["w2:p1"]),
        ]);
        close_worktree_panes(
            &server,
            "/fixture/topic",
            &["w1:p1".into(), "w1:p2".into()],
            CONFIRM_TIMEOUT,
        )
        .unwrap();
        assert!(server.replies.lock().unwrap().is_empty());
        assert_eq!(
            server.requests.lock().unwrap()[1]["params"],
            json!({"pane_id":"w1:p2"})
        );
    }
    #[test]
    fn a_new_pane_at_the_checkout_blocks_removal_authorization() {
        let panes = vec![json!({
            "pane_id":"w2:p1",
            "cwd":"/fixture/topic",
            "foreground_cwd":"/fixture/topic"
        })];
        let (ids, at_checkout) = confirmation_state(&panes, "/fixture/topic").unwrap();
        assert_eq!(ids, vec!["w2:p1"]);
        assert!(at_checkout);
    }
    #[test]
    fn malformed_confirmation_cannot_authorize_removal() {
        for invalid in [
            json!({}),
            json!({"type":"session_snapshot","snapshot":{}}),
            json!({"type":"session_snapshot","snapshot":{"panes":[{}]}}),
        ] {
            let server = server(vec![
                json!({"result":{"type":"ok"}}),
                json!({"result":invalid}),
            ]);
            assert!(
                close_worktree_panes(
                    &server,
                    "/fixture/topic",
                    &["w1:p1".into()],
                    CONFIRM_TIMEOUT
                )
                .is_err()
            );
        }
    }
    #[test]
    fn confirmation_timeout_cannot_authorize_removal() {
        let server = server(vec![json!({"result":{"type":"ok"}}), snapshot(&["w1:p1"])]);
        assert!(
            close_worktree_panes(
                &server,
                "/fixture/topic",
                &["w1:p1".into()],
                Duration::from_millis(20)
            )
            .unwrap_err()
            .contains("Timed out")
        );
    }
    #[test]
    fn open_uses_repository_context_and_existing_pane_focus_is_exact() {
        let server = server(vec![
            json!({"result":{"type":"ok"}}),
            json!({"result":{"type":"ok"}}),
        ]);
        open_worktree(&server, "/fixture/topic", "/fixture/main", None).unwrap();
        open_worktree(&server, "/fixture/topic", "/fixture/main", Some("w1:p2")).unwrap();
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests[0]["method"], "worktree.open");
        assert_eq!(
            requests[0]["params"],
            json!({"cwd":"/fixture/main","path":"/fixture/topic","focus":true})
        );
        assert_eq!(requests[1]["method"], "pane.focus");
        assert_eq!(requests[1]["params"], json!({"pane_id":"w1:p2"}));
    }
    #[test]
    fn open_preserves_the_server_rejection() {
        let server = server(vec![
            json!({"error":{"code":"not_found","message":"checkout no longer exists"}}),
        ]);
        assert!(
            open_worktree(&server, "/fixture/topic", "/fixture/main", None)
                .unwrap_err()
                .contains("checkout no longer exists")
        );
    }

    #[test]
    fn scratch_chat_creates_folder() {
        let root = std::env::temp_dir().join(format!("hide-scratch-create-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let server = server(vec![workspace_created(
            root.to_str().unwrap(),
            "s1",
            "s1:p1",
        )]);
        let pane = create_scratch_tab(
            &server,
            &ScratchTabRequest {
                root: root.to_string_lossy().into_owned(),
                workspace_id: None,
                label: "hide codex".into(),
            },
        )
        .unwrap();
        assert_eq!(pane, "s1:p1");
        assert!(root.is_dir());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scratch_chat_reuses_workspace() {
        let root = std::env::temp_dir().join(format!("hide-scratch-reuse-{}", std::process::id()));
        let server = server(vec![tab_created(root.to_str().unwrap(), "s1", "s1:p2")]);
        let pane = create_scratch_tab(
            &server,
            &ScratchTabRequest {
                root: root.to_string_lossy().into_owned(),
                workspace_id: Some("s1".into()),
                label: "hide codex".into(),
            },
        )
        .unwrap();
        assert_eq!(pane, "s1:p2");
        assert_eq!(server.requests.lock().unwrap()[0]["method"], "tab.create");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scratch_chat_cleanup_on_early_failure() {
        let root = std::env::temp_dir().join(format!("hide-scratch-file-{}", std::process::id()));
        std::fs::write(&root, "not a directory").unwrap();
        let server = server(vec![]);
        let result = create_scratch_tab(
            &server,
            &ScratchTabRequest {
                root: root.join("child").to_string_lossy().into_owned(),
                workspace_id: None,
                label: "hide codex".into(),
            },
        );
        assert!(result.unwrap_err().contains("scratch folder"));
        assert!(server.requests.lock().unwrap().is_empty());
        std::fs::remove_file(root).unwrap();
    }

    #[test]
    fn scratch_chat_late_failure_keeps_tab() {
        let root = std::env::temp_dir().join(format!("hide-scratch-late-{}", std::process::id()));
        let server = server(vec![workspace_created(
            root.to_str().unwrap(),
            "s1",
            "s1:p3",
        )]);
        let created = create_scratch_tab(
            &server,
            &ScratchTabRequest {
                root: root.to_string_lossy().into_owned(),
                workspace_id: None,
                label: "hide codex".into(),
            },
        )
        .unwrap();
        assert_eq!(created, "s1:p3");
        assert_eq!(
            server.requests.lock().unwrap().len(),
            1,
            "a later agent failure has no core cleanup call"
        );
        assert!(root.is_dir());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn created_path_mismatch_is_rolled_back() {
        let missing = "/private/tmp/hide-created-path-missing";
        let server = server(vec![
            worktree_created(missing, "feature"),
            worktree_list(missing, "other"),
            json!({"result":{"type":"ok"}}),
            worktree_list(missing, "other"),
        ]);
        let error = create_worktree(&server, &task("feature")).unwrap_err();
        assert!(error.contains("rolled back"));
        assert!(!Path::new(missing).exists());
        let methods = server
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| request["method"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            methods,
            [
                "worktree.create",
                "worktree.list",
                "worktree.remove",
                "worktree.list"
            ]
        );
    }

    #[test]
    fn created_path_alias_is_not_a_mismatch() {
        let root =
            Path::new("/tmp").join(format!("hide-created-path-alias-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let response_path = root.to_string_lossy().into_owned();
        let listed_path = std::fs::canonicalize(&root)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let server = server(vec![
            worktree_created(&response_path, "feature"),
            worktree_list(&listed_path, "feature"),
        ]);

        let outcome = create_worktree(&server, &task("feature")).unwrap();
        assert_eq!(outcome.path, response_path);
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .map(|request| request["method"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["worktree.create", "worktree.list"]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn existing_unchecked_out_branch_is_rejected_with_gits_message() {
        let root =
            std::env::temp_dir().join(format!("hide-existing-branch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let run = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        run(&["init", "-b", "main"]);
        run(&["config", "user.name", "Fixture"]);
        run(&["config", "user.email", "fixture@example.invalid"]);
        run(&["commit", "--allow-empty", "-m", "seed"]);
        run(&["branch", "existing"]);

        let error =
            reject_existing_unchecked_out_branch(root.to_str().unwrap(), "existing").unwrap_err();
        assert_eq!(error, "fatal: a branch named 'existing' already exists");
        assert!(reject_existing_unchecked_out_branch(root.to_str().unwrap(), "main").is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migrate_branch_blocked_when_dirty() {
        let git = ScriptedGit::new(vec![Ok(" M file")]);
        let server = server(vec![]);
        let error = migrate_branch(&server, &git, &task("feature")).unwrap_err();
        assert!(error.contains("uncommitted changes"));
        assert!(server.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn migrate_branch_moves_branch_to_worktree() {
        let root =
            std::env::temp_dir().join(format!("hide-migrate-success-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.to_string_lossy().into_owned();
        let server = server(vec![
            worktree_created(&path, "feature"),
            worktree_list(&path, "feature"),
        ]);
        let git = ScriptedGit::new(vec![Ok(""), Ok("feature"), Ok("")]);
        let outcome = migrate_branch(&server, &git, &task("feature")).unwrap();
        assert_eq!(outcome.path, path);
        assert_eq!(
            git.calls.lock().unwrap()[2],
            ["checkout", "--no-overwrite-ignore", "main"]
        );
        let requests = server.requests.lock().unwrap();
        let create = requests
            .iter()
            .find(|request| request["method"] == "worktree.create")
            .unwrap();
        assert_eq!(
            create["params"]["focus"], false,
            "migration must not move Herdr focus"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migrate_branch_restores_on_failure() {
        let server = server(vec![
            json!({"error":{"code":"injected","message":"pane creation failed"}}),
        ]);
        let git = ScriptedGit::new(vec![Ok(""), Ok("feature"), Ok(""), Ok("")]);
        let error = migrate_branch(&server, &git, &task("feature")).unwrap_err();
        assert!(error.contains("restored the main worktree to feature"));
        assert_eq!(
            git.calls.lock().unwrap()[3],
            ["checkout", "--no-overwrite-ignore", "feature"]
        );
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .map(|request| request["method"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["worktree.create"]
        );
    }

    #[test]
    fn migrate_branch_leaves_panes_and_focus() {
        let git = ScriptedGit::new(vec![Ok(""), Ok("feature"), Ok(""), Ok("")]);
        let server = server(vec![
            json!({"error":{"code":"injected","message":"create failed"}}),
        ]);
        let _ = migrate_branch(&server, &git, &task("feature"));
        assert!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .all(|request| request["method"] != "pane.focus"
                    && request["method"] != "pane.close")
        );
    }

    #[test]
    fn migrate_branch_does_not_remove_an_unowned_worktree_after_create_failure() {
        let server = server(vec![
            json!({"error":{"code":"injected","message":"create failed after another client used the branch"}}),
        ]);
        let git = ScriptedGit::new(vec![Ok(""), Ok("feature"), Ok(""), Ok("")]);

        let error = migrate_branch(&server, &git, &task("feature")).unwrap_err();

        assert!(error.contains("restored the main worktree to feature"));
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .map(|request| request["method"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["worktree.create"],
            "an ambiguous create failure must not remove a worktree it cannot prove it owns"
        );
    }

    #[test]
    fn migrate_branch_checkout_failure_leaves_repo_untouched() {
        let git = ScriptedGit::new(vec![
            Ok(""),
            Ok("feature"),
            Err("injected checkout failure"),
        ]);
        let server = server(vec![]);
        let error = migrate_branch(&server, &git, &task("feature")).unwrap_err();
        assert!(error.contains("checkout base branch"));
        assert_eq!(git.calls.lock().unwrap().len(), 3);
        assert!(server.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn migrate_branch_refuses_before_overwriting_an_ignored_file() {
        let root = std::env::temp_dir().join(format!(
            "hide-migrate-ignored-collision-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let run = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        run(&["init", "-b", "main"]);
        run(&["config", "user.name", "Fixture"]);
        run(&["config", "user.email", "fixture@example.invalid"]);
        std::fs::write(root.join("ignored.txt"), "base bytes").unwrap();
        run(&["add", "ignored.txt"]);
        run(&["commit", "-m", "base"]);
        run(&["checkout", "-b", "feature"]);
        run(&["rm", "ignored.txt"]);
        std::fs::write(root.join(".gitignore"), "ignored.txt\n").unwrap();
        run(&["add", ".gitignore"]);
        run(&["commit", "-m", "ignore path"]);
        std::fs::write(root.join("ignored.txt"), "private ignored bytes").unwrap();

        let server = server(vec![]);
        let request = WorktreeTaskRequest {
            repository_root: root.to_string_lossy().into_owned(),
            branch: "feature".into(),
            base_branch: Some("main".into()),
            ..task("feature")
        };
        let error = migrate_branch(&server, &SystemGit, &request).unwrap_err();

        assert!(error.contains("checkout base branch"));
        assert_eq!(
            std::fs::read_to_string(root.join("ignored.txt")).unwrap(),
            "private ignored bytes"
        );
        let branch = SystemGit
            .run(
                root.to_str().unwrap(),
                &["symbolic-ref", "--quiet", "--short", "HEAD"],
            )
            .unwrap();
        assert_eq!(branch, "feature");
        assert!(server.requests.lock().unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migrate_branch_restore_failure_reports_repo_state() {
        let server = server(vec![
            json!({"error":{"code":"injected","message":"create failed"}}),
        ]);
        let git = ScriptedGit::new(vec![
            Ok(""),
            Ok("feature"),
            Ok(""),
            Err("injected restore failure"),
            Ok("main"),
        ]);
        let error = migrate_branch(&server, &git, &task("feature")).unwrap_err();
        assert!(error.contains("main worktree is now on main"));
        assert!(error.contains("open the repository at /fixture/repo and check out feature"));
    }
}
