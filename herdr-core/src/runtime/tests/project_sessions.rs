use super::*;
use crate::model::{DeviceSnapshot, ProjectSessionsSnapshot, SessionsSnapshot};
use std::fs;

/// A private HOME holding one Claude Code and one Codex session in `alpha`,
/// one in `zeta`, and one Claude Code file whose only record cannot be read.
struct Fixture {
    _dir: tempfile::TempDir,
    home: PathBuf,
    alpha: PathBuf,
    zeta: PathBuf,
}

fn claude_line(cwd: &Path, at: &str, role: &str, text: &str) -> String {
    let cwd = serde_json::to_string(cwd).unwrap();
    if role == "assistant" {
        format!(
            r#"{{"type":"assistant","cwd":{cwd},"timestamp":"{at}","message":{{"role":"assistant","content":[{{"type":"text","text":"{text}"}}]}}}}"#
        )
    } else {
        format!(
            r#"{{"type":"user","cwd":{cwd},"timestamp":"{at}","userType":"external","promptId":"p-{at}","message":{{"role":"user","content":"{text}"}}}}"#
        )
    }
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    let home = root.join("home");
    let alpha = root.join("alpha");
    let zeta = root.join("zeta");
    for folder in [&alpha, &zeta] {
        fs::create_dir_all(folder).unwrap();
    }
    let claude = home.join(".claude/projects/p");
    let codex = home.join(".codex/sessions/2026/09/21");
    fs::create_dir_all(&claude).unwrap();
    fs::create_dir_all(&codex).unwrap();
    fs::write(
        claude.join("claude-alpha.jsonl"),
        [
            claude_line(
                &alpha,
                "2026-09-21T01:00:00Z",
                "user",
                "배포 스크립트 정리 request",
            ),
            claude_line(&alpha, "2026-09-21T01:00:05Z", "assistant", "Done."),
        ]
        .join("\n")
            + "\n",
    )
    .unwrap();
    fs::write(
        claude.join("claude-zeta.jsonl"),
        claude_line(&zeta, "2026-09-21T03:00:00Z", "user", "zeta request") + "\n",
    )
    .unwrap();
    fs::write(
        claude.join("claude-broken.jsonl"),
        format!(
            r#"{{"type":"user","cwd":{},"timestamp":"not a time","message":{{"content":"lost"}}}}"#,
            serde_json::to_string(&alpha).unwrap()
        ) + "\n",
    )
    .unwrap();
    fs::write(
        codex.join("rollout.jsonl"),
        format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"codex-alpha\",\"cwd\":{}}}}}\n{{\"type\":\"response_item\",\"timestamp\":\"2026-09-21T02:00:00Z\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"codex request\"}}]}}}}\n",
            serde_json::to_string(&alpha).unwrap()
        ),
    )
    .unwrap();
    Fixture {
        _dir: dir,
        home,
        alpha,
        zeta,
    }
}

fn project(path: &Path, checkouts: bool) -> WorkspaceSnapshot {
    let mut project = workspace::inspect_registered(&crate::model::WorkspaceRegistration {
        id: workspace::workspace_id_for_path(path),
        label: "Project".to_owned(),
        path: path.to_string_lossy().into_owned(),
        device_id: workspace::LOCAL_DEVICE_ID.to_owned(),
        pinned: false,
    });
    if !checkouts {
        project.checkouts.clear();
    }
    project
}

/// A runtime whose worker threads read `fixture`'s HOME, with `alpha`
/// (no Workspace) and `zeta` (one Workspace, focused) in its catalog.
fn shared(fixture: &Fixture) -> Arc<Mutex<Runtime>> {
    let mut runtime = runtime();
    runtime.home_path = Some(fixture.home.clone());
    let alpha = project(&fixture.alpha, false);
    let zeta = project(&fixture.zeta, true);
    runtime.snapshot.navigator.focused_workspace_id = Some(zeta.id.clone());
    runtime.snapshot.navigator.focused_checkout_id =
        zeta.checkouts.first().map(|checkout| checkout.id.clone());
    runtime.snapshot.navigator.workspaces = vec![alpha, zeta];
    let shared = Arc::new(Mutex::new(runtime));
    shared
        .lock()
        .unwrap()
        .install_worker_context(Arc::downgrade(&shared), ChangeNotifier::noop());
    shared
}

fn dispatch(shared: &Arc<Mutex<Runtime>>, kind: &str, payload: serde_json::Value) {
    let event =
        serde_json::json!({"schema_version": SCHEMA_VERSION, "kind": kind, "payload": payload});
    shared
        .lock()
        .unwrap()
        .dispatch_json(&serde_json::to_vec(&event).unwrap());
}

