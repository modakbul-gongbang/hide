//! The request view's verb and pull request rules (PRD overview-request-view
//! B9-B16, B48, B54, B56), on rows projected the way Herdr's agents are.

use super::*;
use crate::labels::facts::{Reply, Request, Requester};
use crate::model::{GithubProjectSnapshot, PullRequestSnapshot};
use crate::sidebar::{SessionSnapshotPayload, project_agents};

const ROOT: &str = "/work/app";
const ASKED: u64 = 1_000_000;

/// One row per `(pane, status)`, unread, with the operator's request at `ASKED`.
fn rows(agents: &[(&str, &str)]) -> Vec<SidebarAgentSnapshot> {
    let payload: SessionSnapshotPayload = serde_json::from_value(serde_json::json!({
        "agents": agents.iter().map(|(pane, status)| serde_json::json!({
            "pane_id": pane, "agent": "claude", "agent_status": status, "state_change_seq": 1,
        })).collect::<Vec<_>>()
    }))
    .unwrap();
    let mut rows = project_agents(payload).agents;
    for row in &mut rows {
        row.row_facts = Some(RowFacts {
            operator_request: Some(request("요청", Requester::Operator, ASKED, true)),
            ..RowFacts::default()
        });
    }
    rows
}

fn request(text: &str, requester: Requester, at: u64, first: bool) -> Request {
    Request {
        text: text.to_owned(),
        cut: false,
        images: 0,
        at_unix_ms: at,
        requester,
        first,
    }
}

fn pull_request(
    number: u32,
    branch: &str,
    badge: PullRequestBadge,
    checks: PullRequestChecks,
) -> PullRequestSnapshot {
    PullRequestSnapshot {
        closing_issues: Vec::new(),
        title: format!("PR {number}"),
        checks,
        number,
        head_branch: branch.to_owned(),
        base_branch: "main".to_owned(),
        url: format!("https://github.com/acme/app/pull/{number}"),
        badge,
        review: None,
        is_draft: false,
        merged_at_unix_ms: None,
        updated_at_unix_ms: Some(u64::from(number)),
        created_at_unix_ms: Some(u64::from(number)),
        closed_at_unix_ms: None,
        head_oid: Some(head_of(branch)),
        cross_repository: false,
    }
}

/// The commit every test checkout on `branch` is on, and the head of the
/// pull requests these tests make for it: the work belongs to its branch.
fn head_of(branch: &str) -> String {
    format!("head-of-{branch}")
}

fn github(pull_requests: Vec<PullRequestSnapshot>) -> GithubSnapshot {
    GithubSnapshot {
        projects: vec![GithubProjectSnapshot {
            root_path: ROOT.to_owned(),
            pull_requests,
            ..GithubProjectSnapshot::default()
        }],
    }
}

/// Every row's branch, by pane.
fn run(rows: &mut [SidebarAgentSnapshot], branches: &[(&str, &str)], github: &GithubSnapshot) {
    let branches: HashMap<String, (String, String)> = branches
        .iter()
        .map(|(pane, branch)| ((*pane).to_owned(), ((*branch).to_owned(), head_of(branch))))
        .collect();
    apply(
        rows,
        |pane| {
            Some(RowPlace {
                branch: branches.get(pane).map(|(branch, _)| branch.as_str()),
                head_sha: branches.get(pane).map(|(_, head)| head.as_str()),
                root_path: ROOT,
            })
        },
        github,
        &mut BTreeMap::new(),
        2_000_000,
    );
}

fn verb(row: &SidebarAgentSnapshot) -> RequestVerb {
    row.request.as_ref().unwrap().verb
}

