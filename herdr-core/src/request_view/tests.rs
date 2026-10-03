//! The request view's verb and pull request rules (PRD overview-request-view
//! B9-B16, B48, B54, B56), on rows projected the way Herdr's agents are.

use super::*;
use crate::labels::facts::{Reply, Request, Requester};
use crate::model::GithubProjectSnapshot;
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
    }
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
    let branches: HashMap<String, String> = branches
        .iter()
        .map(|(pane, branch)| ((*pane).to_owned(), (*branch).to_owned()))
        .collect();
    apply(
        rows,
        |pane| {
            Some(RowPlace {
                branch: branches.get(pane).map(String::as_str),
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
               now| {
        apply(
            rows,
            |_| {
                Some(RowPlace {
                    branch: Some("after"),
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
    let mut shown = rows(&[("p", "idle")]);
    lay(
        &mut shown,
        &github(vec![merged.clone()]),
        &mut verbs,
        ASKED + 10,
    );
    assert_eq!(verb(&shown[0]), RequestVerb::Result);

    assert!(open_result(&mut verbs, "p", ASKED + 20));
    let mut shown = rows(&[("p", "idle")]);
    lay(
        &mut shown,
        &github(vec![merged.clone()]),
        &mut verbs,
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
    let mut shown = rows(&[("p", "idle")]);
    lay(
        &mut shown,
        &github(vec![merged, later]),
        &mut verbs,
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
            native_title: Some(format!("세션\u{202E}{}", "제".repeat(300))),
            ..RowFacts::default()
        });
        run(&mut rows, &[], &GithubSnapshot::default());
        rows[0].request.clone().unwrap()
    };
    for reserved in ["나", "에이전트", "Operator", " 나 "] {
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
    let title = shown.native_title.unwrap();
    assert!(!title.contains('\u{202E}'));
    assert_eq!(title.chars().count(), 200);
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
