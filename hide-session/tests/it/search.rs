use hide_session::{Agent, search::SearchIndex, search_read};
use std::fs::{self, OpenOptions};
use std::io::Write;
use tempfile::tempdir;

fn event(role: &str, text: &str) -> String {
    serde_json::json!({"type":"response_item","timestamp":"2026-10-01T00:00:00Z","payload":{"type":"message","role":role,"content":[{"type":if role=="user"{"input_text"}else{"output_text"},"text":text}]}}).to_string()+"\n"
}
fn index_all(index: &mut SearchIndex, path: &std::path::Path, project: &str, session: &str) {
    for _ in 0..12 {
        if !search_read::update(index, project, session, Agent::Codex, path, 0).unwrap() {
            return;
        }
    }
    panic!("index failed to finish within the byte budget");
}
#[test]
fn korean_literal_queries_group_by_session_and_keep_exact_message_offsets() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("s.jsonl");
    let first = event("user", "첫 요청은 메타데이터뿐입니다");
    fs::write(
        &path,
        first.clone()
            + &event(
                "assistant",
                "설정에서 대화검색과 foo_bar, OR \"(literal)*\" 연결을 처리합니다",
            )
            + &event("user", "대화검색 다시 확인"),
    )
    .unwrap();
    let mut index = SearchIndex::open(&tmp.path().join("index.db")).unwrap();
    index_all(&mut index, &path, "project-a", "session-a");
    for query in [
        "화검",
        "대화검",
        "foo_bar",
        "OR \"(literal)*\"",
        "연결",
        "처리",
    ] {
        let result = search_read::search(&index, "project-a", query, 0).unwrap();
        assert_eq!(result.hits.len(), 1, "{query}");
        assert!(
            result.hits[0]
                .snippet
                .to_lowercase()
                .contains(&query.to_lowercase())
        );
    }
    let hit = &search_read::search(&index, "project-a", "foo_bar", 0)
        .unwrap()
        .hits[0];
    assert_eq!(hit.source_offset, first.len() as u64);
    assert_eq!(hit.role, "assistant");
    assert!(
        search_read::search(&index, "project-b", "대화", 0)
            .unwrap()
            .hits
            .is_empty()
    );
    assert!(
        search_read::search(&index, "project-a", "definitely absent", 0)
            .unwrap()
            .hits
            .is_empty()
    );
}
#[test]
fn tool_record_larger_than_two_megabytes_resumes_through_restart() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("s.jsonl");
    let db = tmp.path().join("index.db");
    let tool=serde_json::json!({"type":"response_item","timestamp":"2026-10-01T00:00:00Z","payload":{"type":"function_call_output","output":"x".repeat(3*1024*1024)}}).to_string()+"\n";
    fs::write(
        &path,
        tool.clone()
            + &event("user", "긴 도구 뒤 정상 요청")
            + &event("assistant", "후속 답변 검색 성공"),
    )
    .unwrap();
    let mut index = SearchIndex::open(&db).unwrap();
    assert!(search_read::update(&mut index, "p", "s", Agent::Codex, &path, 0).unwrap());
    drop(index); // persisted while discarding the oversized record
    let mut index = SearchIndex::open(&db).unwrap();
    index_all(&mut index, &path, "p", "s");
    let result = search_read::search(&index, "p", "정상 요청", 0).unwrap();
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].source_offset, tool.len() as u64);
    assert_eq!(
        search_read::search(&index, "p", "후속 답변", 0)
            .unwrap()
            .hits
            .len(),
        1
    );
    assert!(
        search_read::search(&index, "p", "xxx", 0)
            .unwrap()
            .hits
            .is_empty()
    );
}
#[test]
fn append_torn_restart_replace_truncate_delete_and_clear_never_return_invalid_copy() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("s.jsonl");
    let db = tmp.path().join("index.db");
    let original = event("user", "old unique");
    fs::write(&path, &original).unwrap();
    let mut index = SearchIndex::open(&db).unwrap();
    index_all(&mut index, &path, "p", "s");
    let append = event("assistant", "한국어 신규 답변");
    let split = append.len() / 2;
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(&append.as_bytes()[..split])
        .unwrap();
    index_all(&mut index, &path, "p", "s");
    drop(index);
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(&append.as_bytes()[split..])
        .unwrap();
    let mut index = SearchIndex::open(&db).unwrap();
    index_all(&mut index, &path, "p", "s");
    assert_eq!(
        search_read::search(&index, "p", "신규", 0)
            .unwrap()
            .hits
            .len(),
        1
    );
    let replacement = tmp.path().join("replacement");
    fs::write(&replacement, event("user", "replacement 새 메시지")).unwrap();
    fs::rename(&replacement, &path).unwrap();
    assert!(
        search_read::search(&index, "p", "old unique", 0)
            .unwrap()
            .hits
            .is_empty()
    );
    index_all(&mut index, &path, "p", "s");
    assert!(
        search_read::search(&index, "p", "old unique", 0)
            .unwrap()
            .hits
            .is_empty()
    );
    fs::write(&path, event("user", "truncated 내용")).unwrap();
    index_all(&mut index, &path, "p", "s");
    assert!(
        search_read::search(&index, "p", "replacement", 0)
            .unwrap()
            .hits
            .is_empty()
    );
    let before = fs::read(&path).unwrap();
    index.clear("p").unwrap();
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(
        search_read::search(&index, "p", "내용", 0)
            .unwrap()
            .hits
            .is_empty()
    );
    index_all(&mut index, &path, "p", "s");
    fs::remove_file(&path).unwrap();
    assert!(
        search_read::search(&index, "p", "내용", 0)
            .unwrap()
            .hits
            .is_empty()
    );
    index.remove("p", Some("s")).unwrap();
}
#[test]
fn unchanged_file_needs_no_cursor_write_and_retention_is_project_local() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("s.jsonl");
    let db = tmp.path().join("index.db");
    fs::write(&path, event("user", "검색 원본")).unwrap();
    let mut index = SearchIndex::open(&db).unwrap();
    index_all(&mut index, &path, "p", "s");
    let before = fs::read(&db).unwrap();
    index_all(&mut index, &path, "p", "s");
    assert_eq!(before, fs::read(&db).unwrap());
    index.set_days("p", 0).unwrap();
    assert_eq!(index.days("p").unwrap(), 0);
    assert_eq!(index.days("other").unwrap(), 90);
    assert!(
        search_read::search(&index, "p", "원본", 0)
            .unwrap()
            .hits
            .is_empty()
    );
    assert!(index.set_days("p", 7).is_err());
    assert_eq!(index.days("p").unwrap(), 0);
}