#[test]
fn a_rows_verb_follows_its_demand_its_activity_and_its_pull_requests() {
    let mut rows = rows(&[
        ("asks", "blocked"),
        ("works", "working"),
        ("failed", "idle"),
        ("ready", "idle"),
        ("ci", "idle"),
        ("finished", "done"),
        ("quiet", "idle"),
    ]);
    let github = github(vec![
        pull_request(
            1,
            "failed",
            PullRequestBadge::Open,
            PullRequestChecks::Failed,
        ),
        pull_request(
            2,
            "ready",
            PullRequestBadge::Review,
            PullRequestChecks::Passing,
        ),
        pull_request(3, "ci", PullRequestBadge::Open, PullRequestChecks::Pending),
        pull_request(
            4,
            "works",
            PullRequestBadge::Open,
            PullRequestChecks::Failed,
        ),
    ]);
    run(
        &mut rows,
        &[
            ("failed", "failed"),
            ("ready", "ready"),
            ("ci", "ci"),
            ("works", "works"),
        ],
        &github,
    );
    let verbs: Vec<_> = rows.iter().map(verb).collect();
    assert_eq!(
        verbs,
        [
            RequestVerb::Answer,
            RequestVerb::Working,
            RequestVerb::Fix,
            RequestVerb::Review,
            RequestVerb::Waiting,
            RequestVerb::Result,
            RequestVerb::Idle,
        ]
    );
}

#[test]
fn sessions_skip_a_read_ai_question_but_keep_menu_approval_and_the_verb_ladder() {
    use crate::agent_state::sessions::{Group, Tag};
    let mut rows = rows(&[
        ("approval", "blocked"),
        ("question", "idle"),
        ("read-question", "idle"),
        ("read-with-ci", "idle"),
        ("working-with-ci", "working"),
    ]);
    for row in &mut rows {
        row.row_facts.as_mut().unwrap().line = Some("Current task".into());
        if row.pane_id != "approval" && row.pane_id != "working-with-ci" {
            row.demand = "question".into();
        }
        row.unread = row.pane_id == "question";
    }
    run(
        &mut rows,
        &[
            ("question", "ci"),
            ("read-with-ci", "ci"),
            ("working-with-ci", "working"),
        ],
        &github(vec![
            pull_request(1, "ci", PullRequestBadge::Open, PullRequestChecks::Failed),
            pull_request(
                2,
                "working",
                PullRequestBadge::Open,
                PullRequestChecks::Failed,
            ),
        ]),
    );
    let states: Vec<_> = rows
        .iter()
        .map(|row| {
            (
                row.pane_id.as_str(),
                row.state.session.group,
                row.state.session.tag,
            )
        })
        .collect();
    assert_eq!(
        states,
        [
            ("approval", Group::MyTurn, Some(Tag::Approval)),
            ("question", Group::MyTurn, Some(Tag::Answer)),
            ("read-question", Group::Resting, Some(Tag::Idle)),
            // The question holder already owns PR 1, so another row cannot also fix it.
            ("read-with-ci", Group::Resting, Some(Tag::Idle)),
            ("working-with-ci", Group::InProgress, Some(Tag::Working)),
        ]
    );
    rows[1].unread = false;
    run(
        &mut rows,
        &[("question", "ci")],
        &github(vec![pull_request(
            1,
            "ci",
            PullRequestBadge::Open,
            PullRequestChecks::Failed,
        )]),
    );
    assert_eq!(
        rows[1].state.session,
        crate::agent_state::sessions::Row {
            group: Group::MyTurn,
            tag: Some(Tag::Fix),
        },
        "reading an AI question skips only the demand rung, not its PR duty"
    );
}

#[test]
fn sessions_only_offer_merge_after_passing_checks_and_an_acceptable_review() {
    use crate::agent_state::sessions::{Group, Tag};
    for (checks, review, expected) in [
        (PullRequestChecks::Passing, None, Tag::Merge),
        (
            PullRequestChecks::Passing,
            Some(ReviewDecision::Approved),
            Tag::Merge,
        ),
        (
            PullRequestChecks::Passing,
            Some(ReviewDecision::ReviewRequired),
            Tag::Review,
        ),
        (
            PullRequestChecks::Passing,
            Some(ReviewDecision::ChangesRequested),
            Tag::Review,
        ),
        (
            PullRequestChecks::Unknown,
            Some(ReviewDecision::Approved),
            Tag::Review,
        ),
        (
            PullRequestChecks::None,
            Some(ReviewDecision::Approved),
            Tag::Review,
        ),
    ] {
        let mut rows = rows(&[("agent", "idle")]);
        rows[0].row_facts.as_mut().unwrap().line = Some("Review changes".into());
        let mut pull = pull_request(1, "feature", PullRequestBadge::Open, checks);
        pull.review = review;
        run(&mut rows, &[("agent", "feature")], &github(vec![pull]));
        assert_eq!(rows[0].state.session.group, Group::ReviewMerge);
        assert_eq!(rows[0].state.session.tag, Some(expected));
    }
}