/// Waits for the worker threads to settle the named Project.
fn settled(shared: &Arc<Mutex<Runtime>>) -> ProjectSessionsSnapshot {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        {
            let runtime = shared.lock().unwrap();
            let sessions = runtime
                .snapshot
                .project_sessions
                .clone()
                .expect("a named Project");
            let detail_loading = sessions
                .detail
                .as_ref()
                .is_some_and(|detail| detail.loading);
            if !sessions.loading && !detail_loading {
                return sessions;
            }
        }
        assert!(
            Instant::now() < deadline,
            "the session reads did not settle"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn workspace_id(path: &Path) -> String {
    workspace::workspace_id_for_path(path)
}

#[test]
fn a_named_project_lists_its_own_history_newest_first_without_a_workspace() {
    let fixture = fixture();
    let shared = shared(&fixture);

    // `alpha` has no Workspace and `zeta` is focused: the history is
    // `alpha`'s all the same (B1, D-02).
    dispatch(
        &shared,
        "sessions_refresh",
        serde_json::json!({"workspace_id": workspace_id(&fixture.alpha)}),
    );
    let sessions = settled(&shared);

    assert_eq!(sessions.workspace_id, workspace_id(&fixture.alpha));
    assert_eq!(sessions.unavailable_reason, None);
    assert_eq!(sessions.failure, None);
    // Newest first; a file with no readable record is dated by the file.
    let ids = sessions
        .rows
        .iter()
        .map(|row| row.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, ["claude-broken", "codex-alpha", "claude-alpha"]);
    assert_eq!(
        sessions.rows[2].first_human_request.as_deref(),
        Some("배포 스크립트 정리 request")
    );
    // An unreadable source is its own row with a reason in words (B5).
    assert_eq!(
        sessions.rows[0].unavailable_reason.as_deref(),
        Some("The session file could not be parsed.")
    );
    // Nothing of the focused-checkout panel or Memory moved.
    let runtime = shared.lock().unwrap();
    assert_eq!(runtime.snapshot.sessions, SessionsSnapshot::default());
}

#[test]
fn focus_moving_elsewhere_leaves_the_named_project_and_its_history() {
    let fixture = fixture();
    let shared = shared(&fixture);
    dispatch(
        &shared,
        "sessions_refresh",
        serde_json::json!({"workspace_id": workspace_id(&fixture.alpha)}),
    );
    let before = settled(&shared);

    // The operator focuses `zeta`'s Workspace while the screen shows `alpha` (B6).
    {
        let mut runtime = shared.lock().unwrap();
        let zeta = runtime.snapshot.navigator.workspaces[1].clone();
        runtime.focus_checkout(&zeta.id, &zeta.checkouts[0].id);
    }

    let runtime = shared.lock().unwrap();
    assert_eq!(runtime.snapshot.project_sessions.as_ref(), Some(&before));
}

#[test]
fn a_history_read_for_a_project_no_longer_named_cannot_land() {
    let fixture = fixture();
    let shared = shared(&fixture);
    dispatch(
        &shared,
        "sessions_refresh",
        serde_json::json!({"workspace_id": workspace_id(&fixture.zeta)}),
    );
    settled(&shared);
    let zeta_rows = shared
        .lock()
        .unwrap()
        .snapshot
        .project_sessions
        .clone()
        .unwrap()
        .rows;

    let mut runtime = shared.lock().unwrap();
    let stale = runtime.project_sessions_work.list_generation;
    runtime.refresh_project_sessions(None, &workspace_id(&fixture.alpha));
    // `zeta`'s read answers after `alpha` was named (B6).
    let load = crate::runtime::memory::SessionsLoad {
        project_id: "zeta".to_owned(),
        checkout_path: String::new(),
        rows: zeta_rows,
        memories: Vec::new(),
        state: None,
    };
    let landed = runtime.ingest_project_sessions(stale, Ok(load));

    assert!(!landed);
    let sessions = runtime.snapshot.project_sessions.clone().unwrap();
    assert_eq!(sessions.workspace_id, workspace_id(&fixture.alpha));
    assert!(sessions.rows.is_empty());
    assert!(sessions.loading);
}

#[test]
fn a_project_on_a_device_names_the_device_and_reads_no_local_session() {
    let fixture = fixture();
    let shared = shared(&fixture);
    shared.lock().unwrap().snapshot.navigator.devices = vec![
        workspace::local_device(),
        DeviceSnapshot {
            id: "ssh-build".to_owned(),
            label: "build-box".to_owned(),
            kind: "remote".to_owned(),
            state: "ready".to_owned(),
            message: None,
            problem: None,
            ..workspace::local_device()
        },
    ];

    dispatch(
        &shared,
        "sessions_refresh",
        serde_json::json!({"workspace_id": workspace_id(&fixture.alpha), "device_id": "ssh-build"}),
    );

    let runtime = shared.lock().unwrap();
    let sessions = runtime.snapshot.project_sessions.clone().unwrap();
    assert_eq!(
        sessions.unavailable_reason.as_deref(),
        Some(
            "Sessions on build-box are not available here. Hide reads Codex and Claude Code sessions only on This Mac."
        )
    );
    assert!(sessions.rows.is_empty());
    assert!(!sessions.loading);
    assert!(!runtime.project_sessions_work.list_in_flight);
}

#[test]
fn opening_a_session_reads_it_beside_the_history_and_a_retry_rereads_it() {
    let fixture = fixture();
    let shared = shared(&fixture);
    let alpha = workspace_id(&fixture.alpha);
    dispatch(
        &shared,
        "sessions_refresh",
        serde_json::json!({"workspace_id": alpha}),
    );
    settled(&shared);

    dispatch(
        &shared,
        "archive_open",
        serde_json::json!({"kind": "session", "id": "claude-alpha", "workspace_id": alpha}),
    );
    let detail = settled(&shared).detail.expect("an open session");
    assert_eq!(detail.failure, None);
    assert!(detail.locator.ends_with("claude-alpha.jsonl"));
    let archive = detail.archive.expect("its conversation");
    let turns = archive
        .events
        .iter()
        .map(|event| (event.role.as_str(), event.text.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        turns,
        [
            ("user", "배포 스크립트 정리 request"),
            ("assistant", "Done.")
        ]
    );
    // No editor tab: the session never lands in a Workspace (B3).
    assert!(shared.lock().unwrap().snapshot.editor.tabs.is_empty());

    // Reading it again unchanged keeps the shared conversation, so the delta
    // keeps comparing it by pointer.
    dispatch(
        &shared,
        "archive_open",
        serde_json::json!({"kind": "session", "id": "claude-alpha", "workspace_id": alpha}),
    );
    let again = settled(&shared)
        .detail
        .and_then(|detail| detail.archive)
        .expect("still read");
    assert!(Arc::ptr_eq(&archive, &again));

    // Its file goes away; Retry reads the history, then this session. The row
    // stays, unavailable with its last location, and the detail says the same
    // in place (B5, A6).
    let locator = detail.locator.clone();
    fs::remove_file(&locator).unwrap();
    dispatch(
        &shared,
        "sessions_refresh",
        serde_json::json!({"workspace_id": alpha}),
    );
    let sessions = settled(&shared);
    let gone = "The session file can no longer be found. It may have been moved or deleted.";
    let row = sessions
        .rows
        .iter()
        .find(|row| row.id == "claude-alpha")
        .expect("a listed session stays listed after its file went away");
    assert_eq!(row.unavailable_reason.as_deref(), Some(gone));
    assert_eq!(row.locator, locator);
    assert_eq!(
        row.first_human_request.as_deref(),
        Some("배포 스크립트 정리 request")
    );
    let detail = sessions.detail.expect("still open");
    assert_eq!(detail.archive, None);
    assert_eq!(detail.locator, locator);
    assert_eq!(detail.failure.as_deref(), Some(gone));
}

#[test]
fn naming_another_project_closes_the_open_session() {
    let fixture = fixture();
    let shared = shared(&fixture);
    let alpha = workspace_id(&fixture.alpha);
    dispatch(
        &shared,
        "sessions_refresh",
        serde_json::json!({"workspace_id": alpha}),
    );
    settled(&shared);
    dispatch(
        &shared,
        "archive_open",
        serde_json::json!({"kind": "session", "id": "claude-alpha", "workspace_id": alpha}),
    );
    assert!(settled(&shared).detail.is_some());

    dispatch(
        &shared,
        "sessions_refresh",
        serde_json::json!({"workspace_id": workspace_id(&fixture.zeta)}),
    );
    let sessions = settled(&shared);
    assert_eq!(sessions.workspace_id, workspace_id(&fixture.zeta));
    assert_eq!(sessions.detail, None);
    let ids = sessions
        .rows
        .iter()
        .map(|row| row.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, ["claude-zeta"]);
}

#[test]
fn refreshes_during_a_history_read_coalesce_into_one_more_read() {
    let fixture = fixture();
    let shared = shared(&fixture);
    let alpha = workspace_id(&fixture.alpha);
    let mut runtime = shared.lock().unwrap();
    // A read is running: the two requests behind it wait as one.
    runtime.project_sessions_work.list_in_flight = true;
    runtime.refresh_project_sessions(None, &alpha);
    runtime.refresh_project_sessions(None, &alpha);
    assert!(runtime.project_sessions_work.list_waiting);
    let running = runtime.project_sessions_work.list_generation - 2;

    // The running read answers for an older request: it does not land, and
    // exactly one more read starts for the newest.
    let load = crate::runtime::memory::SessionsLoad {
        project_id: "older".to_owned(),
        checkout_path: String::new(),
        rows: Vec::new(),
        memories: Vec::new(),
        state: None,
    };
    runtime.ingest_project_sessions(running, Ok(load));
    assert!(!runtime.project_sessions_work.list_waiting);
    assert!(runtime.project_sessions_work.list_in_flight);
    assert!(runtime.snapshot.project_sessions.as_ref().unwrap().loading);
    drop(runtime);

    let ids = settled(&shared)
        .rows
        .iter()
        .map(|row| row.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(ids, ["claude-broken", "codex-alpha", "claude-alpha"]);
}

#[test]
fn a_history_read_that_fails_settles_the_session_waiting_to_be_read_again() {
    let fixture = fixture();
    let shared = shared(&fixture);
    let alpha = workspace_id(&fixture.alpha);
    dispatch(
        &shared,
        "sessions_refresh",
        serde_json::json!({"workspace_id": alpha}),
    );
    settled(&shared);

    let mut runtime = shared.lock().unwrap();
    // The session opens behind a read of it still running, and a history
    // read is waiting behind another.
    runtime.project_sessions_work.detail_in_flight = true;
    runtime.open_project_session(&alpha, "session", "claude-alpha");
    assert!(runtime.project_sessions_work.detail_waiting);
    runtime.project_sessions_work.list_in_flight = true;
    runtime.refresh_project_sessions(None, &alpha);
    let generation = runtime.project_sessions_work.list_generation;

    // That history read fails: the session is not left reading forever.
    let changed = runtime.ingest_project_sessions(
        generation,
        Err(
            "session_catalog_read_directory:/h/.claude/projects:Permission denied (os error 13)"
                .to_owned(),
        ),
    );
    assert!(changed);
    assert!(!runtime.project_sessions_work.detail_waiting);
    let sessions = runtime.snapshot.project_sessions.clone().unwrap();
    let reason =
        "The session folder /h/.claude/projects could not be read: Permission denied (os error 13)";
    assert_eq!(sessions.failure.as_deref(), Some(reason));
    let detail = sessions.detail.expect("still open");
    assert!(!detail.loading);
    assert_eq!(detail.failure.as_deref(), Some(reason));
}

#[test]
fn an_unreadable_session_opens_as_its_reason() {
    let fixture = fixture();
    let shared = shared(&fixture);
    let alpha = workspace_id(&fixture.alpha);
    dispatch(
        &shared,
        "sessions_refresh",
        serde_json::json!({"workspace_id": alpha}),
    );
    settled(&shared);

    dispatch(
        &shared,
        "archive_open",
        serde_json::json!({"kind": "session", "id": "claude-broken", "workspace_id": alpha}),
    );

    let detail = settled(&shared).detail.expect("an open session");
    assert_eq!(detail.archive, None);
    assert_eq!(
        detail.failure.as_deref(),
        Some("The session file could not be parsed.")
    );
}

#[test]
fn an_open_for_a_project_no_longer_named_changes_nothing() {
    let fixture = fixture();
    let shared = shared(&fixture);
    dispatch(
        &shared,
        "sessions_refresh",
        serde_json::json!({"workspace_id": workspace_id(&fixture.zeta)}),
    );
    let before = settled(&shared);

    dispatch(
        &shared,
        "archive_open",
        serde_json::json!({"kind": "session", "id": "claude-alpha", "workspace_id": workspace_id(&fixture.alpha)}),
    );

    assert_eq!(
        shared.lock().unwrap().snapshot.project_sessions.as_ref(),
        Some(&before)
    );
}

#[test]
fn the_swift_refresh_and_archive_open_keep_their_focused_checkout_meaning() {
    let mut runtime = runtime();

    // The Swift shell sends `{}`; no Project is named and the right panel's
    // Sessions answer for the focused checkout as before.
    for payload in [serde_json::json!({}), serde_json::Value::Null] {
        let event = serde_json::json!({"schema_version": SCHEMA_VERSION, "kind": "sessions_refresh", "payload": payload});
        runtime.dispatch_json(&serde_json::to_vec(&event).unwrap());
        assert_eq!(
            runtime.snapshot.sessions.unavailable_reason.as_deref(),
            Some("Choose a local Project to view sessions")
        );
    }
    let event = serde_json::json!({"schema_version": SCHEMA_VERSION, "kind": "archive_open", "payload": {"kind": "session", "id": "s"}});
    runtime.dispatch_json(&serde_json::to_vec(&event).unwrap());
    assert_eq!(
        runtime
            .snapshot
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        Some("archive.project_unavailable")
    );
    assert_eq!(runtime.snapshot.project_sessions, None);
}

#[test]
fn the_project_sessions_section_rides_the_wire_only_once_a_project_is_named() {
    let fixture = fixture();
    let shared = shared(&fixture);
    let wire = |have: u64| {
        let payload = shared.lock().unwrap().snapshot_delta_payload(have, 0);
        let json: serde_json::Value =
            serde_json::from_slice(&serialize_snapshot_delta(&payload).unwrap()).unwrap();
        (payload.revision, json)
    };

    let (revision, before) = wire(0);
    assert!(before.get("project_sessions").is_none());
    assert!(before["rest"].get("project_sessions").is_none());

    dispatch(
        &shared,
        "sessions_refresh",
        serde_json::json!({"workspace_id": workspace_id(&fixture.alpha)}),
    );
    settled(&shared);
    let (_, after) = wire(revision);
    assert_eq!(
        after["project_sessions"]["rows"].as_array().map(Vec::len),
        Some(3)
    );
    // The history is not resent with the rest of the state.
    assert!(after["rest"].get("project_sessions").is_none());
    let (latest, _) = wire(revision);
    let (_, unchanged) = wire(latest);
    assert!(unchanged.get("project_sessions").is_none());
}

#[test]
fn a_session_whose_file_moved_away_stays_listed_as_unavailable_from_its_memory_record() {
    let fixture = fixture();
    let shared = shared(&fixture);
    // The Memory store sits beside the state file; keep it in the fixture,
    // not in the shared temporary directory every test runtime names.
    let database = {
        let mut runtime = shared.lock().unwrap();
        runtime.state_path = fixture.home.with_file_name("state").join("hide.json");
        runtime.memory_database_path()
    };
    fs::create_dir_all(database.parent().unwrap()).unwrap();
    let identity =
        hide_project::resolve(&fixture.alpha, workspace::LOCAL_DEVICE_ID).expect("alpha resolves");
    let moved = fixture.alpha.join("moved-away.jsonl");
    {
        let store = hide_memory::MemoryStore::open(&database).unwrap();
        store
            .ensure_project(&identity.id, &identity.root, workspace::LOCAL_DEVICE_ID)
            .unwrap();
        store
            .upsert_session_source(&hide_memory::SessionSourceRecord {
                id: "claude-moved".to_owned(),
                project_id: identity.id.clone(),
                provider: "claude".to_owned(),
                locator: moved.to_string_lossy().into_owned(),
                checkout_path: fixture.alpha.to_string_lossy().into_owned(),
                started_at_unix_ms: Some(1),
                updated_at_unix_ms: 2,
                unavailable_reason: None,
            })
            .unwrap();
    }

    let project = workspace_id(&fixture.alpha);
    dispatch(
        &shared,
        "sessions_refresh",
        serde_json::json!({"workspace_id": project}),
    );
    let sessions = settled(&shared);
    let row = sessions
        .rows
        .iter()
        .find(|row| row.id == "claude-moved")
        .expect("a recorded session stays listed after its file moved");
    assert_eq!(row.locator, moved.to_string_lossy());
    assert_eq!(
        row.unavailable_reason.as_deref(),
        Some("Session source is no longer available")
    );

    // Its detail answers with the same reason and reads no file (B5).
    dispatch(
        &shared,
        "archive_open",
        serde_json::json!({"kind": "session", "id": "claude-moved", "workspace_id": project}),
    );
    let detail = settled(&shared).detail.expect("an open session");
    assert_eq!(
        detail.failure.as_deref(),
        Some("Session source is no longer available")
    );
    assert_eq!(detail.locator, moved.to_string_lossy());
    assert!(detail.archive.is_none());
}