#[test]
fn expired_bodies_are_removed_even_if_the_transcript_is_unchanged() {
    let tmp = tempdir().unwrap();
    let source = tmp.path().join("session.jsonl");
    let original = event("user", "retained body expires");
    fs::write(&source, &original).unwrap();
    let mut index = SearchIndex::open(&tmp.path().join("index.db")).unwrap();
    index_all(&mut index, &source, "p", "s");
    let at = search_read::search(&index, "p", "expires", 0).unwrap().hits[0].at_unix_ms;
    index.prune("p", at + 1).unwrap();
    assert!(
        search_read::search(&index, "p", "expires", 0)
            .unwrap()
            .hits
            .is_empty()
    );
    assert_eq!(fs::read_to_string(&source).unwrap(), original);
}

#[test]
fn middle_rewrites_and_rewrites_with_append_invalidate_the_entire_consumed_prefix() {
    let tmp = tempdir().unwrap();
    let source = tmp.path().join("session.jsonl");
    let around = event("assistant", &"padding ".repeat(120));
    let original = around.clone() + &event("user", "old middle marker") + &around;
    fs::write(&source, &original).unwrap();
    let mut index = SearchIndex::open(&tmp.path().join("index.db")).unwrap();
    index_all(&mut index, &source, "p", "s");
    fs::write(
        &source,
        original.replace("old middle marker", "new middle marker"),
    )
    .unwrap();
    index_all(&mut index, &source, "p", "s");
    assert!(
        search_read::search(&index, "p", "old middle", 0)
            .unwrap()
            .hits
            .is_empty()
    );
    assert_eq!(
        search_read::search(&index, "p", "new middle", 0)
            .unwrap()
            .hits
            .len(),
        1
    );
    fs::write(
        &source,
        original.replace("old middle marker", "app middle marker")
            + &event("assistant", "appended answer"),
    )
    .unwrap();
    index_all(&mut index, &source, "p", "s");
    assert!(
        search_read::search(&index, "p", "new middle", 0)
            .unwrap()
            .hits
            .is_empty()
    );
    assert_eq!(
        search_read::search(&index, "p", "app middle", 0)
            .unwrap()
            .hits
            .len(),
        1
    );
}

