//! Native fixture contract for factory-ask-guard B6/B7. These provider-shaped
//! records contain no operator data; the independent expected text and limits
//! come from the sealed contract and the accepted UserTurnContent contract.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use hide_session::Agent;
use hide_session::label_transcript::{LabelTranscript, LabelTranscriptRequest, read};
use hide_session::turns::{TurnTracker, UserTurnContent, UserTurnFact, UserTurnKind, Waiting};
use serde_json::{Value, json};

struct Session {
    home: tempfile::TempDir,
    path: PathBuf,
    request: LabelTranscriptRequest,
}

fn fixture(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

impl Session {
    fn question(agent: Agent) -> Self {
        let (name, relative) = match agent {
            Agent::Claude => (
                "claude-2.1.288-question.jsonl",
                ".claude/projects/-work-app/question-session.jsonl",
            ),
            Agent::Codex => (
                "codex-0.160.1-question.jsonl",
                ".codex/sessions/2026/10/07/rollout-2026-10-07T01-00-00-0199a000-0000-7000-8000-0000000000b1.jsonl",
            ),
            _ => unreachable!("these fixtures cover native Claude/Codex"),
        };
        Self::new(agent, relative, &fixture(&format!("user-turns/{name}")))
    }

    fn new(agent: Agent, relative: &str, contents: &str) -> Self {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, contents).unwrap();
        let request = LabelTranscriptRequest {
            agent,
            reference_kind: "path".to_owned(),
            reference_value: path.display().to_string(),
            cwd: Some("/work/app".to_owned()),
            checkpoint: None,
            subagents: BTreeMap::new(),
            turns: None,
        };
        Self {
            home,
            path,
            request,
        }
    }

    fn read(&self) -> LabelTranscript {
        read(self.home.path(), &self.request).unwrap()
    }

    fn resume(&mut self, answer: &LabelTranscript) {
        // Both request and answer cross the real helper's serde boundary.
        let mut request = self.request.clone();
        request.checkpoint = Some(answer.checkpoint.clone());
        request.turns = answer.turns.clone();
        self.request = serde_json::from_slice(&serde_json::to_vec(&request).unwrap()).unwrap();
    }

    fn append(&self, record: &Value) {
        writeln!(
            fs::OpenOptions::new()
                .append(true)
                .open(&self.path)
                .unwrap(),
            "{record}"
        )
        .unwrap();
    }

    fn native_question(&self, call: &str, questions: Value) -> Value {
        match self.request.agent {
            Agent::Claude => {
                json!({"type":"assistant","sessionId":"question-session","timestamp":"2026-10-07T01:00:02Z",
                "message":{"role":"assistant","content":[{"type":"tool_use","id":call,"name":"AskUserQuestion","input":{"questions":questions}}]}})
            }
            _ => json!({"type":"response_item","timestamp":"2026-10-07T01:00:02Z",
                "payload":{"type":"function_call","call_id":call,"name":"request_user_input","arguments":json!({"questions":questions}).to_string()}}),
        }
    }

    fn result(&self, call: &str) -> Value {
        match self.request.agent {
            Agent::Claude => {
                json!({"type":"user","sessionId":"question-session","timestamp":"2026-10-07T01:00:03Z",
                "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":call,"content":"미리보기"}]}})
            }
            _ => json!({"type":"response_item","timestamp":"2026-10-07T01:00:03Z",
                "payload":{"type":"function_call_output","call_id":call,"output":"{\"answers\":{\"deploy\":{\"answers\":[\"미리보기\"]}}}"}}),
        }
    }

    fn human(&self, text: &str) -> Value {
        match self.request.agent {
            Agent::Claude => {
                json!({"type":"user","sessionId":"question-session","origin":{"kind":"human"},
                "timestamp":"2026-10-07T01:00:04Z","message":{"role":"user","content":text}})
            }
            _ => json!({"type":"response_item","timestamp":"2026-10-07T01:00:04Z",
                "payload":{"type":"message","role":"user","content":[{"type":"input_text","text":text}]}}),
        }
    }
}

fn fact(answer: &LabelTranscript) -> Option<UserTurnFact> {
    answer
        .turns
        .as_ref()
        .expect("native reader reports turns")
        .user_turn()
}