#[test]
fn sessions_counts_match_their_groups_and_keep_unraised_children_out() {
    use crate::agent_state::sessions::{Group, scope};
    let mut rows = rows(&[
        ("approval", "blocked"),
        ("working", "working"),
        ("idle", "idle"),
        ("child", "blocked"),
    ]);
    rows[3].delegated = true;
    run(&mut rows, &[], &github(vec![]));
    let scope = scope(rows.iter().enumerate());
    assert_eq!(scope.counts[&Group::MyTurn], 1);
    assert_eq!(scope.counts[&Group::InProgress], 1);
    assert_eq!(scope.counts[&Group::Resting], 1);
    assert_eq!(scope.counts[&Group::ReviewMerge], 0);
    assert_eq!(
        scope
            .groups
            .iter()
            .map(|section| section.members.clone())
            .collect::<Vec<_>>(),
        [vec![0], vec![1], vec![2]]
    );
    assert!(
        rows[0].state.session.tag.is_none(),
        "an unlabelled row does not invent a task tag"
    );
}

#[test]
fn automatic_resolution_waits_for_every_assigned_pr_and_newer_settlement_after_input() {
    use crate::agent_state::sessions::auto_resolvable;
    let mut agent = rows(&[("maker", "idle")]);
    agent[0].row_facts.as_mut().unwrap().created_prs =
        vec![("acme/app".into(), 1, 1), ("acme/app".into(), 2, 2)];
    let mut merged = pull_request(
        1,
        "one",
        PullRequestBadge::Merged,
        PullRequestChecks::Passing,
    );
    merged.merged_at_unix_ms = Some(ASKED + 10);
    let mut closed = pull_request(
        2,
        "two",
        PullRequestBadge::Closed,
        PullRequestChecks::Passing,
    );
    closed.closed_at_unix_ms = Some(ASKED + 20);
    run(
        &mut agent,
        &[("maker", "main")],
        &github(vec![
            merged.clone(),
            pull_request(2, "two", PullRequestBadge::Open, PullRequestChecks::Passing),
        ]),
    );
    assert!(
        !auto_resolvable(&agent[0], None),
        "one settled PR cannot hide the remaining duty"
    );
    run(
        &mut agent,
        &[("maker", "main")],
        &github(vec![merged, closed]),
    );
    assert!(auto_resolvable(&agent[0], None));
    assert!(
        !auto_resolvable(&agent[0], Some(ASKED + 10)),
        "input after a settlement restores the session until all duties settle again"
    );
    agent[0].blocked = true;
    assert!(!auto_resolvable(&agent[0], None));
    agent[0].blocked = false;
    agent[0].activity = "working".into();
    assert!(!auto_resolvable(&agent[0], None));
}

