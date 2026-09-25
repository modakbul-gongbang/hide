//! A device's projects are grouped from its helper's facts (PRD S5.5 B1-B4):
//! one repository is one project whatever Herdr workspaces sit in it, a
//! workspace whose tabs sit in two repositories is split between them, the
//! same place on two devices is two projects, and a directory the helper has
//! not answered for is shown unconfirmed rather than guessed.

use super::agents::remote_herdr_workspace;
use super::documents::FakeDevice;
use super::*;
use crate::device_catalog::{self, DeviceFacts, Fact};

const TARGET: &str = "mini";

struct Tree {
    _dir: tempfile::TempDir,
    main: String,
    linked: String,
    other: String,
}

/// A repository with a linked worktree, and a plain folder beside it.
fn tree() -> Tree {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let main = root.join("main");
    let linked = root.join("linked");
    let other = root.join("other");
    std::fs::create_dir_all(main.join(".git/worktrees/linked")).unwrap();
    std::fs::write(main.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::create_dir_all(&linked).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(
        linked.join(".git"),
        format!("gitdir: {}\n", main.join(".git/worktrees/linked").display()),
    )
    .unwrap();
    std::fs::write(
        main.join(".git/worktrees/linked/HEAD"),
        "ref: refs/heads/feature\n",
    )
    .unwrap();
    std::fs::write(main.join(".git/worktrees/linked/commondir"), "../..\n").unwrap();
    std::fs::write(
        main.join(".git/worktrees/linked/gitdir"),
        format!("{}\n", linked.join(".git").display()),
    )
    .unwrap();
    let text = |path: PathBuf| path.to_string_lossy().into_owned();
    Tree {
        _dir: dir,
        main: text(main),
        linked: text(linked),
        other: text(other),
    }
}

/// One Herdr workspace as the session projection reports it: one row, one
/// checkout, a tab per (id, directory).
fn herdr_workspace(
    target: &str,
    wid: &str,
    path: &str,
    tabs: &[(&str, &str)],
) -> WorkspaceSnapshot {
    let workspace_id = format!("remote:{target}:workspace:{wid}");
    let checkout_id = format!("remote:{target}:checkout:{wid}");
    let mut row = checkout(&workspace_id, &checkout_id, path, None);
    row.tabs = tabs
        .iter()
        .map(|(tab, cwd)| TabSnapshot {
            id: Some(format!("remote:{target}:tab:{tab}")),
            workspace_id: Some(workspace_id.clone()),
            checkout_id: Some(checkout_id.clone()),
            label: Some((*tab).to_owned()),
            empty: false,
            delegated: false,
            panes: vec![pane(&format!("remote:{target}:pane:{tab}"), cwd)],
        })
        .collect();
    row.strip = StripTabSnapshot::from_herdr_tabs(&row.tabs);
    row.has_panes = true;
    let mut workspace = workspace(&workspace_id, wid, path, vec![row]);
    workspace.remote_target_id = Some(target.to_owned());
    workspace.device_id = target.to_owned();
    workspace.session_workspace_ids = vec![wid.to_owned()];
    workspace
}

fn session(workspaces: Vec<WorkspaceSnapshot>) -> RemoteSessionSnapshot {
    RemoteSessionSnapshot {
        active_tab_ids: workspaces
            .iter()
            .flat_map(|workspace| &workspace.checkouts)
            .filter_map(|checkout| Some((checkout.id.clone(), checkout.tabs.first()?.id.clone()?)))
            .collect(),
        workspaces,
        agents: Vec::new(),
        focused_workspace_id: None,
        focused_checkout_id: None,
        focused_tab_id: None,
        focused_pane_id: None,
        pane_layouts: Vec::new(),
    }
}

fn answered(paths: &[&str]) -> DeviceFacts {
    DeviceFacts {
        facts: paths
            .iter()
            .map(|path| {
                (
                    (*path).to_owned(),
                    Fact::Known(hide_project::facts(Path::new(path)).unwrap()),
                )
            })
            .collect(),
        ..DeviceFacts::default()
    }
}

/// Each project's path with its checkouts' paths and tab labels.
type Layout = Vec<(String, Vec<(String, Vec<String>)>)>;

fn layout(session: &RemoteSessionSnapshot) -> Layout {
    session
        .workspaces
        .iter()
        .map(|project| {
            (
                project.path.clone(),
                project
                    .checkouts
                    .iter()
                    .map(|checkout| {
                        (
                            checkout.path.clone(),
                            checkout
                                .tabs
                                .iter()
                                .filter_map(|tab| tab.label.clone())
                                .collect(),
                        )
                    })
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn a_devices_workspaces_are_grouped_into_its_repositories_by_its_helper_facts() {
    let t = tree();
    let raw = session(vec![
        herdr_workspace(TARGET, "w1", &t.main, &[("t1", &t.main), ("t2", &t.other)]),
        herdr_workspace(TARGET, "w2", &t.linked, &[("t3", &t.linked)]),
        herdr_workspace(TARGET, "w3", &t.other, &[("t4", &t.other)]),
    ]);
    let grouped = device_catalog::group(TARGET, &raw, &answered(&[&t.main, &t.linked, &t.other]));

    let mut shape = layout(&grouped);
    shape.sort();
    assert_eq!(
        shape,
        vec![
            (
                t.main.clone(),
                vec![
                    (t.main.clone(), vec!["t1".to_owned()]),
                    (t.linked.clone(), vec!["t3".to_owned()]),
                ]
            ),
            (
                t.other.clone(),
                vec![
                    (t.other.clone(), vec!["t2".to_owned()]),
                    (t.other.clone(), vec!["t4".to_owned()]),
                ]
            ),
        ]
    );
    let main = grouped
        .workspaces
        .iter()
        .find(|project| project.path == t.main)
        .unwrap();
    assert!(main.id.starts_with("remote:mini:project:"));
    assert!(main.checkouts[1].is_worktree);
    assert_eq!(main.checkouts[1].branch.as_deref(), Some("feature"));
    // The checkout holding a workspace's own directory keeps its id, and
    // every checkout still names exactly one Herdr workspace.
    assert_eq!(main.checkouts[0].id, "remote:mini:checkout:w1");
    let split = grouped
        .workspaces
        .iter()
        .flat_map(|project| &project.checkouts)
        .find(|checkout| {
            checkout
                .tabs
                .iter()
                .any(|tab| tab.label.as_deref() == Some("t2"))
        })
        .unwrap();
    assert!(split.id.starts_with("remote:mini:checkout:w1#"));
    assert_eq!(
        device_catalog::remote_checkout_source_id(TARGET, &split.id),
        Some("w1")
    );
    for project in &grouped.workspaces {
        for checkout in &project.checkouts {
            assert_eq!(checkout.workspace_id, project.id);
            for tab in &checkout.tabs {
                assert_eq!(tab.workspace_id.as_deref(), Some(project.id.as_str()));
                assert_eq!(tab.checkout_id.as_deref(), Some(checkout.id.as_str()));
            }
        }
    }
    assert_eq!(
        remote_herdr_workspace(&grouped, TARGET, &main.id, Some("remote:mini:checkout:w2"))
            .as_deref(),
        Some("w2")
    );
    assert_eq!(
        remote_herdr_workspace(&grouped, TARGET, &main.id, None),
        None,
        "a project of several workspaces needs the checkout to name one"
    );

    // B2: the same place on another device is another project.
    let elsewhere = session(vec![herdr_workspace(
        "studio",
        "w1",
        &t.main,
        &[("t1", &t.main)],
    )]);
    let studio = device_catalog::group("studio", &elsewhere, &answered(&[&t.main]));
    assert_ne!(studio.workspaces[0].id, main.id);
}

#[test]
fn a_directory_the_helper_has_not_confirmed_is_shown_as_its_workspace_and_says_why() {
    let t = tree();
    let raw = session(vec![
        herdr_workspace(TARGET, "w1", &t.main, &[("t1", &t.main)]),
        herdr_workspace(TARGET, "w3", &t.other, &[("t4", &t.other)]),
    ]);
    let mut facts = answered(&[&t.main]);
    let grouped = device_catalog::group(TARGET, &raw, &facts);
    assert!(
        grouped
            .workspaces
            .iter()
            .any(|row| row.id == "remote:mini:workspace:w3" && row.path == t.other)
    );
    assert_eq!(
        device_catalog::catalog_state(&raw, &facts).state,
        "resolving"
    );

    facts.unavailable = Some("The device helper is not allowed".to_owned());
    let state = device_catalog::catalog_state(&raw, &facts);
    assert_eq!(state.state, "unavailable");
    assert_eq!(
        state.message.as_deref(),
        Some("The device helper is not allowed")
    );

    facts.unavailable = None;
    facts
        .facts
        .insert(t.other.clone(), Fact::Refused("No such folder".to_owned()));
    let state = device_catalog::catalog_state(&raw, &facts);
    assert_eq!(state.state, "ready");
    assert_eq!(state.refused.len(), 1);
    assert_eq!(state.refused[0].path, t.other);
}

/// The published session follows the helper: unconfirmed while it is asked
/// on a worker, grouped when it answers.
#[test]
fn a_device_session_is_grouped_when_its_helper_answers() {
    let t = tree();
    let mut runtime = runtime();
    runtime.snapshot.status.remote.push(RemoteStatusSnapshot {
        target_id: TARGET.to_owned(),
        state: "connected".to_owned(),
        message: None,
        herdr_version: None,
        session: None,
        files: RemoteFileListSnapshot::idle(),
        catalog: Default::default(),
    });
    let device = FakeDevice::new();
    runtime.device_hosts.insert(
        TARGET.to_owned(),
        hosts::DeviceHost {
            phase: hosts::HostPhase::Ready {
                host: device.clone(),
                platform: "macos aarch64".to_owned(),
                helper_path: "/fake/hide-host-helper".to_owned(),
            },
            generation: 1,
        },
    );
    let shared = Arc::new(Mutex::new(runtime));
    shared
        .lock()
        .unwrap()
        .install_worker_context(Arc::downgrade(&shared), crate::ffi::ChangeNotifier::noop());
    let raw = session(vec![
        herdr_workspace(TARGET, "w1", &t.main, &[("t1", &t.main)]),
        herdr_workspace(TARGET, "w2", &t.linked, &[("t3", &t.linked)]),
    ]);

    device.hold();
    shared
        .lock()
        .unwrap()
        .ingest_remote_session(TARGET, Ok(raw.clone()));
    {
        let runtime = shared.lock().unwrap();
        let status = &runtime.snapshot.status.remote[0];
        assert_eq!(status.catalog.state, "resolving");
        assert_eq!(status.session.as_ref().unwrap().workspaces.len(), 2);
    }
    device.release();
    let deadline = Instant::now() + Duration::from_secs(5);
    while shared.lock().unwrap().snapshot.status.remote[0]
        .catalog
        .state
        != "ready"
    {
        assert!(Instant::now() < deadline, "the helper's facts never landed");
        thread::sleep(Duration::from_millis(5));
    }
    let mut runtime = shared.lock().unwrap();
    let grouped = runtime.snapshot.status.remote[0].session.clone().unwrap();
    assert_eq!(grouped.workspaces.len(), 1);
    assert_eq!(grouped.workspaces[0].checkouts.len(), 2);
    assert!(
        !runtime.ingest_remote_session(TARGET, Ok(raw)),
        "the same Herdr session is not a change once grouped"
    );
}

#[test]
fn a_device_whose_helper_is_not_allowed_shows_its_workspaces_unconfirmed() {
    let t = tree();
    let mut runtime = runtime();
    runtime.snapshot.status.remote.push(RemoteStatusSnapshot {
        target_id: TARGET.to_owned(),
        state: "connected".to_owned(),
        message: None,
        herdr_version: None,
        session: None,
        files: RemoteFileListSnapshot::idle(),
        catalog: Default::default(),
    });
    runtime.ingest_remote_session(
        TARGET,
        Ok(session(vec![herdr_workspace(
            TARGET,
            "w1",
            &t.main,
            &[("t1", &t.main)],
        )])),
    );
    let status = &runtime.snapshot.status.remote[0];
    assert_eq!(status.catalog.state, "unavailable");
    assert!(status.catalog.message.is_some());
    assert_eq!(
        status.session.as_ref().unwrap().workspaces[0].id,
        "remote:mini:workspace:w1"
    );
}

/// An attempt started before a device was removed and added again under the
/// same id answers late: its failure or its close must not touch the new
/// connection, whose attempt number the removal did not reset.
#[test]
fn a_helper_attempt_from_before_a_removal_cannot_settle_the_new_connection() {
    let mut runtime = runtime();
    let stale = runtime.advance_host_generation(TARGET);
    runtime.forget_device_host(TARGET);
    let current = runtime.advance_host_generation(TARGET);
    assert_ne!(stale, current);
    let device = FakeDevice::new();
    runtime.device_hosts.get_mut(TARGET).unwrap().phase = hosts::HostPhase::Ready {
        host: device,
        platform: "macos aarch64".to_owned(),
        helper_path: "/fake/hide-host-helper".to_owned(),
    };

    assert!(!runtime.ingest_host_established(
        TARGET,
        stale,
        Err(crate::remote::host::EstablishError::Helper(
            "old attempt".to_owned()
        )),
    ));
    assert!(!runtime.ingest_host_closed(TARGET, stale, "old attempt closed".to_owned()));
    assert!(matches!(
        runtime.device_hosts[TARGET].phase,
        hosts::HostPhase::Ready { .. }
    ));
    assert!(runtime.ingest_host_closed(TARGET, current, "closed".to_owned()));
}

fn dispatch(runtime: &mut Runtime, kind: &str, payload: serde_json::Value) {
    let event =
        serde_json::json!({"schema_version": SCHEMA_VERSION, "kind": kind, "payload": payload});
    runtime.dispatch_json(&serde_json::to_vec(&event).unwrap());
}

/// B3, B23-B25: a project registered on a device is listed there with no
/// Herdr workspace in it, is pinned and removed as a local one is, and a
/// registration of the same path on this machine never shows on the device
/// nor leaves with it; a Herdr-only project on the device is not registered.
#[test]
fn a_device_registration_is_listed_without_panes_pinned_and_removed_on_that_device_only() {
    let t = tree();
    let mut runtime = runtime();
    runtime.snapshot.status.remote.push(RemoteStatusSnapshot {
        target_id: TARGET.to_owned(),
        state: "connected".to_owned(),
        message: None,
        herdr_version: None,
        session: None,
        files: RemoteFileListSnapshot::idle(),
        catalog: Default::default(),
    });
    runtime.device_hosts.insert(
        TARGET.to_owned(),
        hosts::DeviceHost {
            phase: hosts::HostPhase::Ready {
                host: FakeDevice::new(),
                platform: "macos aarch64".to_owned(),
                helper_path: "/fake/hide-host-helper".to_owned(),
            },
            generation: 1,
        },
    );
    runtime
        .snapshot
        .ui_state
        .workspace_registrations
        .push(crate::model::WorkspaceRegistration {
            id: "workspace:local-other".to_owned(),
            label: "Local other".to_owned(),
            path: t.other.clone(),
            device_id: "local".to_owned(),
            pinned: false,
        });
    runtime.ingest_remote_session(
        TARGET,
        Ok(session(vec![herdr_workspace(
            TARGET,
            "w1",
            &t.main,
            &[("t1", &t.main)],
        )])),
    );
    let rows = |runtime: &Runtime| {
        runtime.snapshot.status.remote[0]
            .session
            .as_ref()
            .unwrap()
            .workspaces
            .iter()
            .map(|project| (project.path.clone(), project.registered, project.pinned))
            .collect::<Vec<_>>()
    };
    assert_eq!(rows(&runtime), vec![(t.main.clone(), false, false)]);

    assert!(runtime.ingest_device_registration(
        TARGET,
        "Other".to_owned(),
        Ok(hide_host::register::Registrable {
            root: t.other.clone(),
            is_git: false,
        }),
    ));
    let id = device_catalog::project_id(TARGET, Path::new(&t.other));
    let session = runtime.snapshot.status.remote[0].session.clone().unwrap();
    let registered = session
        .workspaces
        .iter()
        .find(|project| project.id == id)
        .unwrap();
    assert!(registered.registered);
    assert_eq!(registered.label, "Other");
    assert_eq!(registered.checkouts.len(), 1);
    assert!(registered.checkouts[0].tabs.is_empty());
    assert_eq!(
        device_catalog::remote_checkout_source_id(TARGET, &registered.checkouts[0].id),
        None,
        "a registration-only checkout names no Herdr workspace"
    );
    assert_eq!(rows(&runtime).len(), 2);

    dispatch(
        &mut runtime,
        "workspace_pin_set",
        serde_json::json!({"workspace_id": id, "pinned": true}),
    );
    assert_eq!(rows(&runtime)[0], (t.other.clone(), true, true));

    dispatch(
        &mut runtime,
        "remove_workspace",
        serde_json::json!({"workspace_id": id}),
    );
    assert_eq!(rows(&runtime), vec![(t.main.clone(), false, false)]);
    let registrations = &runtime.snapshot.ui_state.workspace_registrations;
    assert_eq!(registrations.len(), 1);
    assert_eq!(registrations[0].device_id, "local");
    assert!(
        Path::new(&t.other).is_dir(),
        "removal never touches the folder"
    );
}

/// B24: a device's helper judges a folder against that device's home, so a
/// folder outside it is refused with the reason and nothing is registered.
#[test]
fn a_device_folder_outside_its_home_is_refused_by_its_helper() {
    let t = tree();
    let mut runtime = runtime();
    runtime.device_hosts.insert(
        TARGET.to_owned(),
        hosts::DeviceHost {
            phase: hosts::HostPhase::Ready {
                host: FakeDevice::new(),
                platform: "macos aarch64".to_owned(),
                helper_path: "/fake/hide-host-helper".to_owned(),
            },
            generation: 1,
        },
    );
    // The double answers with this process's home; the fixture is a
    // temporary folder outside it.
    assert!(!Path::new(&t.other).starts_with(std::env::var("HOME").unwrap()));

    dispatch(
        &mut runtime,
        "create_workspace",
        serde_json::json!({"device_id": TARGET, "path": t.other, "label": "Other", "initialize_git": false}),
    );

    let error = runtime.snapshot.status.last_error.clone().unwrap();
    assert_eq!(error.kind, "workspace.create_refused");
    assert!(
        error.message.contains("inside the home folder"),
        "{}",
        error.message
    );
    assert!(runtime.snapshot.ui_state.workspace_registrations.is_empty());
}

/// B23: a new tab in a device project Herdr has no workspace in creates one
/// at the registered folder on that device; a registration-only checkout
/// id for a project that is not registered there names nothing.
#[test]
fn a_tab_in_a_device_registration_without_a_workspace_creates_one_there() {
    let t = tree();
    let mut runtime = runtime();
    runtime.snapshot.status.remote.push(RemoteStatusSnapshot {
        target_id: TARGET.to_owned(),
        state: "connected".to_owned(),
        message: None,
        herdr_version: Some("0.9.1".to_owned()),
        session: None,
        files: RemoteFileListSnapshot::idle(),
        catalog: Default::default(),
    });
    runtime.ingest_remote_session(TARGET, Ok(session(Vec::new())));
    let connector: Arc<dyn hide_herdr_client::ApiConnector> = Arc::new(
        hide_herdr_client::UnixSocketConnector::new("/tmp/herdr-core-never-connect.sock"),
    );
    runtime.install_remote_control(RemoteControlContext::new(
        TARGET,
        connector,
        Weak::new(),
        ChangeNotifier::noop(),
    ));
    // A registration answer comes from the device's ready helper.
    runtime.device_hosts.insert(
        TARGET.to_owned(),
        hosts::DeviceHost {
            phase: hosts::HostPhase::Ready {
                host: FakeDevice::new(),
                platform: "macos aarch64".to_owned(),
                helper_path: "/fake/hide-host-helper".to_owned(),
            },
            generation: 1,
        },
    );
    assert!(runtime.ingest_device_registration(
        TARGET,
        "Other".to_owned(),
        Ok(hide_host::register::Registrable {
            root: t.other.clone(),
            is_git: false,
        }),
    ));
    let id = device_catalog::project_id(TARGET, Path::new(&t.other));
    let create = |runtime: &mut Runtime, workspace_id: &str, request: &str| {
        runtime.request_remote_control(RemoteControlPayload {
            target_id: TARGET.to_owned(),
            request_id: request.to_owned(),
            report_pane_focus_outcome: false,
            focus_device: false,
            request: RemoteControlRequest::CreateTab {
                workspace_id: workspace_id.to_owned(),
                checkout_id: Some(format!("{workspace_id}#registered")),
                cwd: "/elsewhere".to_owned(),
                label: "Tab 1".to_owned(),
            },
        })
    };

    assert!(create(&mut runtime, &id, "request-1"));
    assert_eq!(runtime.snapshot.status.last_error, None);
    assert!(
        runtime
            .snapshot
            .status
            .diagnostics
            .iter()
            .any(|row| row.kind == "remote.control.requested"
                && row.message.contains("workspace.create")),
        "the device is asked to create a workspace"
    );
    assert!(
        runtime
            .remote_tab_creations_in_flight
            .iter()
            .any(|key| key.2 == t.other),
        "it is created at the registered folder, not the path the shell sent"
    );

    create(
        &mut runtime,
        "remote:mini:project:unregistered",
        "request-2",
    );
    assert_eq!(
        runtime
            .snapshot
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        Some("remote.control.workspace_not_found")
    );
}

/// A Herdr socket that records each request and answers none, so a test sees
/// exactly what a device's Herdr was asked.
struct RecordingHerdr {
    requests: Arc<Mutex<Vec<serde_json::Value>>>,
}

impl hide_herdr_client::ApiConnector for RecordingHerdr {
    fn connect(
        &self,
    ) -> Result<Box<dyn hide_herdr_client::ApiStream>, hide_herdr_client::ApiError> {
        use std::io::BufRead;
        let (client, server) = std::os::unix::net::UnixStream::pair().unwrap();
        let requests = self.requests.clone();
        std::thread::spawn(move || {
            let mut line = String::new();
            if std::io::BufReader::new(server).read_line(&mut line).is_ok()
                && let Ok(request) = serde_json::from_str(&line)
            {
                requests.lock().unwrap().push(request);
            }
        });
        Ok(Box::new(client))
    }
}

fn device_strip(runtime: &Runtime, checkout_id: &str) -> Vec<String> {
    runtime.snapshot.status.remote[0]
        .session
        .as_ref()
        .unwrap()
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.checkouts.iter())
        .find(|checkout| checkout.id == checkout_id)
        .unwrap()
        .strip
        .iter()
        .map(|entry| entry.id.clone())
        .collect()
}

/// B23: a device's strip is arranged as this machine's is. A file tab keeps
/// the slot it was dropped in without asking Herdr; a Herdr tab moves on the
/// device's own Herdr by the id that Herdr knows, and the strip follows only
/// when the device reports the new order. A device that is not connected
/// takes no move.
#[test]
fn a_device_tab_moves_on_its_own_herdr_and_a_file_tab_keeps_the_slot_it_was_dropped_in() {
    let t = tree();
    let mut runtime = runtime();
    runtime.snapshot.status.remote.push(RemoteStatusSnapshot {
        target_id: TARGET.to_owned(),
        state: "connected".to_owned(),
        message: None,
        herdr_version: Some("0.9.1".to_owned()),
        session: None,
        files: RemoteFileListSnapshot::idle(),
        catalog: Default::default(),
    });
    let raw = |order: &[&str]| {
        let tabs = order
            .iter()
            .map(|tab| (*tab, t.main.as_str()))
            .collect::<Vec<_>>();
        session(vec![herdr_workspace(TARGET, "w1", &t.main, &tabs)])
    };
    runtime.ingest_remote_session(TARGET, Ok(raw(&["t1", "t2"])));
    let requests = Arc::new(Mutex::new(Vec::new()));
    runtime.install_remote_control(RemoteControlContext::new(
        TARGET,
        Arc::new(RecordingHerdr {
            requests: requests.clone(),
        }),
        Weak::new(),
        ChangeNotifier::noop(),
    ));
    let workspace_id = format!("remote:{TARGET}:workspace:w1");
    let checkout_id = format!("remote:{TARGET}:checkout:w1");
    runtime.snapshot.editor.tabs.push(EditorTabSnapshot {
        id: "file:device".to_owned(),
        workspace_id: workspace_id.clone(),
        checkout_id: checkout_id.clone(),
        path: format!("{}/README.md", t.main),
        label: "README.md".to_owned(),
        kind: EditorTabKind::File,
        diff_committed: None,
        markdown_live: false,
        wrap: false,
        dirty: false,
        preview: false,
        unavailable_reason: None,
    });
    runtime.rebuild_tab_strips();
    let t1 = format!("herdr:remote:{TARGET}:tab:t1");
    let t2 = format!("herdr:remote:{TARGET}:tab:t2");
    let file = "file:file:device".to_owned();
    assert_eq!(
        device_strip(&runtime, &checkout_id),
        [t1.clone(), t2.clone(), file.clone()]
    );
    let reorder = |runtime: &mut Runtime, entry: &str, to_index: usize| {
        dispatch(
            runtime,
            "reorder_tab",
            serde_json::json!({
                "workspace_id": workspace_id,
                "checkout_id": checkout_id,
                "tab_id": entry,
                "to_index": to_index,
            }),
        );
    };

    reorder(&mut runtime, &file, 0);
    assert_eq!(
        device_strip(&runtime, &checkout_id),
        [file.clone(), t1.clone(), t2.clone()],
        "a file tab's slot is Hide's and lands at once"
    );
    assert!(runtime.pending_tab_move.is_empty());

    reorder(&mut runtime, &t2, 1);
    assert_eq!(runtime.snapshot.status.last_error, None);
    assert!(runtime.pending_tab_move.contains_key(&checkout_id));
    assert_eq!(
        device_strip(&runtime, &checkout_id),
        [file.clone(), t1.clone(), t2.clone()],
        "the strip waits for the device's Herdr"
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while requests.lock().unwrap().is_empty() {
        assert!(
            Instant::now() < deadline,
            "the device's Herdr was not asked"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let request = requests.lock().unwrap()[0].clone();
    assert_eq!(request["method"], "tab.move");
    assert_eq!(
        request["params"]["tab_id"], "t2",
        "the id the device's Herdr knows"
    );

    runtime.ingest_remote_session(TARGET, Ok(raw(&["t2", "t1"])));
    assert_eq!(
        device_strip(&runtime, &checkout_id),
        [file.clone(), t2.clone(), t1.clone()]
    );
    assert!(runtime.pending_tab_move.is_empty());

    runtime.snapshot.status.remote[0].state = "disconnected".to_owned();
    reorder(&mut runtime, &t1, 1);
    assert_eq!(
        runtime
            .snapshot
            .status
            .last_error
            .as_ref()
            .map(|error| error.kind.as_str()),
        Some("tab.control_unavailable")
    );
    assert!(runtime.pending_tab_move.is_empty());
    assert_eq!(requests.lock().unwrap().len(), 1);
}

/// B26: removing a device removes Hide's record of it and nothing else. Its
/// project registrations, expanded folders, file tabs and closed items go;
/// this machine's stay; and the device's Herdr is not asked for anything.
#[test]
fn removing_a_device_forgets_its_projects_tabs_and_folders_and_keeps_this_machines() {
    let mut runtime = runtime();
    runtime
        .snapshot
        .ui_state
        .device_registrations
        .push(crate::model::DeviceRegistration {
            id: TARGET.to_owned(),
            label: "Mini".to_owned(),
            ssh_alias: Some(TARGET.to_owned()),
            herdr_socket_path: None,
            host_consent: None,
        });
    let registration = |id: &str, device: &str| crate::model::WorkspaceRegistration {
        id: id.to_owned(),
        label: id.to_owned(),
        path: format!("/repo/{id}"),
        device_id: device.to_owned(),
        pinned: true,
    };
    runtime.snapshot.ui_state.workspace_registrations = vec![
        registration("workspace:here", "local"),
        registration(&format!("remote:{TARGET}:project:p1"), TARGET),
    ];
    runtime
        .snapshot
        .ui_state
        .device_expanded_paths
        .insert(TARGET.to_owned(), vec!["/repo/p1/src".to_owned()]);
    let tab = |id: &str, checkout: &str| EditorTabSnapshot {
        id: id.to_owned(),
        workspace_id: "workspace".to_owned(),
        checkout_id: checkout.to_owned(),
        path: format!("/repo/{id}"),
        label: id.to_owned(),
        kind: EditorTabKind::File,
        diff_committed: None,
        markdown_live: false,
        wrap: false,
        dirty: true,
        preview: false,
        unavailable_reason: None,
    };
    runtime.snapshot.editor.tabs = vec![
        tab("file:here", "checkout:here"),
        tab("file:device", &format!("remote:{TARGET}:checkout:w1")),
    ];
    runtime.push_recent_closed(ClosedItem::File {
        key: "closed-device".to_owned(),
        device_id: TARGET.to_owned(),
        workspace_id: "workspace".to_owned(),
        checkout_id: format!("remote:{TARGET}:checkout:w1"),
        checkout_path: "/repo/p1".to_owned(),
        path: "/repo/p1/gone.txt".to_owned(),
        label: "gone.txt".to_owned(),
    });

    dispatch(
        &mut runtime,
        "remove_device",
        serde_json::json!({"device_id": TARGET}),
    );

    assert!(runtime.snapshot.ui_state.device_registrations.is_empty());
    assert_eq!(
        runtime
            .snapshot
            .ui_state
            .workspace_registrations
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["workspace:here"]
    );
    assert!(runtime.snapshot.ui_state.device_expanded_paths.is_empty());
    assert_eq!(
        runtime
            .snapshot
            .editor
            .tabs
            .iter()
            .map(|tab| tab.id.as_str())
            .collect::<Vec<_>>(),
        ["file:here"]
    );
    assert!(runtime.recent_closed.is_empty());
    assert_eq!(runtime.snapshot.status.last_error, None);
}

/// A device removed while its helper judged a folder takes no registration
/// from that late answer.
#[test]
fn a_registration_answer_after_its_device_was_removed_is_dropped() {
    let t = tree();
    let mut runtime = runtime();
    assert!(!runtime.ingest_device_registration(
        TARGET,
        "Other".to_owned(),
        Ok(hide_host::register::Registrable {
            root: t.other.clone(),
            is_git: false,
        }),
    ));
    assert!(runtime.snapshot.ui_state.workspace_registrations.is_empty());
}

/// S6 D-08, B12, B21: an agent chosen on a device is one event that brings
/// the device forward, and the Agent area comes back on the Workspace that
/// holds the agent, not on the one the device showed before its Herdr moved.
#[test]
fn a_device_agent_opened_from_views_only_brings_its_own_workspace_to_together() {
    use crate::workspace_views::ViewMode;
    let t = tree();
    let mut runtime = runtime();
    runtime.snapshot.navigator.devices.push(DeviceSnapshot {
        id: TARGET.to_owned(),
        label: "Mac mini".to_owned(),
        kind: "remote".to_owned(),
        state: "connected".to_owned(),
        message: None,
        problem: None,
        ssh_alias: Some(TARGET.to_owned()),
        herdr_socket_path: None,
        agent_count: 0,
        test: None,
        host: Default::default(),
    });
    runtime.snapshot.status.remote.push(RemoteStatusSnapshot {
        target_id: TARGET.to_owned(),
        state: "connected".to_owned(),
        message: None,
        herdr_version: Some("0.9.1".to_owned()),
        session: None,
        files: RemoteFileListSnapshot::idle(),
        catalog: Default::default(),
    });
    let mut raw = session(vec![
        herdr_workspace(TARGET, "w1", &t.main, &[("t1", &t.main)]),
        herdr_workspace(TARGET, "w2", &t.linked, &[("t3", &t.linked)]),
    ]);
    raw.focused_workspace_id = Some(format!("remote:{TARGET}:workspace:w1"));
    raw.focused_checkout_id = Some(format!("remote:{TARGET}:checkout:w1"));
    runtime.ingest_remote_session(TARGET, Ok(raw));
    let connector: Arc<dyn hide_herdr_client::ApiConnector> = Arc::new(
        hide_herdr_client::UnixSocketConnector::new("/tmp/herdr-core-never-connect.sock"),
    );
    runtime.install_remote_control(RemoteControlContext::new(
        TARGET,
        connector,
        Weak::new(),
        ChangeNotifier::noop(),
    ));
    let views = tempfile::tempdir().unwrap();
    let mut store = WorkspaceViewStore::open(views.path().join("views.json"), Default::default()).0;
    store.views.entry(TARGET, &t.main).mode = ViewMode::Views;
    store.views.entry(TARGET, &t.linked).mode = ViewMode::Views;
    runtime.workspace_views = Some(store);

    runtime.request_remote_control(RemoteControlPayload {
        target_id: TARGET.to_owned(),
        request_id: "open-agent".to_owned(),
        report_pane_focus_outcome: false,
        focus_device: true,
        request: RemoteControlRequest::FocusPane {
            pane_id: format!("remote:{TARGET}:pane:t3"),
        },
    });

    assert_eq!(runtime.snapshot.status.last_error, None);
    assert_eq!(
        runtime.snapshot.navigator.focused_device_id.as_deref(),
        Some(TARGET)
    );
    let mode = |runtime: &Runtime, path: &str| {
        runtime
            .workspace_views
            .as_ref()
            .unwrap()
            .views
            .get(TARGET, path)
            .unwrap()
            .mode
    };
    assert_eq!(mode(&runtime, &t.linked), ViewMode::Together);
    assert_eq!(mode(&runtime, &t.main), ViewMode::Views);

    // D-11: the choice is remembered only once the device's Herdr has moved
    // there, so a refusal on the device would remember nothing.
    let resumed = |runtime: &mut Runtime| {
        runtime.sync_workspace_view();
        let view = runtime.snapshot.workspace_view.clone().expect("front");
        (view.path, view.resumed)
    };
    assert_eq!(resumed(&mut runtime), (t.main.clone(), false));
    let moved = |runtime: &mut Runtime, workspace: &str| {
        let mut raw = session(vec![
            herdr_workspace(TARGET, "w1", &t.main, &[("t1", &t.main)]),
            herdr_workspace(TARGET, "w2", &t.linked, &[("t3", &t.linked)]),
        ]);
        raw.focused_workspace_id = Some(format!("remote:{TARGET}:workspace:{workspace}"));
        raw.focused_checkout_id = Some(format!("remote:{TARGET}:checkout:{workspace}"));
        runtime.ingest_remote_session(TARGET, Ok(raw));
    };
    moved(&mut runtime, "w2");
    assert_eq!(resumed(&mut runtime), (t.linked.clone(), true));

    // A Workspace opened from Main or an Overview is chosen the same way.
    runtime.request_remote_control(RemoteControlPayload {
        target_id: TARGET.to_owned(),
        request_id: "open-workspace".to_owned(),
        report_pane_focus_outcome: false,
        focus_device: true,
        request: RemoteControlRequest::FocusWorkspace {
            workspace_id: format!("remote:{TARGET}:workspace:w1"),
            checkout_id: Some(format!("remote:{TARGET}:checkout:w1")),
        },
    });
    assert_eq!(runtime.snapshot.status.last_error, None);
    assert_eq!(resumed(&mut runtime), (t.linked.clone(), true));
    moved(&mut runtime, "w1");
    assert_eq!(resumed(&mut runtime), (t.main.clone(), true));

    // A request the device refuses chooses nothing, even when its Herdr
    // later moves there on its own.
    runtime.request_remote_control(RemoteControlPayload {
        target_id: TARGET.to_owned(),
        request_id: "refused".to_owned(),
        report_pane_focus_outcome: false,
        focus_device: true,
        request: RemoteControlRequest::FocusWorkspace {
            workspace_id: format!("remote:{TARGET}:workspace:w2"),
            checkout_id: Some(format!("remote:{TARGET}:checkout:w2")),
        },
    });
    runtime.ingest_remote_control_result(
        TARGET,
        "refused",
        RemoteControlAction::FocusWorkspace {
            workspace_id: "w2".to_owned(),
        },
        Err("the workspace is gone".to_owned()),
        3,
    );
    moved(&mut runtime, "w2");
    assert_eq!(resumed(&mut runtime), (t.linked.clone(), false));
}

/// S6 B21: a device request refused before it is sent leaves the device
/// that was in front where it was.
#[test]
fn a_refused_device_request_does_not_bring_the_device_forward() {
    let mut runtime = runtime();
    runtime.snapshot.status.remote.push(RemoteStatusSnapshot {
        target_id: TARGET.to_owned(),
        state: "not_connected".to_owned(),
        message: None,
        herdr_version: None,
        session: None,
        files: RemoteFileListSnapshot::idle(),
        catalog: Default::default(),
    });
    let connector: Arc<dyn hide_herdr_client::ApiConnector> = Arc::new(
        hide_herdr_client::UnixSocketConnector::new("/tmp/herdr-core-never-connect.sock"),
    );
    runtime.install_remote_control(RemoteControlContext::new(
        TARGET,
        connector,
        Weak::new(),
        ChangeNotifier::noop(),
    ));
    let before = runtime.snapshot.navigator.focused_device_id.clone();
    runtime.request_remote_control(RemoteControlPayload {
        target_id: TARGET.to_owned(),
        request_id: "open-workspace".to_owned(),
        report_pane_focus_outcome: false,
        focus_device: true,
        request: RemoteControlRequest::FocusWorkspace {
            workspace_id: format!("remote:{TARGET}:workspace:w1"),
            checkout_id: None,
        },
    });
    assert!(runtime.snapshot.status.last_error.is_some());
    assert_eq!(runtime.snapshot.navigator.focused_device_id, before);
}

/// S6 B14-B16, B21: a device pane carries its direct children and its path
/// back to the parent from the device's own lineage, as a pane here does.
#[test]
fn a_device_pane_carries_its_children_and_its_path_to_the_parent() {
    let t = tree();
    let mut runtime = runtime();
    runtime.snapshot.status.remote.push(RemoteStatusSnapshot {
        target_id: TARGET.to_owned(),
        state: "connected".to_owned(),
        message: None,
        herdr_version: Some("0.9.1".to_owned()),
        session: None,
        files: RemoteFileListSnapshot::idle(),
        catalog: Default::default(),
    });
    let mut raw = session(vec![herdr_workspace(
        TARGET,
        "w1",
        &t.main,
        &[("t1", &t.main), ("t2", &t.main)],
    )]);
    let parent = format!("remote:{TARGET}:pane:t1");
    let child = format!("remote:{TARGET}:pane:t2");
    let row = |pane: &str, spawned_from: Option<&str>| {
        let mut agents = crate::sidebar::project_agents(
            serde_json::from_value(serde_json::json!({"agents": [{
                "pane_id": pane, "agent": "claude", "agent_status": "working",
                "state_change_seq": 1, "tokens": {"task": format!("Task {pane}")}
            }]}))
            .unwrap(),
        )
        .agents;
        let mut agent = agents.remove(0);
        agent.spawned_from_pane_id = spawned_from.map(str::to_owned);
        agent
    };
    raw.agents = vec![row(&parent, None), row(&child, Some(&parent))];
    runtime.ingest_remote_session(TARGET, Ok(raw));

    let panes = runtime.snapshot.status.remote[0]
        .session
        .as_ref()
        .unwrap()
        .workspaces
        .iter()
        .flat_map(|workspace| &workspace.checkouts)
        .flat_map(|checkout| &checkout.tabs)
        .flat_map(|tab| &tab.panes)
        .cloned()
        .collect::<Vec<_>>();
    let pane = |id: &str| panes.iter().find(|pane| pane.id == id).unwrap();
    let chips = &pane(&parent).children.as_ref().expect("children").chips;
    assert_eq!(
        chips
            .iter()
            .map(|chip| chip.pane_id.as_str())
            .collect::<Vec<_>>(),
        [child.as_str()]
    );
    let path = &pane(&child).lineage_path;
    assert_eq!(
        path.iter()
            .map(|step| step.pane_id.as_str())
            .collect::<Vec<_>>(),
        [parent.as_str(), child.as_str()]
    );
}