#[test]
fn oversized_claude_tool_blocks_resume_after_restart_without_losing_human_text() {
    let tmp = tempdir().unwrap();
    let source = tmp.path().join("claude.jsonl");
    let human = |text: &str| {
        serde_json::json!({"type":"user","userType":"external","promptId":"p","timestamp":"2026-10-01T00:00:00Z","message":{"role":"user","content":text}}).to_string()+"\n"
    };
    // A named Bash call cannot answer a native question. A correlated
    // result omits the tool name and cannot safely authorize a discard.
    let tool = serde_json::json!({"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash","input":{"command":"x".repeat(3*1024*1024)}}]}}).to_string()+"\n";
    fs::write(
        &source,
        human("before tool") + &tool + &human("after tool 대화검색"),
    )
    .unwrap();
    let database = tmp.path().join("index.db");
    let mut index = SearchIndex::open(&database).unwrap();
    assert!(search_read::update(&mut index, "p", "s", Agent::Claude, &source, 0).unwrap());
    drop(index);
    let mut index = SearchIndex::open(&database).unwrap();
    let mut finished = false;
    for _ in 0..12 {
        if !search_read::update(&mut index, "p", "s", Agent::Claude, &source, 0).unwrap() {
            finished = true;
            break;
        }
    }
    assert!(finished);
    assert_eq!(
        search_read::search(&index, "p", "before tool", 0)
            .unwrap()
            .hits
            .len(),
        1
    );
    assert_eq!(
        search_read::search(&index, "p", "after tool", 0)
            .unwrap()
            .hits
            .len(),
        1
    );
    // A mixed envelope's text is lost with its large tool input, and the
    // read goes on to the records after it.
    fs::write(&source, serde_json::json!({"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","input":{"data":"x".repeat(2*1024*1024)}},{"type":"text","text":"lost with its tool input"}]}}).to_string()+"\n"+&human("after mixed 대화검색")).unwrap();
    let mut finished = false;
    for _ in 0..12 {
        if !search_read::update(&mut index, "p", "s", Agent::Claude, &source, 0).unwrap() {
            finished = true;
            break;
        }
    }
    assert!(finished);
    assert!(
        search_read::search(&index, "p", "lost with", 0)
            .unwrap()
            .hits
            .is_empty()
    );
    assert_eq!(
        search_read::search(&index, "p", "after mixed", 0)
            .unwrap()
            .hits
            .len(),
        1
    );
}

#[cfg(unix)]
#[test]
fn special_file_replacement_fails_promptly_instead_of_waiting_for_a_writer() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let tmp = tempdir().unwrap();
    let source = tmp.path().join("session.jsonl");
    fs::write(&source, event("user", "original")).unwrap();
    let mut index = SearchIndex::open(&tmp.path().join("index.db")).unwrap();
    index_all(&mut index, &source, "p", "s");
    fs::remove_file(&source).unwrap();
    let path = CString::new(source.as_os_str().as_bytes()).unwrap();
    // Safety: a valid NUL-terminated path in an isolated test directory.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let started = std::time::Instant::now();
    assert!(
        search_read::update(&mut index, "p", "s", Agent::Codex, &source, 0)
            .unwrap_err()
            .contains("regular file")
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
}