#[test]
fn closed_session_prs_need_a_recorded_link_and_never_duplicate_a_live_sessions_chip() {
    use crate::agent_state::sessions::{Group, Scope, Tag, add_closed_prs};
    let mut live = rows(&[("live", "idle")]);
    let pulls = vec![
        pull_request(
            1,
            "live",
            PullRequestBadge::Open,
            PullRequestChecks::Passing,
        ),
        pull_request(
            2,
            "closed-session",
            PullRequestBadge::Open,
            PullRequestChecks::Passing,
        ),
        pull_request(
            3,
            "unlinked",
            PullRequestBadge::Open,
            PullRequestChecks::Passing,
        ),
        pull_request(
            4,
            "settled",
            PullRequestBadge::Merged,
            PullRequestChecks::Passing,
        ),
    ];
    run(&mut live, &[("live", "live")], &github(pulls.clone()));
    let project = crate::model::WorkspaceSnapshot {
        agent_scope: Default::default(),
        home_issues: Default::default(),
        tasks: Default::default(),
        pull_requests: pulls,
        id: "app".into(),
        label: "App".into(),
        path: ROOT.into(),
        remote_target_id: None,
        expanded: true,
        device_id: "local".into(),
        repo_name: "App".into(),
        is_git: true,
        default_branch: Some("main".into()),
        branches: vec![],
        registered: true,
        temporary: false,
        session_workspace_ids: vec![],
        last_activity_unix_ms: None,
        pinned: false,
        is_home: false,
        checkouts: vec![],
        inactive_checkouts: Default::default(),
        session_folds: Default::default(),
        removal: Default::default(),
        disk: Default::default(),
        cleanup: None,
    };
    let mut scope = Scope::default();
    add_closed_prs(&mut scope, &project, Some(&[1, 2, 4].into()), &[&live[0]]);
    assert_eq!(
        scope
            .closed_prs
            .iter()
            .map(|row| (row.number, row.tag))
            .collect::<Vec<_>>(),
        [(2, Tag::Merge)]
    );
    assert_eq!(scope.counts[&Group::ReviewMerge], 1);
    // B17: a live PR with the same number in another repository does not
    // consume this project's closed-session PR.
    live[0].request.as_mut().unwrap().pull_requests[0].url =
        "https://github.com/acme/other/pull/1".into();
    let mut other = Scope::default();
    add_closed_prs(&mut other, &project, Some(&[1, 2].into()), &[&live[0]]);
    assert_eq!(other.closed_prs.len(), 2);
    assert_eq!(other.counts[&Group::ReviewMerge], 2);
}

#[test]
fn a_session_that_made_pull_requests_on_main_shows_the_one_to_look_at_first() {
    let mut rows = rows(&[("main-agent", "idle")]);
    rows[0].row_facts.as_mut().unwrap().created_prs = vec![
        ("acme/app".to_owned(), 10, 5),
        ("acme/app".to_owned(), 11, 6),
        ("acme/app".to_owned(), 12, 7),
    ];
    let mut merged_before = pull_request(
        10,
        "prd/old",
        PullRequestBadge::Merged,
        PullRequestChecks::Passing,
    );
    merged_before.merged_at_unix_ms = Some(ASKED - 1);
    let github = github(vec![
        merged_before,
        pull_request(
            11,
            "prd/a",
            PullRequestBadge::Open,
            PullRequestChecks::Failed,
        ),
        pull_request(
            12,
            "prd/b",
            PullRequestBadge::Review,
            PullRequestChecks::Passing,
        ),
        pull_request(
            13,
            "prd/c",
            PullRequestBadge::Open,
            PullRequestChecks::Pending,
        ),
    ]);
    run(&mut rows, &[("main-agent", "main")], &github);
    let request = rows[0].request.as_ref().unwrap();
    assert_eq!(
        request.verb,
        RequestVerb::Fix,
        "the older PR's failure is not hidden"
    );
    let shown: Vec<_> = request
        .pull_requests
        .iter()
        .map(|pr| (pr.number, pr.live, pr.created))
        .collect();
    assert_eq!(
        shown,
        [(11, true, true), (12, true, true), (10, false, true)],
        "a PR it did not make is not its"
    );
}

#[test]
fn a_pull_request_settled_after_the_request_is_a_result_and_one_settled_before_is_history() {
    let mut rows = rows(&[("after", "idle"), ("before", "idle")]);
    let mut after = pull_request(
        1,
        "after",
        PullRequestBadge::Merged,
        PullRequestChecks::Passing,
    );
    after.merged_at_unix_ms = Some(ASKED + 1);
    let mut before = pull_request(
        2,
        "before",
        PullRequestBadge::Closed,
        PullRequestChecks::Passing,
    );
    before.closed_at_unix_ms = Some(ASKED - 1);
    run(
        &mut rows,
        &[("after", "after"), ("before", "before")],
        &github(vec![after, before]),
    );
    assert_eq!(verb(&rows[0]), RequestVerb::Result);
    assert!(rows[0].request.as_ref().unwrap().pull_requests[0].live);
    assert_eq!(verb(&rows[1]), RequestVerb::Idle);
    assert!(!rows[1].request.as_ref().unwrap().pull_requests[0].live);
}

