//! The helper's link calls over its shipped boundary (PRD link-graph D-21,
//! B40): a device answers the session files changed since a time and the
//! link facts read from them, and never a conversation or a file outside
//! the agent roots.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use hide_platform::process::OwnedChild;
use serde_json::{Value, json};

const HELPER: &str = env!("CARGO_BIN_EXE_hide-host-helper");
const DEADLINE: Duration = Duration::from_secs(5);
const OUTPUT_LIMIT: usize = 64 * 1024;
const REPLY: &str = "assistant reply that is conversation, not a link fact";

fn serve(home: &Path, requests: &[Value]) -> BTreeMap<u64, Value> {
    let mut command = Command::new(HELPER);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("HIDE_") || key.to_string_lossy().starts_with("HERDR_")
        {
            command.env_remove(key);
        }
    }
    command
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("PATH", "")
        .arg("serve")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let deadline = Instant::now() + DEADLINE;
    let mut child = OwnedChild::spawn(&mut command).unwrap();
    let input = requests
        .iter()
        .map(|request| format!("{request}\n"))
        .collect::<String>();
    assert!(input.len() < 4096);
    child
        .take_stdin()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.capture_until(deadline, OUTPUT_LIMIT).unwrap();
    assert!(output.status.success());
    let mut answers = BTreeMap::new();
    for line in output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let answer: Value = serde_json::from_slice(line).unwrap();
        answers.insert(answer["id"].as_u64().unwrap(), answer);
    }
    assert_eq!(answers.len(), requests.len());
    answers
}

#[test]
fn a_device_lists_its_changed_sessions_and_answers_their_link_facts_only() {
    let home = tempfile::tempdir().unwrap();
    let home_path = home.path().canonicalize().unwrap();
    let dir = home_path.join(".claude/projects/-work-app");
    fs::create_dir_all(&dir).unwrap();
    let session = dir.join("s1.jsonl");
    let lines = [
        json!({"type": "user", "isSidechain": false, "uuid": "u1", "parentUuid": null,
            "message": {"role": "user", "content": "PR 올려 줘"},
            "timestamp": "2026-10-03T01:00:00Z", "promptId": "p", "origin": {"kind": "human"},
            "userType": "external", "entrypoint": "cli", "cwd": "/work/app",
            "sessionId": "s1", "gitBranch": "feat"}),
        json!({"type": "assistant", "isSidechain": false, "uuid": "a1", "parentUuid": "u1",
            "message": {"role": "assistant", "content": [{"type": "text", "text": REPLY}]},
            "timestamp": "2026-10-03T01:00:05Z", "cwd": "/work/app", "sessionId": "s1",
            "gitBranch": "feat"}),
        json!({"type": "pr-link", "sessionId": "s1", "prNumber": 7, "prRepository": "acme/app",
            "prUrl": "https://github.com/acme/app/pull/7", "timestamp": "2026-10-03T01:00:09Z"}),
    ];
    fs::write(
        &session,
        lines.map(|line| line.to_string()).join("\n") + "\n",
    )
    .unwrap();
    let outside = home_path.join("elsewhere.jsonl");
    fs::copy(&session, &outside).unwrap();

    let answers = serve(
        &home_path,
        &[
            json!({"id": 1, "op": "link_files", "since_unix_ms": 0}),
            json!({"id": 2, "op": "link_read", "requests": [
                {"agent": "claude", "path": session.to_string_lossy()},
                {"agent": "claude", "path": outside.to_string_lossy()},
            ]}),
            json!({"id": 3, "op": "link_read", "requests": (0..9).map(|_| json!(
                {"agent": "claude", "path": session.to_string_lossy()})).collect::<Vec<_>>()}),
        ],
    );

    let listed = answers[&1]["ok"].as_array().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["path"], session.to_string_lossy().as_ref());
    let read = answers[&2]["ok"].as_array().unwrap();
    let facts = &read[0]["facts"];
    assert_eq!(facts["session_id"], "s1");
    assert_eq!(facts["spans"][0]["branch"], "feat");
    assert_eq!(facts["prs"][0]["number"], 7);
    assert_eq!(facts["prs"][0]["request"], "PR 올려 줘");
    assert!(read[0]["checkpoint"].is_object());
    assert_eq!(read[1]["error"], "links_session_outside_roots");
    assert!(
        !answers[&2].to_string().contains(REPLY),
        "a reply is conversation and never leaves the device"
    );
    assert_eq!(answers[&3]["error"]["message"], "links_read_limit");
}
