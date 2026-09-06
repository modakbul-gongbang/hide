//! Worktree actions cross the socket boundary before publishing removal readiness.
use super::*;

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
}