/// B15, D-29: opening a result a merged pull request gave sends the row to
/// rest, and a pull request that settles after the opening is a result again.
#[test]
fn an_opened_result_rests_until_another_pull_request_settles() {
    let lay = |rows: &mut Vec<SidebarAgentSnapshot>,
               github: &GithubSnapshot,
               verbs: &mut BTreeMap<String, VerbRecord>,
               head: &str,
               now| {
        apply(
            rows,
            |_| {
                Some(RowPlace {
                    branch: Some("after"),
                    head_sha: Some(head),
                    root_path: ROOT,
                })
            },
            github,
            verbs,
            now,
        );
    };
    let mut merged = pull_request(
        1,
        "after",
        PullRequestBadge::Merged,
        PullRequestChecks::Passing,
    );
    merged.merged_at_unix_ms = Some(ASKED + 1);
    let mut verbs = BTreeMap::new();
    let first_head = head_of("after");
    let mut shown = rows(&[("p", "idle")]);
    lay(
        &mut shown,
        &github(vec![merged.clone()]),
        &mut verbs,
        &first_head,
        ASKED + 10,
    );
    assert_eq!(verb(&shown[0]), RequestVerb::Result);

    assert!(open_result(&mut verbs, "p", ASKED + 20));
    let mut shown = rows(&[("p", "idle")]);
    lay(
        &mut shown,
        &github(vec![merged.clone()]),
        &mut verbs,
        &first_head,
        ASKED + 30,
    );
    assert_eq!(verb(&shown[0]), RequestVerb::Idle);
    assert!(
        !open_result(&mut verbs, "p", ASKED + 40),
        "only a result is opened"
    );

    let mut later = pull_request(
        2,
        "after",
        PullRequestBadge::Closed,
        PullRequestChecks::Passing,
    );
    later.closed_at_unix_ms = Some(ASKED + 50);
    // The branch was worked on again after the merge, so the checkout is on
    // the later pull request's commit, not the merged one's.
    later.head_oid = Some("second-round".to_owned());
    let mut shown = rows(&[("p", "idle")]);
    lay(
        &mut shown,
        &github(vec![merged, later]),
        &mut verbs,
        "second-round",
        ASKED + 60,
    );
    assert_eq!(verb(&shown[0]), RequestVerb::Result);
}

#[test]
fn a_shared_pull_request_gives_its_duty_to_the_row_on_its_branch() {
    let mut rows = rows(&[("maker", "idle"), ("branch", "idle")]);
    rows[0].row_facts.as_mut().unwrap().created_prs = vec![("acme/app".to_owned(), 5, 1)];
    run(
        &mut rows,
        &[("maker", "main"), ("branch", "prd/x")],
        &github(vec![pull_request(
            5,
            "prd/x",
            PullRequestBadge::Open,
            PullRequestChecks::Failed,
        )]),
    );
    assert_eq!(
        verb(&rows[0]),
        RequestVerb::Idle,
        "the chip stays, the duty goes"
    );
    assert!(!rows[0].request.as_ref().unwrap().pull_requests[0].duty);
    assert_eq!(verb(&rows[1]), RequestVerb::Fix);
}

