//! The core reads GitHub for every local Git project from the start, asks
//! again on a clock or when a session prints a pull request it does not hold,
//! and draws the last run's answer until a read replaces it.

use super::*;
use crate::github_store::GithubStore;
use crate::labels::worker::SightedPullRequest;
use crate::model::{
    GithubProjectSnapshot, GithubSnapshot, GithubStatusSnapshot, PullRequestBadge,
    PullRequestChecks, PullRequestSnapshot,
};
use std::time::{Duration, Instant};

fn git_project(id: &str, path: &str, branch: &str) -> WorkspaceSnapshot {
    let mut checkout = checkout(id, &format!("{id}-checkout"), path, None);
    checkout.branch = Some(branch.to_owned());
    let mut project = workspace(id, id, path, vec![checkout]);
    project.is_git = true;
    project
}

fn pull_request(number: u32, branch: &str, badge: PullRequestBadge) -> PullRequestSnapshot {
    PullRequestSnapshot {
        closing_issues: Vec::new(),
        title: format!("Pull request {number}"),
        checks: PullRequestChecks::Passing,
        number,
        head_branch: branch.to_owned(),
        base_branch: "main".to_owned(),
        url: format!("https://example.invalid/pull/{number}"),
        badge,
        review: None,
        is_draft: false,
        merged_at_unix_ms: None,
        updated_at_unix_ms: Some(5),
        created_at_unix_ms: Some(2),
        closed_at_unix_ms: Some(3),
        head_oid: Some(format!("{number:040x}")),
        cross_repository: false,
    }
}

fn read_ok(root: &str, pull_requests: Vec<PullRequestSnapshot>, at: u64) -> GithubProjectSnapshot {
    GithubProjectSnapshot {
        root_path: root.to_owned(),
        status: GithubStatusSnapshot {
            available: true,
            last_success_at_unix_ms: Some(at),
            ..Default::default()
        },
        pull_requests,
        pull_requests_read: true,
        issues_read: true,
        ..Default::default()
    }
}

