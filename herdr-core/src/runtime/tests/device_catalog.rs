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