#[test]
fn who_sent_the_shown_request_and_who_came_after() {
    let mut rows = rows(&[("parent", "idle"), ("child", "idle")]);
    rows[1].delegated = true;
    rows[1].lineage_parent_pane_id = Some("parent".to_owned());
    rows[0].identity_label = "요청 보기".to_owned();
    rows[0].row_facts.as_mut().unwrap().other_request = Some(request(
        "CI 다시 봐줘",
        Requester::Named("ci-lead".to_owned()),
        ASKED + 5,
        false,
    ));
    rows[1].row_facts = Some(RowFacts {
        operator_request: Some(request("맡긴 일", Requester::Unobserved, ASKED, true)),
        reply: Some(Reply {
            text: "끝".to_owned(),
            cut: false,
            at_unix_ms: ASKED + 9,
        }),
        ..RowFacts::default()
    });
    run(&mut rows, &[], &GithubSnapshot::default());
    let parent = rows[0].request.as_ref().unwrap();
    assert_eq!(
        parent.request.as_ref().unwrap().sender,
        RequestSender::Operator
    );
    assert_eq!(
        parent.later_by,
        Some(RequestSender::Named("ci-lead".to_owned()))
    );
    let child = rows[1].request.as_ref().unwrap();
    assert_eq!(
        child.request.as_ref().unwrap().sender,
        RequestSender::Named("요청 보기".to_owned()),
        "a delegated child's first request is its parent's"
    );
    assert_eq!(child.reply.as_ref().unwrap().text, "끝");
}

#[test]
fn a_sender_cannot_pass_as_the_operator_or_carry_hidden_text() {
    let named_as = |name: &str| {
        let mut rows = rows(&[("p", "idle")]);
        rows[0].row_facts = Some(RowFacts {
            other_request: Some(request(
                "봐줘",
                Requester::Named(name.to_owned()),
                ASKED,
                false,
            )),
            ..RowFacts::default()
        });
        run(&mut rows, &[], &GithubSnapshot::default());
        rows[0].request.clone().unwrap()
    };
    for reserved in [
        "나",
        "에이전트",
        "Operator",
        " 나 ",
        "나\u{200B}",
        "ｏｐｅｒａｔｏｒ",
        "\u{1102}\u{1161}",
    ] {
        assert_eq!(
            named_as(reserved).request.unwrap().sender,
            RequestSender::Agent,
            "{reserved}"
        );
    }
    let shown = named_as(&format!("ci\u{202E}-lead\n{}", "x".repeat(100)));
    let RequestSender::Named(name) = shown.request.unwrap().sender else {
        panic!("a plain name is shown");
    };
    assert!(name.starts_with("ci-lead x"), "{name}");
    assert_eq!(name.chars().count(), 64);
}

#[test]
fn a_verbs_time_holds_while_the_verb_holds_and_starts_over_when_it_moves() {
    let mut verbs = BTreeMap::new();
    let mut idle = rows(&[("p", "idle")]);
    assert!(apply(
        &mut idle,
        |_| None,
        &GithubSnapshot::default(),
        &mut verbs,
        10
    ));
    assert!(!apply(
        &mut idle,
        |_| None,
        &GithubSnapshot::default(),
        &mut verbs,
        20
    ));
    assert_eq!(idle[0].request.as_ref().unwrap().verb_since_unix_ms, 10);
    let mut working = rows(&[("p", "working")]);
    assert!(apply(
        &mut working,
        |_| None,
        &GithubSnapshot::default(),
        &mut verbs,
        30
    ));
    assert_eq!(working[0].request.as_ref().unwrap().verb_since_unix_ms, 30);
    assert!(prune_verbs(&mut verbs, |_| true));
}

#[test]
fn a_merged_pull_request_of_a_reused_branch_name_is_not_a_rows_result() {
    let merged = pull_request(
        1,
        "feature",
        PullRequestBadge::Merged,
        PullRequestChecks::Passing,
    );
    let on = |head: &str| {
        let mut shown = rows(&[("p", "idle")]);
        apply(
            &mut shown,
            |_| {
                Some(RowPlace {
                    branch: Some("feature"),
                    head_sha: Some(head),
                    root_path: ROOT,
                })
            },
            &github(vec![merged.clone()]),
            &mut BTreeMap::new(),
            2_000_000,
        );
        shown
            .remove(0)
            .request
            .map_or(0, |block| block.pull_requests.len())
    };
    let current = head_of("feature");
    assert_eq!(on("new-work"), 0);
    assert_eq!(on(&current), 1);
}