#[test]
fn prolific_session_and_provider_scopes_are_bounded_after_grouping() {
    let tmp = tempdir().unwrap();
    let mut index = SearchIndex::open(&tmp.path().join("index.db")).unwrap();
    let path = tmp.path().join("prolific.jsonl");
    fs::write(&path, event("assistant", "shared needle").repeat(550)).unwrap();
    index_all(&mut index, &path, "p", "prolific");
    let older = event("assistant", "shared needle").replace("2026-10-01", "2026-09-30");
    for n in 0..100 {
        let path = tmp.path().join(format!("codex-{n}.jsonl"));
        fs::write(&path, event("user", "shared needle")).unwrap();
        index_all(&mut index, &path, "p", &format!("codex-{n}"));
    }
    let path = tmp.path().join("claude.jsonl");
    fs::write(&path, &older).unwrap();
    index_all(&mut index, &path, "p", "claude-older");
    let two = vec!["prolific".into(), "claude-older".into()];
    let page = index
        .search_scoped("p", "shared needle", 0, Some(&two), &mut |paths| {
            Ok(search_read::stamps(paths))
        })
        .unwrap();
    assert_eq!(page.hits.len(), 2);
    assert!(!page.limited);
    assert!(
        search_read::search(&index, "p", "shared needle", 0)
            .unwrap()
            .limited
    );
    let page = index
        .search_scoped(
            "p",
            "shared needle",
            0,
            Some(&["claude-older".into()]),
            &mut |paths| Ok(search_read::stamps(paths)),
        )
        .unwrap();
    assert_eq!(page.hits[0].session_id, "claude-older");
    assert!(!page.limited);
}

#[test]
fn inactive_project_copies_expire_under_their_own_policies() {
    let tmp = tempdir().unwrap();
    let mut index = SearchIndex::open(&tmp.path().join("index.db")).unwrap();
    index.set_days("inactive", 30).unwrap();
    index.set_days("active", 90).unwrap();
    let path = tmp.path().join("s.jsonl");
    fs::write(&path, event("user", "retained body")).unwrap();
    index_all(&mut index, &path, "inactive", "s");
    index_all(&mut index, &path, "active", "s");
    let original = fs::read(&path).unwrap();
    // Oct 1 body is expired under 30 days on Nov 2, retained under 90.
    index.prune_all(1_793_577_600_000).unwrap();
    assert!(
        search_read::search(&index, "inactive", "retained", 0)
            .unwrap()
            .hits
            .is_empty()
    );
    assert_eq!(
        search_read::search(&index, "active", "retained", 0)
            .unwrap()
            .hits
            .len(),
        1
    );
    assert_eq!(fs::read(&path).unwrap(), original);
}

#[test]
fn all_source_work_is_bounded_and_unchanged_reads_are_zero() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("s.jsonl");
    // Put discriminators after the output to test resumable structural scanning.
    let tool = format!(
        "{{\"payload\":{{\"output\":\"{}\",\"type\":\"function_call_output\"}},\"type\":\"response_item\"}}\n",
        "x".repeat(5 * 1024 * 1024)
    );
    fs::write(&path, tool + &event("user", "bounded source body")).unwrap();
    let mut index = SearchIndex::open(&tmp.path().join("index.db")).unwrap();
    let measure = |index: &mut SearchIndex, label: &str| {
        let mut bytes = 0;
        let mut max = 0;
        let mut calls = 0;
        loop {
            calls += 1;
            let (more, r) =
                search_read::update_measured(index, "p", "s", Agent::Codex, &path, 0).unwrap();
            let n = r.cursor_bytes + r.witness_bytes;
            bytes += n;
            max = max.max(n);
            assert!(n <= 1024 * 1024, "{label}: {n}");
            assert!(calls < 40);
            if !more {
                break;
            }
        }
        eprintln!("source workload={label} calls={calls} read_bytes={bytes} max_call_bytes={max}");
        bytes
    };
    assert!(measure(&mut index, "backfill") > 5 * 1024 * 1024);
    assert_eq!(measure(&mut index, "unchanged"), 0);
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(event("assistant", "append needle").as_bytes())
        .unwrap();
    assert!(measure(&mut index, "append") > 0);
    assert_eq!(
        search_read::search(&index, "p", "append needle", 0)
            .unwrap()
            .hits
            .len(),
        1
    );
}