fn question(text: &str, choices: &[&str]) -> Value {
    json!([{"id":"choice","header":"선택","question":text,
        "options":choices.iter().map(|label| json!({"label":label,"description":"native option"})).collect::<Vec<_>>() }])
}

#[test]
fn native_question_fixtures_preserve_exact_text_and_choices_through_serde() {
    for agent in [Agent::Claude, Agent::Codex] {
        let session = Session::question(agent);
        let answer: LabelTranscript =
            serde_json::from_slice(&serde_json::to_vec(&session.read()).unwrap()).unwrap();
        assert_eq!(
            answer.turns.as_ref().unwrap().waiting(),
            Some(Waiting::Question)
        );
        assert_eq!(
            serde_json::to_value(fact(&answer)).unwrap(),
            json!({
                "kind":"question", "content":{"text":"배포 대상을 골라주세요\n검토 방식을 고르세요",
                "choices":["미리보기","운영","직접","자동"], "truncated":false}
            })
        );
    }
}

#[test]
fn only_the_correlated_native_result_clears_a_question_and_a_replayed_call_stays_answered() {
    for agent in [Agent::Claude, Agent::Codex] {
        let mut session = Session::question(agent);
        let asked = session.read();
        session.resume(&asked);
        session.append(&session.result("unrelated-call"));
        let unrelated = session.read();
        assert_eq!(fact(&unrelated), fact(&asked));
        session.resume(&unrelated);
        session.append(&session.result("question-1"));
        let answered = session.read();
        assert_eq!(
            fact(&answered),
            None,
            "{agent:?} native answer clears its fact"
        );
        session.resume(&answered);
        session.append(&session.native_question(
            "question-1",
            question("A replay must not ask again", &["Again"]),
        ));
        assert_eq!(fact(&session.read()), None, "{agent:?} call-id replay");
    }
}

#[test]
fn a_human_answer_or_native_abort_clears_question_content() {
    for agent in [Agent::Claude, Agent::Codex] {
        for interrupted in [false, true] {
            let mut session = Session::question(agent);
            let asked = session.read();
            session.resume(&asked);
            let record = if interrupted && agent == Agent::Codex {
                json!({"type":"event_msg","timestamp":"2026-10-07T01:00:04Z","payload":{"type":"turn_aborted","turn_id":"turn-1"}})
            } else {
                session.human(if interrupted {
                    "[Request interrupted by user]"
                } else {
                    "미리보기로 해줘"
                })
            };
            session.append(&record);
            assert_eq!(
                fact(&session.read()),
                None,
                "{agent:?} interruption={interrupted}"
            );
        }
    }
}

#[test]
fn anchor_replay_and_session_replacement_do_not_reopen_an_answered_question() {
    for agent in [Agent::Claude, Agent::Codex] {
        let mut session = Session::question(agent);
        let asked = session.read();
        session.resume(&asked);
        session.append(&session.result("question-1"));
        let answered = session.read();
        session.resume(&answered);
        session.request.checkpoint = asked.anchor.clone();
        assert_eq!(fact(&session.read()), None, "{agent:?} anchor replay");

        // A shorter replacement is a rescan, so a question from the prior
        // incarnation does not carry into the replacement's first record.
        let first = fs::read_to_string(&session.path)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .to_owned();
        session.resume(&asked);
        fs::write(&session.path, format!("{first}\n")).unwrap();
        let replaced = session.read();
        assert!(replaced.rescanned.is_some());
        assert_eq!(fact(&replaced), None, "{agent:?} replacement");
    }
}