fn read_failed(root: &str) -> GithubProjectSnapshot {
    GithubProjectSnapshot {
        root_path: root.to_owned(),
        status: GithubStatusSnapshot {
            available: true,
            stale: true,
            unavailable_reason: Some("gh pr list: network unreachable".to_owned()),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn generations(runtime: &Runtime) -> Vec<(String, u64)> {
    runtime
        .github_request()
        .projects
        .into_iter()
        .map(|project| {
            (
                project.root.to_string_lossy().into_owned(),
                project.generation,
            )
        })
        .collect()
}

fn row_loading(runtime: &Runtime, workspace: usize) -> bool {
    runtime.snapshot.navigator.workspaces[workspace].checkouts[0]
        .github
        .loading
}

#[test]
fn every_local_git_project_is_read_with_no_screen_open() {
    let mut runtime = runtime();
    let mut remote = git_project("remote", "/tmp/remote", "main");
    remote.remote_target_id = Some("device".to_owned());
    let folder = workspace("folder", "folder", "/tmp/folder", Vec::new());
    runtime.snapshot.navigator.workspaces = vec![
        git_project("b", "/tmp/b", "main"),
        remote,
        folder,
        git_project("a", "/tmp/a", "main"),
    ];
    runtime.snapshot.ui_state.right_panel_visible = false;
    assert_eq!(
        generations(&runtime),
        vec![("/tmp/a".to_owned(), 0), ("/tmp/b".to_owned(), 0)],
        "local Git projects only, in path order"
    );
}

#[test]
fn a_project_loads_until_its_first_read_answers_even_with_nothing_to_show() {
    let mut runtime = runtime();
    runtime.snapshot.navigator.workspaces = vec![
        git_project("a", "/tmp/a", "main"),
        git_project("b", "/tmp/b", "main"),
    ];
    runtime.apply_pull_requests();
    assert!(row_loading(&runtime, 0) && row_loading(&runtime, 1));

    // The pass answered for `a` only: `b` is not inside a Git repository, so
    // the reader returns no entry for it.
    let request = runtime.github_request();
    assert!(runtime.ingest_github_answer(
        GithubSnapshot {
            projects: vec![read_ok("/tmp/a", Vec::new(), 10)],
        },
        true
    ));
    assert_eq!(request, runtime.github_request());
    assert!(!row_loading(&runtime, 0));
    assert!(
        !row_loading(&runtime, 1),
        "a project the read answered nothing for does not load forever"
    );
}

/// One answer for the request as it stands, as the reader hands it back.
fn answer_all(runtime: &mut Runtime) {
    let projects = runtime
        .github_request()
        .projects
        .iter()
        .map(|project| read_ok(&project.root.to_string_lossy(), Vec::new(), 10))
        .collect();
    runtime.ingest_github_answer(GithubSnapshot { projects }, true);
}

#[test]
fn a_project_is_asked_for_again_five_minutes_after_its_last_answer() {
    let mut runtime = runtime();
    runtime.snapshot.navigator.workspaces = vec![
        git_project("a", "/tmp/a", "main"),
        git_project("b", "/tmp/b", "main"),
    ];
    let start = Instant::now();
    runtime.reread_stale_github(start);
    assert_eq!(
        generations(&runtime),
        vec![("/tmp/a".to_owned(), 0), ("/tmp/b".to_owned(), 0)],
        "the first sight of a project is read already, not asked again"
    );
    runtime.reread_stale_github(start + Duration::from_secs(1000));
    assert_eq!(
        generations(&runtime)[0].1,
        0,
        "no answer yet: the pass is slow, not the answer old"
    );

    // The answer lands; the next wake starts its five minutes.
    answer_all(&mut runtime);
    let answered = start + Duration::from_secs(2000);
    runtime.reread_stale_github(answered);
    runtime.reread_stale_github(answered + Duration::from_secs(299));
    assert_eq!(generations(&runtime)[0].1, 0);
    runtime.reread_stale_github(answered + Duration::from_secs(300));
    assert_eq!(
        generations(&runtime),
        vec![("/tmp/a".to_owned(), 1), ("/tmp/b".to_owned(), 1)]
    );

    // Asked again and not yet answered: no further ask, however long it takes.
    runtime.reread_stale_github(answered + Duration::from_secs(10_000));
    assert_eq!(generations(&runtime)[0].1, 1);
    answer_all(&mut runtime);
    runtime.reread_stale_github(answered + Duration::from_secs(10_001));
    runtime.reread_stale_github(answered + Duration::from_secs(10_301));
    assert_eq!(generations(&runtime)[0].1, 2, "one ask per answer");
}

/// An answer for the request as it stands: `failed` projects came back
/// without their pull requests, the others read fine.
fn answer_with(runtime: &mut Runtime, failed: &[&str]) {
    let projects = runtime
        .github_request()
        .projects
        .iter()
        .map(|project| {
            let root = project.root.to_string_lossy().into_owned();
            if failed.contains(&root.as_str()) {
                read_failed(&root)
            } else {
                read_ok(&root, Vec::new(), 10)
            }
        })
        .collect();
    runtime.ingest_github_answer(GithubSnapshot { projects }, true);
}

fn generation_of(runtime: &Runtime, path: &str) -> u64 {
    generations(runtime)
        .into_iter()
        .find(|(project, _)| project == path)
        .unwrap()
        .1
}

/// Lets the clock see the answer at `at`, then asks at each second after it
/// up to 400 and returns the first one at which `path` was asked again.
fn seconds_until_asked_again(runtime: &mut Runtime, path: &str, at: Instant) -> Option<u64> {
    let before = generation_of(runtime, path);
    runtime.reread_stale_github(at);
    (1..=400).find(|wait| {
        runtime.reread_stale_github(at + Duration::from_secs(*wait));
        generation_of(runtime, path) != before
    })
}

#[test]
fn a_project_whose_read_failed_is_asked_again_in_thirty_seconds_while_the_others_keep_their_clock()
{
    let mut runtime = runtime();
    runtime.snapshot.navigator.workspaces = vec![
        git_project("a", "/tmp/a", "main"),
        git_project("b", "/tmp/b", "main"),
    ];
    let start = Instant::now();
    runtime.reread_stale_github(start);
    answer_with(&mut runtime, &["/tmp/a"]);
    let answered = start + Duration::from_secs(1);
    runtime.reread_stale_github(answered);
    runtime.reread_stale_github(answered + Duration::from_secs(29));
    assert_eq!(
        generations(&runtime),
        vec![("/tmp/a".to_owned(), 0), ("/tmp/b".to_owned(), 0)]
    );
    runtime.reread_stale_github(answered + Duration::from_secs(30));
    assert_eq!(
        generations(&runtime),
        vec![("/tmp/a".to_owned(), 1), ("/tmp/b".to_owned(), 0)],
        "only the failed project is asked again"
    );
    runtime.reread_stale_github(answered + Duration::from_secs(299));
    assert_eq!(
        generation_of(&runtime, "/tmp/b"),
        0,
        "the one that worked waits its five minutes"
    );
    runtime.reread_stale_github(answered + Duration::from_secs(300));
    assert_eq!(generation_of(&runtime, "/tmp/b"), 1);
}

#[test]
fn failures_in_a_row_wait_longer_up_to_five_minutes_and_a_success_starts_the_count_over() {
    let mut runtime = runtime();
    runtime.snapshot.navigator.workspaces = vec![git_project("a", "/tmp/a", "main")];
    let mut now = Instant::now();
    runtime.reread_stale_github(now);
    let mut waits = Vec::new();
    for _ in 0..7 {
        answer_with(&mut runtime, &["/tmp/a"]);
        now += Duration::from_secs(1);
        let wait = seconds_until_asked_again(&mut runtime, "/tmp/a", now).unwrap();
        waits.push(wait);
        now += Duration::from_secs(wait);
    }
    assert_eq!(
        waits,
        vec![30, 60, 120, 240, 300, 300, 300],
        "doubling from thirty seconds, stopping at the full re-read"
    );

    answer_with(&mut runtime, &[]);
    now += Duration::from_secs(1);
    let wait = seconds_until_asked_again(&mut runtime, "/tmp/a", now).unwrap();
    assert_eq!(wait, 300, "a read that worked waits the full five minutes");
    now += Duration::from_secs(wait);

    answer_with(&mut runtime, &["/tmp/a"]);
    now += Duration::from_secs(1);
    let wait = seconds_until_asked_again(&mut runtime, "/tmp/a", now).unwrap();
    assert_eq!(wait, 30, "the count started over after the success");
}

#[test]
fn a_neighbours_retry_does_not_restart_a_healthy_projects_wait() {
    let mut runtime = runtime();
    runtime.snapshot.navigator.workspaces = vec![
        git_project("a", "/tmp/a", "main"),
        git_project("b", "/tmp/b", "main"),
    ];
    let start = Instant::now();
    runtime.reread_stale_github(start);
    answer_with(&mut runtime, &["/tmp/a"]);
    runtime.reread_stale_github(start + Duration::from_secs(1));

    // The retry of `a` is answered; the reader hands back its cached entry for `b`.
    runtime.reread_stale_github(start + Duration::from_secs(31));
    assert_eq!(generation_of(&runtime, "/tmp/a"), 1);
    answer_with(&mut runtime, &["/tmp/a"]);
    runtime.reread_stale_github(start + Duration::from_secs(32));

    runtime.reread_stale_github(start + Duration::from_secs(300));
    assert_eq!(generation_of(&runtime, "/tmp/b"), 0);
    runtime.reread_stale_github(start + Duration::from_secs(301));
    assert_eq!(
        generation_of(&runtime, "/tmp/b"),
        1,
        "five minutes from its own answer, not from the neighbour's retry"
    );
}

#[test]
fn a_refresh_of_one_project_does_not_climb_a_failed_neighbours_backoff() {
    let mut runtime = runtime();
    runtime.snapshot.navigator.workspaces = vec![
        git_project("a", "/tmp/a", "main"),
        git_project("b", "/tmp/b", "main"),
    ];
    let start = Instant::now();
    runtime.reread_stale_github(start);
    answer_with(&mut runtime, &["/tmp/a"]);
    runtime.reread_stale_github(start);

    // The operator refreshes `b`; the answer carries `a`'s cached failure again.
    runtime.refresh_pull_requests("/tmp/b");
    answer_with(&mut runtime, &["/tmp/a"]);
    runtime.reread_stale_github(start + Duration::from_secs(10));

    runtime.reread_stale_github(start + Duration::from_secs(30));
    assert_eq!(
        generation_of(&runtime, "/tmp/a"),
        1,
        "still the first failure's thirty seconds, not a second failure"
    );
}

#[test]
fn a_failed_first_read_with_a_restored_answer_is_asked_again_soon() {
    let mut runtime = runtime();
    runtime.snapshot.navigator.workspaces = vec![git_project("a", "/tmp/a", "main")];
    runtime.github = GithubSnapshot {
        projects: vec![read_ok(
            "/tmp/a",
            vec![pull_request(7, "main", PullRequestBadge::Open)],
            5,
        )],
    };
    let start = Instant::now();
    runtime.reread_stale_github(start);
    answer_with(&mut runtime, &["/tmp/a"]);
    let wait = seconds_until_asked_again(&mut runtime, "/tmp/a", start + Duration::from_secs(1));
    assert_eq!(wait, Some(30));
    assert_eq!(
        runtime
            .github
            .project("/tmp/a")
            .unwrap()
            .pull_requests
            .len(),
        1,
        "the previous answer stays shown while the retry waits"
    );
}

#[test]
fn a_project_that_leaves_the_request_forgets_its_first_read() {
    let mut runtime = runtime();
    runtime.snapshot.navigator.workspaces = vec![git_project("a", "/tmp/a", "main")];
    answer_all(&mut runtime);
    runtime.reread_stale_github(Instant::now());
    runtime.snapshot.navigator.workspaces.clear();
    runtime.reread_stale_github(Instant::now());
    runtime.snapshot.navigator.workspaces = vec![git_project("a", "/tmp/a", "main")];
    // Another project's answer, ingested meanwhile, kept nothing of this one.
    runtime.github = GithubSnapshot::default();
    runtime.apply_pull_requests();
    assert!(
        row_loading(&runtime, 0),
        "a project that comes back is read again, not shown as having nothing"
    );
}

#[test]
fn an_answer_to_an_empty_request_does_not_erase_what_was_restored() {
    let mut runtime = runtime();
    runtime.github = GithubSnapshot {
        projects: vec![read_ok("/tmp/a", Vec::new(), 1_000)],
    };
    assert!(runtime.github_request().projects.is_empty());
    assert!(!runtime.ingest_github_answer(GithubSnapshot::default(), true));
    assert_eq!(runtime.github.projects.len(), 1);
}

#[test]
fn projects_past_the_limit_are_not_read_and_do_not_load() {
    let mut runtime = runtime();
    runtime.snapshot.navigator.workspaces = (0..66)
        .map(|index| {
            git_project(
                &format!("w{index:02}"),
                &format!("/tmp/p{index:02}"),
                "main",
            )
        })
        .collect();
    runtime.apply_pull_requests();
    runtime.reread_stale_github(Instant::now());
    let request = runtime.github_request();
    assert_eq!(request.projects.len(), 64);
    assert_eq!(request.projects[63].root, PathBuf::from("/tmp/p63"));
    assert_eq!(runtime.github_over_limit, 2);
    assert!(row_loading(&runtime, 0));
    assert!(
        !row_loading(&runtime, 65),
        "a project the core does not read has no read in flight"
    );
}

#[test]
fn a_restart_draws_the_last_answer_as_stale_and_a_failed_read_keeps_it() {
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("github-snapshot.json");
    let merged = pull_request(7, "feature", PullRequestBadge::Open);

    // The first run reads, saves and quits.
    let mut first = runtime();
    first.snapshot.navigator.workspaces = vec![git_project("a", "/tmp/a", "feature")];
    first.github_store = Some(GithubStore::new(file.clone()));
    assert!(first.ingest_github_answer(
        GithubSnapshot {
            projects: vec![read_ok("/tmp/a", vec![merged.clone()], 1_000)],
        },
        true
    ));
    drop(first.github_store.take());

    // The next run has no read yet; its first frame already has the answer.
    let mut second = runtime();
    second.snapshot.navigator.workspaces = vec![git_project("a", "/tmp/a", "feature")];
    let store = GithubStore::new(file.clone());
    let restored = store.restore();
    second.install_github_store(store, restored);
    let row = &second.snapshot.navigator.workspaces[0].checkouts[0];
    assert_eq!(
        row.pull_request
            .as_ref()
            .map(|pull_request| pull_request.number),
        Some(7)
    );
    assert!(row.github.stale);
    assert!(!row.github.loading);
    assert_eq!(row.github.last_success_at_unix_ms, Some(1_000));

    // The first read fails: the answer from before the restart stays.
    assert!(second.ingest_github_answer(
        GithubSnapshot {
            projects: vec![read_failed("/tmp/a")],
        },
        true
    ));
    let row = &second.snapshot.navigator.workspaces[0].checkouts[0];
    assert_eq!(
        row.pull_request
            .as_ref()
            .map(|pull_request| pull_request.number),
        Some(7)
    );
    assert!(row.github.stale);
    assert_eq!(row.github.last_success_at_unix_ms, Some(1_000));
    assert!(row.github.unavailable_reason.is_some());

    // A successful read replaces it and is fresh.
    assert!(second.ingest_github_answer(
        GithubSnapshot {
            projects: vec![read_ok("/tmp/a", vec![merged], 2_000)],
        },
        true
    ));
    let row = &second.snapshot.navigator.workspaces[0].checkouts[0];
    assert!(!row.github.stale);
    assert_eq!(row.github.last_success_at_unix_ms, Some(2_000));
}

fn many_projects(runtime: &mut Runtime, count: usize) {
    runtime.snapshot.navigator.workspaces = (0..count)
        .map(|index| {
            git_project(
                &format!("w{index:02}"),
                &format!("/tmp/p{index:02}"),
                "main",
            )
        })
        .collect();
    runtime.apply_pull_requests();
}

fn requested_paths(runtime: &Runtime) -> Vec<String> {
    generations(runtime)
        .into_iter()
        .map(|(path, _)| path)
        .collect()
}

#[test]
fn past_the_limit_the_project_in_front_is_still_read() {
    let mut runtime = runtime();
    many_projects(&mut runtime, 66);
    runtime.snapshot.navigator.focused_checkout_id = Some("w65-checkout".to_owned());
    let paths = requested_paths(&runtime);
    assert_eq!(paths.len(), 64);
    assert!(paths.contains(&"/tmp/p65".to_owned()));
    assert!(
        !paths.contains(&"/tmp/p63".to_owned()),
        "the last of the others makes room"
    );
    assert!(paths.windows(2).all(|pair| pair[0] < pair[1]), "path order");
}

#[test]
fn past_the_limit_a_project_a_screen_named_is_read_before_the_rest() {
    let mut runtime = runtime();
    many_projects(&mut runtime, 66);
    runtime.snapshot.navigator.focused_checkout_id = Some("w65-checkout".to_owned());
    let named = |kind: &str| {
        serde_json::to_vec(&serde_json::json!({
            "schema_version": SCHEMA_VERSION, "kind": kind,
            "payload": {"workspace_id": "w64", "refresh": false}
        }))
        .unwrap()
    };
    assert!(runtime.dispatch_json(&named("github_request")));
    let paths = requested_paths(&runtime);
    assert_eq!(paths.len(), 64);
    assert!(paths.contains(&"/tmp/p64".to_owned()) && paths.contains(&"/tmp/p65".to_owned()));
    assert!(!paths.contains(&"/tmp/p62".to_owned()));
    assert!(
        !runtime.dispatch_json(&named("github_request")),
        "naming it again changes nothing"
    );
}

#[test]
fn a_named_project_that_is_no_longer_registered_loses_its_place() {
    let mut runtime = runtime();
    many_projects(&mut runtime, 2);
    runtime.github_wanted.insert("/tmp/p01".to_owned());
    runtime.snapshot.navigator.workspaces.truncate(1);
    runtime.reread_stale_github(Instant::now());
    assert!(runtime.github_wanted.is_empty());
}

/// A repository with one commit on `branch`, and that commit.
fn repository_on(branch: &str) -> (tempfile::TempDir, String) {
    let folder = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.invalid",
            ])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .current_dir(folder.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}");
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    git(&["init", "-q", "-b", branch]);
    git(&["commit", "-q", "--allow-empty", "-m", "work"]);
    let head = git(&["rev-parse", "HEAD"]);
    (folder, head)
}

/// A runtime whose only project is the repository at `path`, with no worktree
/// catalog answered yet, as in the first second after a restart.
fn runtime_before_the_catalog(path: &std::path::Path) -> Runtime {
    let mut runtime = runtime();
    runtime.snapshot.ui_state.workspace_registrations = vec![WorkspaceRegistration {
        primary_checkout_id: None,
        id: "workspace:restart".to_owned(),
        label: "restart".to_owned(),
        path: path.to_string_lossy().into_owned(),
        device_id: "local".to_owned(),
        pinned: false,
        home: false,
    }];
    runtime.rebuild_catalog();
    assert!(runtime.worktree_catalog.projects.is_empty());
    runtime
}

/// The restored answer of a merged pull request on `branch` whose head is `head`.
fn saved_merged(file: &std::path::Path, root: &std::path::Path, branch: &str, head: &str) {
    let mut merged = pull_request(9, branch, PullRequestBadge::Merged);
    merged.head_oid = Some(head.to_owned());
    let store = GithubStore::new(file.to_path_buf());
    store.save(GithubSnapshot {
        projects: vec![read_ok(&root.to_string_lossy(), vec![merged], 1_000)],
    });
}

fn shown(runtime: &Runtime) -> Option<u32> {
    runtime.snapshot.navigator.workspaces[0].checkouts[0]
        .pull_request
        .as_ref()
        .map(|pull_request| pull_request.number)
}

#[test]
fn a_restored_merged_pull_request_attaches_at_once_to_the_checkout_on_its_head() {
    let (repository, head) = repository_on("feature");
    let root = std::fs::canonicalize(repository.path()).unwrap();
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("github-snapshot.json");
    saved_merged(&file, &root, "feature", &head);

    let mut runtime = runtime_before_the_catalog(&root);
    let store = GithubStore::new(file);
    let restored = store.restore();
    runtime.install_github_store(store, restored);
    assert_eq!(
        shown(&runtime),
        Some(9),
        "the commit read from Git's files connects it before the catalog answers"
    );
}

#[test]
fn a_restored_merged_pull_request_stays_off_a_checkout_on_another_commit() {
    let (repository, _head) = repository_on("feature");
    let root = std::fs::canonicalize(repository.path()).unwrap();
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("github-snapshot.json");
    saved_merged(&file, &root, "feature", &"f".repeat(40));

    let mut runtime = runtime_before_the_catalog(&root);
    let store = GithubStore::new(file);
    let restored = store.restore();
    runtime.install_github_store(store, restored);
    assert_eq!(
        shown(&runtime),
        None,
        "a reused branch name shows no old merge"
    );
}

#[test]
fn a_checkouts_commit_is_the_readers_when_it_has_one_and_the_files_before_that() {
    let mut checkout = crate::model::CheckoutSnapshot {
        head_oid: Some("b".repeat(40)),
        ..Default::default()
    };
    assert_eq!(
        checkout.head_sha(),
        Some("b".repeat(40).as_str()),
        "before the reader has answered, the commit read from Git's files"
    );
    checkout.worktree = Some(crate::model::WorktreeSnapshot {
        head_sha: Some("a".repeat(40)),
        ..Default::default()
    });
    assert_eq!(
        checkout.head_sha(),
        Some("a".repeat(40).as_str()),
        "the reader reruns on a moved HEAD, so its commit beats the file value"
    );
    checkout.worktree = Some(crate::model::WorktreeSnapshot::default());
    assert_eq!(
        checkout.head_sha(),
        Some("b".repeat(40).as_str()),
        "a reader row with no commit leaves the file value standing"
    );
}

/// A pull request in `repository` at its GitHub address.
fn pull_request_in(repository: &str, number: u32) -> PullRequestSnapshot {
    PullRequestSnapshot {
        url: format!("https://github.com/{repository}/pull/{number}"),
        ..pull_request(number, "main", PullRequestBadge::Open)
    }
}

fn sighted(pane_id: &str, repository: &str, number: u64, at_unix_ms: u64) -> SightedPullRequest {
    SightedPullRequest {
        pane_id: pane_id.to_owned(),
        repository: repository.to_owned(),
        number,
        at_unix_ms,
    }
}

const NOW_MS: u64 = 10_000_000;

#[test]
fn a_pull_request_a_session_just_printed_is_read_for_once_in_its_repository() {
    let mut runtime = runtime();
    runtime.snapshot.navigator.workspaces = vec![
        git_project("a", "/tmp/a", "main"),
        git_project("b", "/tmp/b", "main"),
    ];
    runtime.ingest_github_answer(
        GithubSnapshot {
            projects: vec![
                read_ok("/tmp/a", vec![pull_request_in("owner/a", 1)], 10),
                read_ok("/tmp/b", vec![pull_request_in("owner/b", 7)], 10),
            ],
        },
        true,
    );
    let before = generations(&runtime);

    runtime.read_sighted_pull_requests(&[sighted("w9:p1", "owner/a", 2, NOW_MS - 1_000)], NOW_MS);
    assert_eq!(
        generations(&runtime),
        vec![("/tmp/a".to_owned(), before[0].1 + 1), before[1].clone()],
        "only the project whose pull requests are in that repository is read again"
    );

    let after = generations(&runtime);
    runtime.read_sighted_pull_requests(
        &[
            sighted("w9:p1", "owner/a", 2, NOW_MS - 500),
            sighted("w9:p1", "owner/a", 1, NOW_MS - 500),
            sighted("w9:p1", "owner/b", 8, NOW_MS - 16 * 60 * 1_000),
        ],
        NOW_MS,
    );
    assert_eq!(
        generations(&runtime),
        after,
        "an address already asked for, one the answer holds, and an old sighting read nothing"
    );
}

#[test]
fn a_repositorys_first_pull_request_is_read_in_the_project_its_session_works_in() {
    let mut runtime = runtime();
    let mut first = git_project("c", "/tmp/c", "main");
    first.checkouts[0] = checkout("c", "c-checkout", "/tmp/c", Some(pane("w1:p1", "/tmp/c")));
    let mut other = git_project("d", "/tmp/d", "main");
    other.checkouts[0] = checkout("d", "d-checkout", "/tmp/d", Some(pane("w2:p1", "/tmp/d")));
    runtime.snapshot.navigator.workspaces = vec![first, other];
    runtime.ingest_github_answer(
        GithubSnapshot {
            projects: vec![
                read_ok("/tmp/c", Vec::new(), 10),
                read_ok("/tmp/d", vec![pull_request_in("owner/d", 3)], 10),
            ],
        },
        true,
    );
    let before = generations(&runtime);

    runtime.read_sighted_pull_requests(&[sighted("w1:p1", "owner/c", 1, NOW_MS)], NOW_MS);
    runtime.read_sighted_pull_requests(&[sighted("w2:p1", "owner/elsewhere", 4, NOW_MS)], NOW_MS);
    assert_eq!(
        generations(&runtime),
        vec![("/tmp/c".to_owned(), before[0].1 + 1), before[1].clone()],
        "a project with no pull request yet is its session's; one whose pull requests name another repository is not"
    );
}