#[test]
fn escaped_conversation_discriminators_are_skipped_and_the_read_continues_after_restart() {
    for (agent, prefix, suffix, after) in [
        (
            Agent::Codex,
            r#"{"type":"response\u005fitem","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":""#,
            r#""}]}}"#,
            event("user", "after escaped 대화검색"),
        ),
        (
            Agent::Codex,
            r#"{"type":"response_item","payload":{"type":"mess\u0061ge","role":"assistant","content":[{"type":"output_text","text":""#,
            r#""}]}}"#,
            event("user", "after escaped 대화검색"),
        ),
        (
            Agent::Claude,
            r#"{"type":"us\u0065r","userType":"external","message":{"role":"user","content":""#,
            r#""}}"#,
            serde_json::json!({"type":"user","userType":"external","promptId":"p","timestamp":"2026-10-01T00:00:00Z",
                "message":{"role":"user","content":"after escaped 대화검색"}})
            .to_string()
                + "\n",
        ),
    ] {
        let tmp = tempdir().unwrap();
        let source = tmp.path().join("s.jsonl");
        fs::write(
            &source,
            format!("{prefix}{}{suffix}\n{after}", "real text ".repeat(300_000)),
        )
        .unwrap();
        let database = tmp.path().join("index.db");
        let mut index = SearchIndex::open(&database).unwrap();
        assert!(search_read::update(&mut index, "p", "s", agent, &source, 0).unwrap());
        drop(index);
        let mut index = SearchIndex::open(&database).unwrap();
        let mut finished = false;
        for _ in 0..12 {
            if !search_read::update(&mut index, "p", "s", agent, &source, 0).unwrap() {
                finished = true;
                break;
            }
        }
        assert!(finished, "{agent:?}");
        assert!(
            search_read::search(&index, "p", "real text", 0)
                .unwrap()
                .hits
                .is_empty()
        );
        assert_eq!(
            search_read::search(&index, "p", "after escaped", 0)
                .unwrap()
                .hits
                .len(),
            1,
            "{agent:?}"
        );
    }
}