#[test]
fn native_content_limits_cut_utf8_and_report_omitted_questions_and_choices() {
    for agent in [Agent::Claude, Agent::Codex] {
        let mut session = Session::question(agent);
        let asked = session.read();
        session.resume(&asked);
        session.append(&session.human("new question"));
        let text = "질".repeat(2731);
        let choice = "선".repeat(86);
        let choices = vec![choice.as_str(); 9];
        session.append(&session.native_question("question-limit", question(&text, &choices)));
        let content = fact(&session.read()).unwrap().content.unwrap();
        assert_eq!(content.text(), "질".repeat(2730));
        assert_eq!(content.choices(), vec!["선".repeat(85); 8]);
        assert!(content.truncated());

        let mut second = Session::question(agent);
        let first = second.read();
        second.resume(&first);
        second.append(&second.human("multiple questions"));
        second.append(&second.native_question(
            "many",
            Value::Array((0..9).map(|_| question("Q", &[])[0].clone()).collect()),
        ));
        let content = fact(&second.read()).unwrap().content.unwrap();
        assert_eq!(content.text(), "Q\nQ\nQ\nQ\nQ\nQ\nQ\nQ");
        assert!(content.truncated(), "omitting a native question is visible");
    }
}

#[test]
fn native_call_and_id_capacity_is_a_reported_read_failure() {
    for agent in [Agent::Claude, Agent::Codex] {
        for long_id in [false, true] {
            let mut session = Session::question(agent);
            let asked = session.read();
            session.resume(&asked);
            if long_id {
                session.append(&session.native_question(&"x".repeat(257), question("Q", &["A"])));
            } else {
                // The fixture already holds one unanswered call.
                for index in 1..9 {
                    session.append(
                        &session
                            .native_question(&format!("capacity-{index}"), question("Q", &["A"])),
                    );
                }
                // A later answer cannot hide a capacity failure in this read.
                session.append(&session.human("미리보기"));
            }
            assert_eq!(
                read(session.home.path(), &session.request).unwrap_err(),
                "user_turn_capacity"
            );
        }
    }
}

#[test]
fn oversized_native_questions_and_answers_cannot_return_stale_or_missing_facts() {
    for agent in [Agent::Claude, Agent::Codex] {
        for answering in [false, true] {
            let mut session = Session::question(agent);
            let asked = session.read();
            assert_eq!(fact(&asked).unwrap().kind, UserTurnKind::Question);
            session.resume(&asked);
            let mut record = if answering {
                session.result("question-1")
            } else {
                session.native_question("question-large", question("", &["A"]))
            };
            let path = if agent == Agent::Claude {
                "/message/content/0/content"
            } else {
                "/payload/output"
            };
            if answering {
                *record.pointer_mut(path).unwrap() = json!("");
            }
            // Include the physical newline in the source-admission budget.
            let huge =
                "x".repeat(hide_session::SESSION_LINE_LIMIT_BYTES - record.to_string().len());
            if answering {
                *record.pointer_mut(path).unwrap() = json!(huge);
            } else {
                record = session.native_question("question-large", question(&huge, &["A"]));
            }
            assert_eq!(
                record.to_string().len() + 1,
                hide_session::SESSION_LINE_LIMIT_BYTES + 1
            );
            session.append(&record);
            assert_eq!(
                read(session.home.path(), &session.request).unwrap_err(),
                format!(
                    "session_capacity:line_bytes:{}",
                    hide_session::SESSION_LINE_LIMIT_BYTES
                )
            );
        }
    }
}

#[test]
fn a_large_native_question_at_the_record_cap_still_truncates_its_content() {
    for agent in [Agent::Claude, Agent::Codex] {
        let mut session = Session::question(agent);
        let asked = session.read();
        session.resume(&asked);
        session.append(&session.result("question-1"));
        let envelope = session.native_question("question-large", question("", &["A"]));
        let text =
            "x".repeat(hide_session::SESSION_LINE_LIMIT_BYTES - envelope.to_string().len() - 1);
        let record = session.native_question("question-large", question(&text, &["A"]));
        assert_eq!(
            record.to_string().len() + 1,
            hide_session::SESSION_LINE_LIMIT_BYTES
        );
        session.append(&record);
        let content = fact(&session.read()).unwrap().content.unwrap();
        assert_eq!(content.text(), "x".repeat(8192));
        assert_eq!(content.choices(), ["A"]);
        assert!(content.truncated());
    }
}