#[test]
fn later_prefix_rewrite_with_append_survives_restart_during_validation() {
    let tmp = tempdir().unwrap();
    let source = tmp.path().join("s.jsonl");
    let database = tmp.path().join("index.db");
    let old = "obsolete later needle";
    let new = "replacement needle!!!";
    assert_eq!(old.len(), new.len());
    let tool = |n| {
        serde_json::json!({"type":"response_item","payload":{"type":"function_call_output","output":"x".repeat(n)}}).to_string()+"\n"
    };
    let mut body = event("user", "stable first message") + &tool(2_500_000);
    let expected_offset = body.len() as u64;
    body.push_str(&event("user", old));
    body.push_str(&tool(1_000_000));
    body.push_str(&event("assistant", "stable final message"));
    assert!(expected_offset > 2 * 1024 * 1024);
    fs::write(&source, &body).unwrap();
    let mut index = SearchIndex::open(&database).unwrap();
    index_all(&mut index, &source, "p", "s");
    assert_eq!(
        search_read::search(&index, "p", old, 0).unwrap().hits[0].source_offset,
        expected_offset
    );
    let changed = body.replacen(old, new, 1);
    let append_offset = changed.len() as u64;
    fs::write(
        &source,
        changed + &event("assistant", "new appended answer"),
    )
    .unwrap();
    let (more, reads) =
        search_read::update_measured(&mut index, "p", "s", Agent::Codex, &source, 0).unwrap();
    assert!(more);
    assert_eq!(reads.cursor_bytes, 0);
    assert_eq!(reads.witness_bytes, 1024 * 1024);
    drop(index);
    let mut index = SearchIndex::open(&database).unwrap();
    let mut finished = false;
    for _ in 0..40 {
        let (more, reads) =
            search_read::update_measured(&mut index, "p", "s", Agent::Codex, &source, 0).unwrap();
        assert!(reads.cursor_bytes + reads.witness_bytes <= 1024 * 1024);
        if !more {
            finished = true;
            break;
        }
    }
    assert!(finished);
    assert!(
        search_read::search(&index, "p", old, 0)
            .unwrap()
            .hits
            .is_empty()
    );
    assert_eq!(
        search_read::search(&index, "p", new, 0).unwrap().hits[0].source_offset,
        expected_offset
    );
    assert_eq!(
        search_read::search(&index, "p", "new appended answer", 0)
            .unwrap()
            .hits[0]
            .source_offset,
        append_offset
    );
}
#[test]
fn rekeying_onto_a_project_indexed_since_keeps_the_newer_rows_and_drops_the_old_ones() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("s.jsonl");
    fs::write(&path, event("user", "대화검색 이전 키")).unwrap();
    let mut index = SearchIndex::open(&tmp.path().join("index.db")).unwrap();
    // Setting the days clears a project's index, so they come first. The same
    // session is then indexed under the old id and, since, under the new one.
    index.set_days("project-old", 90).unwrap();
    index.set_days("project-new", 30).unwrap();
    index_all(&mut index, &path, "project-old", "session-a");
    index_all(&mut index, &path, "project-new", "session-a");

    let (moved, dropped) = index
        .rekey_projects(&[("project-old".to_owned(), "project-new".to_owned())])
        .unwrap();
    assert_eq!(moved, 0, "every old row collides with a newer one");
    assert!(dropped >= 3, "policy, file and message rows: {dropped}");
    assert!(
        search_read::search(&index, "project-old", "대화", 0)
            .unwrap()
            .hits
            .is_empty()
    );
    assert_eq!(
        search_read::search(&index, "project-new", "대화", 0)
            .unwrap()
            .hits
            .len(),
        1
    );
    assert_eq!(index.days("project-new").unwrap(), 30);
}

// The index holds 25,000 copied messages in all. These tests fill it with
// synthetic reads (`IndexStep::Read`) so no session file is needed, and read
// the count from the database the way the search worker's limit does.
mod capacity {
    use hide_session::search::{IndexStep, IndexedMessage, SearchIndex};
    use std::path::Path;
    use tempfile::tempdir;

    const CAP: usize = 25_000;

    fn read(messages: Vec<IndexedMessage>) -> IndexStep {
        IndexStep::Read {
            reset: false,
            messages,
            cursor: "c".into(),
            stamp: "s".into(),
            witness: "w".into(),
            more: false,
        }
    }
    /// `count` messages of `session`, the newest at `newest` ms.
    fn messages(session: &str, count: usize, newest: u64) -> Vec<IndexedMessage> {
        (0..count)
            .map(|i| IndexedMessage {
                offset: i as u64,
                role: "user".into(),
                at_unix_ms: newest - (count - 1 - i) as u64,
                text: format!("needle {session} message {i}"),
            })
            .collect()
    }
    fn index_session(
        index: &mut SearchIndex,
        project: &str,
        session: &str,
        count: usize,
        newest: u64,
    ) -> Result<(), String> {
        // A read is at most 1 MiB of a file, so a big session arrives in many.
        for chunk in messages(session, count, newest).chunks(500) {
            index.apply(project, session, session, 0, read(chunk.to_vec()))?;
        }
        Ok(())
    }
    fn stored(database: &Path) -> usize {
        rusqlite::Connection::open(database)
            .unwrap()
            .query_row("SELECT count(*) FROM messages", [], |r| r.get(0))
            .unwrap()
    }
    /// The sessions of `project` the search finds a hit in.
    fn found(index: &SearchIndex, project: &str) -> Vec<String> {
        let mut sessions = index
            .search_scoped(project, "needle", 0, None, &mut |paths| {
                Ok(paths.iter().map(|_| Some("s".to_owned())).collect())
            })
            .unwrap()
            .hits
            .into_iter()
            .map(|hit| hit.session_id)
            .collect::<Vec<_>>();
        sessions.sort();
        sessions
    }
    fn full_of_another_project(index: &mut SearchIndex) {
        for k in 0..5 {
            index_session(index, "other", &format!("o{k}"), 5_000, 10_000 + k * 10_000).unwrap();
        }
    }

    #[test]
    fn the_index_holds_exactly_the_documented_number_of_messages() {
        let tmp = tempdir().unwrap();
        let database = tmp.path().join("index.db");
        let mut index = SearchIndex::open(&database).unwrap();
        full_of_another_project(&mut index);
        assert_eq!(stored(&database), CAP);
    }

    #[test]
    fn a_new_project_session_displaces_the_oldest_sessions_of_another_project() {
        let tmp = tempdir().unwrap();
        let database = tmp.path().join("index.db");
        let mut index = SearchIndex::open(&database).unwrap();
        full_of_another_project(&mut index);
        assert_eq!(found(&index, "other").len(), 5);

        index_session(&mut index, "viewed", "fresh", 3, 900_000).unwrap();

        assert_eq!(found(&index, "viewed"), ["fresh"]);
        assert_eq!(stored(&database), CAP - 5_000 + 3);
        // The oldest session went whole, with its saved cursor, so it is read
        // again from the start rather than searched half-copied.
        assert_eq!(found(&index, "other"), ["o1", "o2", "o3", "o4"]);
        assert!(index.saved("other", "o0").unwrap().is_none());
        assert!(index.saved("other", "o1").unwrap().is_some());
    }

    #[test]
    fn a_session_that_does_not_fit_leaves_every_other_session_in_place() {
        let tmp = tempdir().unwrap();
        let database = tmp.path().join("index.db");
        let mut index = SearchIndex::open(&database).unwrap();
        index_session(&mut index, "viewed", "huge", CAP, 5_000_000).unwrap();

        // A second session of the viewed Project, older than "huge": nothing
        // of its own newer content may be displaced for it.
        let error = index_session(&mut index, "viewed", "older", 10, 1_000).unwrap_err();

        assert!(error.contains("Copied history"), "{error}");
        assert!(!error.contains("clear the index"), "{error}");
        assert_eq!(stored(&database), CAP);
        assert_eq!(found(&index, "viewed"), ["huge"]);
    }

    #[test]
    fn a_newer_session_displaces_the_older_sessions_of_its_own_project() {
        let tmp = tempdir().unwrap();
        let database = tmp.path().join("index.db");
        let mut index = SearchIndex::open(&database).unwrap();
        for k in 0..5 {
            index_session(
                &mut index,
                "viewed",
                &format!("v{k}"),
                5_000,
                10_000 + k * 10_000,
            )
            .unwrap();
        }

        index_session(&mut index, "viewed", "new", 10, 900_000).unwrap();

        assert_eq!(found(&index, "viewed"), ["new", "v1", "v2", "v3", "v4"]);
        assert_eq!(stored(&database), CAP - 5_000 + 10);
        assert!(index.saved("viewed", "v0").unwrap().is_none());
    }

    #[test]
    fn the_session_being_read_is_never_its_own_victim() {
        let tmp = tempdir().unwrap();
        let database = tmp.path().join("index.db");
        let mut index = SearchIndex::open(&database).unwrap();
        index_session(&mut index, "viewed", "s", CAP - 5, 1_000_000).unwrap();
        // The same session grows by 10 messages past the cap.
        let step = IndexStep::Read {
            reset: false,
            messages: (0..10)
                .map(|i| IndexedMessage {
                    offset: 10_000_000 + i,
                    role: "assistant".into(),
                    at_unix_ms: 2_000_000 + i,
                    text: format!("needle grown {i}"),
                })
                .collect(),
            cursor: "c2".into(),
            stamp: "s".into(),
            witness: "w".into(),
            more: false,
        };

        let error = index.apply("viewed", "s", "s", 0, step).unwrap_err();

        assert!(error.contains("Copied history"), "{error}");
        assert_eq!(stored(&database), CAP - 5);
        assert_eq!(found(&index, "viewed"), ["s"]);
    }
}