#[test]
fn a_native_question_without_a_valid_correlation_is_a_reported_read_failure() {
    for agent in [Agent::Claude, Agent::Codex] {
        for invalid in [None, Some(Value::Null), Some(json!("")), Some(json!(17))] {
            let session = Session::question(agent);
            let mut record = session.native_question("question-invalid", question("Q", &["A"]));
            let (path, key) = if agent == Agent::Claude {
                ("/message/content/0", "id")
            } else {
                ("/payload", "call_id")
            };
            let native = record.pointer_mut(path).unwrap().as_object_mut().unwrap();
            if let Some(invalid) = invalid {
                native.insert(key.to_owned(), invalid);
            } else {
                native.remove(key);
            }
            session.append(&record);
            assert_eq!(
                read(session.home.path(), &session.request).unwrap_err(),
                "user_turn_invalid"
            );
        }
    }
}

#[test]
fn a_completed_native_plan_carries_its_text_and_clears_on_answer_without_invented_choices() {
    let contents = fixture(
        "adapters/codex-0.160.1-plan/sessions/2026/10/07/rollout-2026-10-07T01-00-00-0199a000-0000-7000-8000-0000000000a1.jsonl",
    );
    let mut session = Session::new(
        Agent::Codex,
        ".codex/sessions/2026/10/07/rollout-2026-10-07T01-00-00-0199a000-0000-7000-8000-0000000000a1.jsonl",
        &contents,
    );
    let plan = session.read();
    assert_eq!(
        serde_json::to_value(fact(&plan)).unwrap(),
        json!({"kind":"plan_approval",
        "content":{"text":"1. 입력 검증을 고친다\n2. 테스트를 더한다","choices":[],"truncated":false}})
    );
    session.resume(&plan);
    session.append(&session.human("Implement the plan."));
    let answered = session.read();
    assert_eq!(fact(&answered), None);
    session.resume(&answered);
    session.append(
        &json!({"type":"event_msg","timestamp":"2026-10-07T01:00:04Z",
        "payload":{"type":"task_complete","turn_id":"turn-2"}}),
    );
    assert_eq!(
        fact(&session.read()),
        None,
        "repeated native turn completion cannot reopen a plan"
    );
}

#[test]
fn missing_native_content_stays_absent_and_plain_question_prose_is_no_native_fact() {
    for agent in [Agent::Claude, Agent::Codex] {
        let mut session = Session::question(agent);
        let asked = session.read();
        session.resume(&asked);
        session.append(&session.result("question-1"));
        let prose = if agent == Agent::Claude {
            json!({"type":"assistant","sessionId":"question-session","timestamp":"2026-10-07T01:00:04Z",
                "message":{"role":"assistant","content":[{"type":"text","text":"Which choice would you prefer?"}]}})
        } else {
            json!({"type":"response_item","timestamp":"2026-10-07T01:00:04Z",
                "payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"An example <proposed_plan> is not a plan. Which choice would you prefer?"}]}})
        };
        session.append(&prose);
        let no_native_question = session.read();
        assert_eq!(
            fact(&no_native_question),
            None,
            "{agent:?} prose supplies no native fact"
        );
        session.resume(&no_native_question);
        session.append(&session.human("new request"));
        session.append(&session.native_question("missing", json!([{"header":"No text"}])));
        assert_eq!(
            fact(&session.read()),
            Some(UserTurnFact {
                kind: UserTurnKind::Question,
                content: None
            })
        );

        fs::remove_file(&session.path).unwrap();
        assert!(
            read(session.home.path(), &session.request).is_err(),
            "an unreadable source supplies no fabricated fact"
        );
    }
}

#[test]
fn old_checkpoints_keep_their_kind_and_foreign_content_cannot_exceed_the_contract() {
    let old: TurnTracker = serde_json::from_value(json!({"through":5,"last":{
        "id":"turn-1","mode":"plan","plan":true,"end":"completed","answered":false}}))
    .unwrap();
    assert_eq!(
        old.user_turn(),
        Some(UserTurnFact {
            kind: UserTurnKind::PlanApproval,
            content: None
        })
    );
    for content in [
        json!({"text":"x".repeat(8193),"choices":[],"truncated":true}),
        json!({"text":"Q","choices":vec!["A";9],"truncated":true}),
        json!({"text":"Q","choices":["x".repeat(257)],"truncated":true}),
    ] {
        assert!(serde_json::from_value::<UserTurnContent>(content).is_err());
    }
}
